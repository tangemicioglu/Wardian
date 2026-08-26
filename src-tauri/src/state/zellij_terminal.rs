use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use wardian_core::models::{TerminalLaunchManifest, TERMINAL_LAUNCH_MANIFEST_SCHEMA};

pub const ZELLIJ_VERSION: &str = "0.45.0";
#[cfg(windows)]
const WINDOWS_ATTACHED_CLIENT_START_ATTEMPTS: usize = 4;
#[cfg(not(test))]
const PANE_ID_RECONCILIATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const PANE_ID_RECONCILIATION_TIMEOUT: std::time::Duration = std::time::Duration::ZERO;
#[cfg(not(test))]
const PANE_CLEANUP_CONFIRMATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const PANE_CLEANUP_CONFIRMATION_TIMEOUT: std::time::Duration = std::time::Duration::ZERO;
#[cfg(not(test))]
const ACTIVATION_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
#[cfg(test)]
const ACTIVATION_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);
const ACTIVATION_COMMAND_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);
const ACTIVATION_PROCESS_TERMINATION_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(2);
const MANAGED_PANE_MARKER_SCHEMA: u32 = 1;
const MANAGED_PANE_MARKER_MAX_BYTES: u64 = 16 * 1024;

fn zellij_helper_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    crate::utils::process::apply_silent_std_command_policy(&mut command);
    command
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZellijEnginePhase {
    Stopped,
    Starting,
    Running,
    Reattaching,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZellijPanePhase {
    Starting,
    Running,
    Exited,
    Closing,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ZellijPaneId(String);

impl ZellijPaneId {
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        let suffix = trimmed
            .strip_prefix("terminal_")
            .ok_or_else(|| "Zellij returned a non-terminal pane identity".to_string())?;
        if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("Zellij returned an invalid terminal pane identity".to_string());
        }
        Ok(Self(format!("terminal_{suffix}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ZellijPaneBinding {
    pub session_id: String,
    pub pane_id: Option<ZellijPaneId>,
    pub generation: u64,
    pub phase: ZellijPanePhase,
}

pub struct ZellijPaneLease {
    engine: Arc<ZellijTerminalEngine>,
    session_id: String,
    generation: u64,
    cleanup_scheduled: bool,
}

impl ZellijPaneLease {
    pub(crate) fn new(
        engine: Arc<ZellijTerminalEngine>,
        session_id: String,
        generation: u64,
    ) -> Self {
        Self {
            engine,
            session_id,
            generation,
            cleanup_scheduled: false,
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Closes this generation and confirms that its provider pane disappeared.
    /// A failed confirmation remains registered as `Closing` for retry.
    pub async fn close(mut self) -> Result<(), String> {
        self.cleanup_scheduled = true;
        let result = self
            .engine
            .close_pane(&self.session_id, self.generation)
            .await;
        if result.is_err() {
            self.cleanup_scheduled = false;
        }
        result
    }
}

impl Drop for ZellijPaneLease {
    fn drop(&mut self) {
        if !self.cleanup_scheduled {
            self.engine
                .schedule_pane_cleanup(&self.session_id, self.generation);
        }
    }
}

pub struct ZellijPaneTransport {
    reader: Option<Box<dyn std::io::Read + Send>>,
    snapshot_frames: Option<std::sync::mpsc::Receiver<Vec<u8>>>,
    runtime: Option<crate::state::terminal_session::TerminalRuntimeHandles>,
    subscription: Option<std::process::Child>,
    lease: Option<ZellijPaneLease>,
    subscription_cancelled: Arc<AtomicBool>,
    input_cancelled: Arc<AtomicBool>,
    subscription_reader: Option<std::thread::JoinHandle<()>>,
    input_worker: Option<std::thread::JoinHandle<()>>,
    handed_off: bool,
}

pub struct ZellijPaneActiveTransport {
    pub reader: Box<dyn std::io::Read + Send>,
    pub snapshot_frames: std::sync::mpsc::Receiver<Vec<u8>>,
    pub subscription: std::process::Child,
    pub lease: ZellijPaneLease,
}

impl ZellijPaneTransport {
    pub fn runtime(&self) -> crate::state::terminal_session::TerminalRuntimeHandles {
        self.runtime
            .as_ref()
            .expect("Zellij pane transport runtime must exist before handoff")
            .clone()
    }

    pub fn into_active(mut self) -> ZellijPaneActiveTransport {
        self.handed_off = true;
        let active = ZellijPaneActiveTransport {
            reader: self
                .reader
                .take()
                .expect("Zellij pane transport reader must exist before handoff"),
            snapshot_frames: self
                .snapshot_frames
                .take()
                .expect("Zellij snapshot receiver must exist before handoff"),
            subscription: self
                .subscription
                .take()
                .expect("Zellij subscription must exist before handoff"),
            lease: self
                .lease
                .take()
                .expect("Zellij pane lease must exist before handoff"),
        };
        // The broker owns the cloned runtime after a successful start. These
        // handles remain detached for the active generation and terminate when
        // the broker runtime and subscription child are torn down.
        self.runtime.take();
        self.subscription_reader.take();
        self.input_worker.take();
        active
    }

    /// Tears down a transport that never committed to the terminal broker.
    /// Every step is idempotent so callers can retry cleanup after an error.
    pub async fn shutdown(&mut self) -> Result<(), String> {
        let mut errors = self.shutdown_local_resources();
        if let Some(lease) = self.lease.take() {
            if let Err(error) = lease.close().await {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn shutdown_local_resources(&mut self) -> Vec<String> {
        if self.handed_off {
            return Vec::new();
        }
        self.subscription_cancelled.store(true, Ordering::Release);
        self.input_cancelled.store(true, Ordering::Release);
        self.runtime.take();
        self.reader.take();
        self.snapshot_frames.take();

        let mut errors = Vec::new();
        if let Some(mut subscription) = self.subscription.take() {
            if let Err(error) = terminate_subscription_process(&mut subscription) {
                errors.push(error);
            }
        }
        if let Some(reader) = self.subscription_reader.take() {
            if let Err(error) = join_transport_worker(reader, "subscription reader") {
                errors.push(error);
            }
        }
        if let Some(worker) = self.input_worker.take() {
            if let Err(error) = join_transport_worker(worker, "input worker") {
                errors.push(error);
            }
        }
        errors
    }
}

impl Drop for ZellijPaneTransport {
    fn drop(&mut self) {
        if self.handed_off {
            return;
        }
        let _ = self.shutdown_local_resources();
        // Dropping an unconsumed lease schedules generation-scoped pane cleanup.
    }
}

fn terminate_subscription_process(subscription: &mut std::process::Child) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = crate::utils::process::force_kill_process_tree(subscription.id());
    }
    let _ = subscription.kill();
    let deadline = std::time::Instant::now() + ACTIVATION_PROCESS_TERMINATION_TIMEOUT;
    loop {
        match subscription.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(ACTIVATION_COMMAND_POLL_INTERVAL);
            }
            Ok(None) => {
                return Err(
                    "Zellij pane subscription termination could not be confirmed".to_string(),
                )
            }
            Err(error) => {
                return Err(format!(
                    "Zellij pane subscription termination could not be confirmed: {error}"
                ))
            }
        }
    }
}

fn join_transport_worker(worker: std::thread::JoinHandle<()>, label: &str) -> Result<(), String> {
    let deadline = std::time::Instant::now() + ACTIVATION_PROCESS_TERMINATION_TIMEOUT;
    while !worker.is_finished() && std::time::Instant::now() < deadline {
        std::thread::sleep(ACTIVATION_COMMAND_POLL_INTERVAL);
    }
    if !worker.is_finished() {
        return Err(format!(
            "Zellij pane {label} termination could not be confirmed"
        ));
    }
    worker
        .join()
        .map_err(|_| format!("Zellij pane {label} terminated with a panic"))
}

fn send_subscription_frame(
    sender: &std::sync::mpsc::SyncSender<Vec<u8>>,
    mut frame: Vec<u8>,
    cancelled: &AtomicBool,
) -> bool {
    loop {
        if cancelled.load(Ordering::Acquire) {
            return false;
        }
        match sender.try_send(frame) {
            Ok(()) => return true,
            Err(std::sync::mpsc::TrySendError::Full(returned)) => {
                frame = returned;
                std::thread::sleep(ACTIVATION_COMMAND_POLL_INTERVAL);
            }
            Err(std::sync::mpsc::TrySendError::Disconnected(_)) => return false,
        }
    }
}

struct ZellijSnapshotReader {
    frames: std::sync::mpsc::Receiver<Vec<u8>>,
    pending: std::io::Cursor<Vec<u8>>,
}

impl ZellijSnapshotReader {
    fn new(frames: std::sync::mpsc::Receiver<Vec<u8>>) -> Self {
        Self {
            frames,
            pending: std::io::Cursor::new(Vec::new()),
        }
    }
}

impl std::io::Read for ZellijSnapshotReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        loop {
            let read = std::io::Read::read(&mut self.pending, buffer)?;
            if read > 0 {
                return Ok(read);
            }
            let frame = self.frames.recv().map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Zellij pane subscription ended",
                )
            })?;
            self.pending = std::io::Cursor::new(frame);
        }
    }
}

#[derive(Debug, Deserialize)]
struct ZellijPaneUpdate {
    event: String,
    pane_id: String,
    scrollback: Option<Vec<String>>,
    viewport: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ZellijPaneInfo {
    pub id: u32,
    pub is_plugin: bool,
    pub is_fullscreen: bool,
    pub title: String,
    pub exited: bool,
    pub exit_status: Option<i32>,
    pub pane_command: Option<String>,
    pub pane_cwd: Option<String>,
    pub pane_rows: u16,
    pub pane_columns: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ManagedPaneMarker {
    schema: u32,
    nonce: String,
    session_id: String,
}

impl ZellijPaneInfo {
    pub fn pane_id(&self) -> Option<ZellijPaneId> {
        (!self.is_plugin).then(|| ZellijPaneId(format!("terminal_{}", self.id)))
    }
}

#[derive(Debug, Clone)]
pub struct ZellijLaunchSpec {
    pub session_id: String,
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ZellijTerminalConfig {
    pub executable: PathBuf,
    pub wardian_cli: PathBuf,
    pub runtime_root: PathBuf,
    pub wardian_home: PathBuf,
    pub session_name: String,
}

impl ZellijTerminalConfig {
    pub fn from_resources(resources: &Path, wardian_home: &Path) -> Result<Self, String> {
        let binary_name = if cfg!(windows) {
            "zellij.exe"
        } else {
            "zellij"
        };
        let cli_name = if cfg!(windows) {
            "wardian-cli.exe"
        } else {
            "wardian-cli"
        };
        let executable = bundled_binary(resources, binary_name);
        let wardian_cli = bundled_binary(resources, cli_name);
        if !executable.is_file() {
            return Err(format!("Bundled Zellij {ZELLIJ_VERSION} is unavailable"));
        }
        if !wardian_cli.is_file() {
            return Err("Bundled Wardian terminal host is unavailable".to_string());
        }
        Ok(Self {
            executable,
            wardian_cli,
            runtime_root: wardian_home.join("runtime").join("zellij"),
            wardian_home: wardian_home.to_path_buf(),
            session_name: session_name_for_home(wardian_home),
        })
    }

    fn config_dir(&self) -> PathBuf {
        self.runtime_root.join("config")
    }

    fn launches_dir(&self) -> PathBuf {
        self.runtime_root.join("launches")
    }

    fn managed_panes_dir(&self) -> PathBuf {
        self.runtime_root.join("managed-panes")
    }

    fn managed_pane_marker_path(&self, nonce: &str) -> PathBuf {
        self.managed_panes_dir().join(format!("{nonce}.json"))
    }

    #[cfg(windows)]
    fn attached_pid_path(&self) -> PathBuf {
        self.runtime_root.join("attached-client.pid")
    }
}

fn bundled_binary(resources: &Path, name: &str) -> PathBuf {
    let direct = resources.join("bin").join(name);
    if direct.is_file() {
        direct
    } else {
        resources.join("resources").join("bin").join(name)
    }
}

fn session_name_for_home(home: &Path) -> String {
    #[cfg(windows)]
    let identity = home
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
        .into_bytes();
    #[cfg(unix)]
    let identity = {
        use std::os::unix::ffi::OsStrExt;
        home.as_os_str().as_bytes().to_vec()
    };
    #[cfg(not(any(windows, unix)))]
    let identity = home.to_string_lossy().into_owned().into_bytes();
    let digest = Sha256::digest(identity);
    format!("wardian-{}", hex_prefix(&digest, 12))
}

fn hex_prefix(bytes: &[u8], count: usize) -> String {
    bytes
        .iter()
        .flat_map(|byte| format!("{byte:02x}").chars().collect::<Vec<_>>())
        .take(count)
        .collect()
}

pub trait ZellijCommandRunner: Send + Sync {
    fn run(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
    ) -> Result<Output, String>;

    fn run_status(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
    ) -> Result<Output, String> {
        self.run(executable, args, env)
    }

    fn run_controlled(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
        cancelled: Arc<AtomicBool>,
        deadline: std::time::Instant,
    ) -> Result<Output, String> {
        if cancelled.load(Ordering::Acquire) {
            return Err("Terminal handoff was superseded".to_string());
        }
        if std::time::Instant::now() >= deadline {
            return Err("Terminal handoff timed out".to_string());
        }
        self.run(executable, args, env)
    }

    fn run_status_controlled(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
        cancelled: Arc<AtomicBool>,
        deadline: std::time::Instant,
    ) -> Result<Output, String> {
        if cancelled.load(Ordering::Acquire) {
            return Err("Terminal handoff was superseded".to_string());
        }
        if std::time::Instant::now() >= deadline {
            return Err("Terminal handoff timed out".to_string());
        }
        self.run_status(executable, args, env)
    }
}

#[derive(Debug, Default)]
pub struct ProcessZellijCommandRunner;

impl ZellijCommandRunner for ProcessZellijCommandRunner {
    fn run(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
    ) -> Result<Output, String> {
        zellij_helper_command(executable)
            .args(args)
            .envs(env.iter().cloned())
            .output()
            .map_err(|error| format!("Zellij command could not start: {error}"))
    }

    fn run_status(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
    ) -> Result<Output, String> {
        zellij_helper_command(executable)
            .args(args)
            .envs(env.iter().cloned())
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| format!("Zellij command could not start: {error}"))
    }

    fn run_controlled(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
        cancelled: Arc<AtomicBool>,
        deadline: std::time::Instant,
    ) -> Result<Output, String> {
        run_controlled_zellij_process(executable, args, env, cancelled, deadline)
    }

    fn run_status_controlled(
        &self,
        executable: &Path,
        args: &[String],
        env: &[(OsString, OsString)],
        cancelled: Arc<AtomicBool>,
        deadline: std::time::Instant,
    ) -> Result<Output, String> {
        run_controlled_zellij_process(executable, args, env, cancelled, deadline)
    }
}

fn run_controlled_zellij_process(
    executable: &Path,
    args: &[String],
    env: &[(OsString, OsString)],
    cancelled: Arc<AtomicBool>,
    deadline: std::time::Instant,
) -> Result<Output, String> {
    let mut stdout_file = tempfile::tempfile()
        .map_err(|error| format!("Zellij command stdout capture failed: {error}"))?;
    let mut stderr_file = tempfile::tempfile()
        .map_err(|error| format!("Zellij command stderr capture failed: {error}"))?;
    let mut command = zellij_helper_command(executable);
    command
        .args(args)
        .envs(env.iter().cloned())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file.try_clone().map_err(|error| {
            format!("Zellij command stdout capture failed: {error}")
        })?))
        .stderr(Stdio::from(stderr_file.try_clone().map_err(|error| {
            format!("Zellij command stderr capture failed: {error}")
        })?));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    let mut child_job = Some(crate::utils::process::create_kill_on_close_job(
        "Zellij activation helper",
    )?);
    let mut child = command
        .spawn()
        .map_err(|error| format!("Zellij command could not start: {error}"))?;
    let child_pid = child.id();
    #[cfg(windows)]
    if let Some(job) = child_job.as_ref() {
        if let Err(error) =
            crate::utils::process::assign_pid_to_job(job, child_pid, "Zellij activation helper")
        {
            if !child.try_wait().is_ok_and(|status| status.is_some()) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            child_job = None;
        }
    }

    let outcome = loop {
        if cancelled.load(Ordering::Acquire) {
            break Err("Terminal handoff was superseded".to_string());
        }
        if std::time::Instant::now() >= deadline {
            break Err("Terminal handoff timed out".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(ACTIVATION_COMMAND_POLL_INTERVAL),
            Err(error) => break Err(format!("Zellij command status failed: {error}")),
        }
    };

    if outcome.is_err() {
        terminate_controlled_zellij_process(
            &mut child,
            child_pid,
            #[cfg(windows)]
            &mut child_job,
        )?;
    }
    stdout_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("Zellij command stdout capture failed: {error}"))?;
    stderr_file
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("Zellij command stderr capture failed: {error}"))?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    stdout_file
        .read_to_end(&mut stdout)
        .map_err(|error| format!("Zellij command stdout failed: {error}"))?;
    stderr_file
        .read_to_end(&mut stderr)
        .map_err(|error| format!("Zellij command stderr failed: {error}"))?;
    outcome.map(|status| Output {
        status,
        stdout,
        stderr,
    })
}

fn terminate_controlled_zellij_process(
    child: &mut std::process::Child,
    child_pid: u32,
    #[cfg(windows)] child_job: &mut Option<win32job::Job>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = child_pid;
        // Closing a kill-on-close job terminates the helper and any descendants
        // even when `Child::kill` cannot reach a process that is changing state.
        drop(child_job.take());
    }
    #[cfg(unix)]
    unsafe {
        let result = libc::kill(-(child_pid as libc::pid_t), libc::SIGKILL);
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                let _ = child.kill();
            }
        }
    }
    let _ = child.kill();
    let deadline = std::time::Instant::now() + ACTIVATION_PROCESS_TERMINATION_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(ACTIVATION_COMMAND_POLL_INTERVAL);
            }
            Ok(None) => {
                return Err("Terminal handoff helper termination could not be confirmed".to_string())
            }
            Err(error) => {
                return Err(format!(
                    "Terminal handoff helper termination could not be confirmed: {error}"
                ))
            }
        }
    }
}

struct ZellijEngineState {
    phase: ZellijEnginePhase,
    attached: Option<ZellijAttachedClient>,
    attached_generation: Option<u64>,
    next_attached_generation: u64,
}

#[derive(Default)]
struct ZellijPaneRegistry {
    next_generation: u64,
    bindings: HashMap<String, ZellijPaneBinding>,
    replacement_bindings: HashMap<String, ZellijPaneBinding>,
    replacement_reservations: HashMap<String, u64>,
    retired_bindings: HashMap<(String, u64), ZellijPaneBinding>,
    managed_tokens: HashMap<(String, u64), String>,
}

impl ZellijPaneRegistry {
    fn binding_for_generation(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Option<&ZellijPaneBinding> {
        if self
            .bindings
            .get(session_id)
            .is_some_and(|binding| binding.generation == generation)
        {
            return self.bindings.get(session_id);
        }
        if self
            .replacement_bindings
            .get(session_id)
            .is_some_and(|binding| binding.generation == generation)
        {
            return self.replacement_bindings.get(session_id);
        }
        self.retired_bindings
            .get(&(session_id.to_string(), generation))
    }

    fn binding_for_generation_mut(
        &mut self,
        session_id: &str,
        generation: u64,
    ) -> Option<&mut ZellijPaneBinding> {
        if self
            .bindings
            .get(session_id)
            .is_some_and(|binding| binding.generation == generation)
        {
            return self.bindings.get_mut(session_id);
        }
        if self
            .replacement_bindings
            .get(session_id)
            .is_some_and(|binding| binding.generation == generation)
        {
            return self.replacement_bindings.get_mut(session_id);
        }
        self.retired_bindings
            .get_mut(&(session_id.to_string(), generation))
    }
}

struct ZellijAttachedClient {
    child: ZellijAttachedProcess,
    #[cfg(not(windows))]
    _master: crate::state::terminal_session::SharedPtyMaster,
}

enum ZellijAttachedProcess {
    #[cfg(windows)]
    NativeConsole(u32),
    #[cfg(not(windows))]
    Portable(Box<dyn portable_pty::Child + Send>),
}

impl Default for ZellijEngineState {
    fn default() -> Self {
        Self {
            phase: ZellijEnginePhase::Stopped,
            attached: None,
            attached_generation: None,
            next_attached_generation: 0,
        }
    }
}

impl ZellijEngineState {
    fn allocate_attached_generation(&mut self) -> Result<u64, String> {
        let generation = self
            .next_attached_generation
            .checked_add(1)
            .ok_or_else(|| "Zellij attached-client generation was exhausted".to_string())?;
        self.next_attached_generation = generation;
        Ok(generation)
    }

    #[cfg_attr(windows, allow(dead_code))]
    fn record_attached_exit(&mut self, generation: u64) -> bool {
        if self.attached_generation != Some(generation) {
            return false;
        }
        self.attached = None;
        self.attached_generation = None;
        self.phase = ZellijEnginePhase::Reattaching;
        true
    }
}

#[derive(Clone)]
struct ZellijActivationRequest {
    id: String,
    cancelled: Arc<AtomicBool>,
}

struct ZellijActionControl {
    cancelled: Arc<AtomicBool>,
    deadline: std::time::Instant,
}

impl ZellijActionControl {
    fn for_activation(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            deadline: std::time::Instant::now() + ACTIVATION_COMMAND_TIMEOUT,
        }
    }

    fn uncancelled() -> Self {
        Self::for_activation(Arc::new(AtomicBool::new(false)))
    }
}

pub struct ZellijTerminalEngine {
    config: ZellijTerminalConfig,
    runner: Arc<dyn ZellijCommandRunner>,
    state: Mutex<ZellijEngineState>,
    panes: std::sync::Mutex<ZellijPaneRegistry>,
    start_lock: Mutex<()>,
    activation_lock: Mutex<()>,
    latest_activation_request: std::sync::Mutex<Option<ZellijActivationRequest>>,
}

impl ZellijTerminalEngine {
    pub fn new(config: ZellijTerminalConfig) -> Self {
        Self::with_runner(config, Arc::new(ProcessZellijCommandRunner))
    }

    pub fn with_runner(config: ZellijTerminalConfig, runner: Arc<dyn ZellijCommandRunner>) -> Self {
        Self {
            config,
            runner,
            state: Mutex::new(ZellijEngineState::default()),
            panes: std::sync::Mutex::new(ZellijPaneRegistry::default()),
            start_lock: Mutex::new(()),
            activation_lock: Mutex::new(()),
            latest_activation_request: std::sync::Mutex::new(None),
        }
    }

    pub async fn phase(&self) -> ZellijEnginePhase {
        self.state.lock().await.phase
    }

    pub async fn set_phase(&self, phase: ZellijEnginePhase) {
        self.state.lock().await.phase = phase;
    }

    pub async fn binding(&self, session_id: &str) -> Option<ZellijPaneBinding> {
        self.pane_registry().bindings.get(session_id).cloned()
    }

    /// Returns whether pane and broker generations are inside one replacement transaction.
    pub fn replacement_pending(&self, session_id: &str) -> bool {
        self.pane_registry()
            .replacement_reservations
            .contains_key(session_id)
    }

    pub async fn reserve_replacement(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        let _activation_guard = self.activation_lock.lock().await;
        self.retry_pending_session_cleanup(session_id).await?;
        let mut panes = self.pane_registry();
        let active = panes
            .bindings
            .get(session_id)
            .ok_or_else(|| "Agent has no Zellij pane to replace".to_string())?;
        if active.generation != generation || active.phase != ZellijPanePhase::Running {
            return Err("Agent Zellij pane generation is stale".to_string());
        }
        if panes.replacement_bindings.contains_key(session_id) {
            return Err("Agent already has a Zellij replacement pending".to_string());
        }
        if panes
            .replacement_reservations
            .get(session_id)
            .is_some_and(|reserved_generation| *reserved_generation == generation)
        {
            return Ok(());
        }
        panes
            .replacement_reservations
            .insert(session_id.to_string(), generation);
        Ok(())
    }

    pub async fn promote_replacement(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        let mut panes = self.pane_registry();
        let expected = panes
            .replacement_reservations
            .get(session_id)
            .copied()
            .ok_or_else(|| "Agent has no Zellij replacement reservation".to_string())?;
        let replacement = panes
            .replacement_bindings
            .get(session_id)
            .filter(|binding| {
                binding.generation == generation && binding.phase == ZellijPanePhase::Running
            })
            .cloned()
            .ok_or_else(|| "Agent Zellij replacement is not ready".to_string())?;
        let displaced = panes
            .bindings
            .get(session_id)
            .cloned()
            .ok_or_else(|| "Agent Zellij replacement lost its active pane".to_string())?;
        if displaced.generation != expected {
            return Err("Agent Zellij replacement reservation is stale".to_string());
        }
        panes.replacement_bindings.remove(session_id);
        panes.bindings.insert(session_id.to_string(), replacement);
        panes
            .retired_bindings
            .insert((session_id.to_string(), displaced.generation), displaced);
        Ok(())
    }

    pub async fn rollback_promoted_replacement(
        &self,
        session_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        let mut panes = self.pane_registry();
        let displaced_generation = panes
            .replacement_reservations
            .get(session_id)
            .copied()
            .ok_or_else(|| "Agent has no Zellij replacement reservation".to_string())?;
        let candidate = panes
            .bindings
            .get(session_id)
            .filter(|binding| binding.generation == generation)
            .cloned()
            .ok_or_else(|| "Agent Zellij replacement is not active".to_string())?;
        let displaced = panes
            .retired_bindings
            .get(&(session_id.to_string(), displaced_generation))
            .cloned()
            .ok_or_else(|| "Agent Zellij replacement lost its displaced pane".to_string())?;
        panes.bindings.remove(session_id);
        panes
            .retired_bindings
            .remove(&(session_id.to_string(), displaced_generation));
        panes.bindings.insert(session_id.to_string(), displaced);
        panes
            .replacement_bindings
            .insert(session_id.to_string(), candidate);
        Ok(())
    }

    pub fn finalize_replacement(&self, session_id: &str) {
        self.pane_registry()
            .replacement_reservations
            .remove(session_id);
    }

    pub async fn cancel_replacement(&self, session_id: &str) -> Result<(), String> {
        let candidate = {
            let panes = self.pane_registry();
            panes.replacement_bindings.get(session_id).cloned()
        };
        if let Some(candidate) = candidate {
            self.close_pane(session_id, candidate.generation).await?;
        } else {
            self.pane_registry()
                .replacement_reservations
                .remove(session_id);
        }
        Ok(())
    }

    fn pane_registry(&self) -> std::sync::MutexGuard<'_, ZellijPaneRegistry> {
        self.panes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn finish_pane_subscription(&self, session_id: &str, generation: u64, downstream_closed: bool) {
        if downstream_closed {
            return;
        }
        let mut panes = self.pane_registry();
        if let Some(binding) = panes.binding_for_generation_mut(session_id, generation) {
            if binding.generation == generation && binding.phase == ZellijPanePhase::Running {
                binding.phase = ZellijPanePhase::Exited;
            }
        }
    }

    pub async fn attached_runtime_generation(&self) -> Option<u64> {
        self.state.lock().await.attached_generation
    }

    pub async fn attached_exit_status(&self) -> Result<Option<u32>, String> {
        let mut state = self.state.lock().await;
        let Some(attached) = state.attached.as_mut() else {
            return Ok(None);
        };
        match &mut attached.child {
            #[cfg(windows)]
            ZellijAttachedProcess::NativeConsole(pid) => {
                Ok((!crate::utils::process::process_exists(*pid)).then_some(0))
            }
            #[cfg(not(windows))]
            ZellijAttachedProcess::Portable(child) => child
                .try_wait()
                .map(|status| status.map(|status| status.exit_code()))
                .map_err(|error| error.to_string()),
        }
    }

    pub async fn start_attached_client(self: &Arc<Self>) -> Result<u64, String> {
        let _start_guard = self.start_lock.lock().await;
        {
            let mut state = self.state.lock().await;
            if let Some(attached) = state.attached.as_mut() {
                let alive = match &mut attached.child {
                    #[cfg(windows)]
                    ZellijAttachedProcess::NativeConsole(pid) => {
                        crate::utils::process::process_exists(*pid)
                    }
                    #[cfg(not(windows))]
                    ZellijAttachedProcess::Portable(child) => {
                        child.try_wait().is_ok_and(|status| status.is_none())
                    }
                };
                if alive {
                    state.phase = ZellijEnginePhase::Running;
                    return state
                        .attached_generation
                        .ok_or_else(|| "Zellij attached client lost its generation".to_string());
                }
                state.attached = None;
                state.attached_generation = None;
                state.phase = ZellijEnginePhase::Reattaching;
            }
            state.phase = ZellijEnginePhase::Starting;
        }

        if let Err(error) = self.prepare_runtime_directories() {
            self.set_phase(ZellijEnginePhase::Failed).await;
            return Err(error);
        }

        #[cfg(windows)]
        if let Some(pid) = std::fs::read_to_string(self.config.attached_pid_path())
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
        {
            if crate::utils::process::process_exists(pid) && self.list_panes().await.is_ok() {
                if let Err(error) = self.close_unregistered_managed_panes().await {
                    self.set_phase(ZellijEnginePhase::Failed).await;
                    return Err(error);
                }
                let mut state = self.state.lock().await;
                let runtime_generation = state.allocate_attached_generation()?;
                state.attached = Some(ZellijAttachedClient {
                    child: ZellijAttachedProcess::NativeConsole(pid),
                });
                state.attached_generation = Some(runtime_generation);
                state.phase = ZellijEnginePhase::Running;
                return Ok(runtime_generation);
            }
        }

        #[cfg(not(windows))]
        if self.list_panes().await.is_err() {
            let executable = self.config.executable.clone();
            let args = vec![
                "attach".to_string(),
                "--create-background".to_string(),
                self.config.session_name.clone(),
            ];
            let env = vec![
                (
                    OsString::from("ZELLIJ_CONFIG_DIR"),
                    self.config.config_dir().into_os_string(),
                ),
                (
                    OsString::from("WARDIAN_HOME"),
                    self.config.wardian_home.clone().into_os_string(),
                ),
                (OsString::from("TERM"), OsString::from("xterm-256color")),
                (OsString::from("COLORTERM"), OsString::from("truecolor")),
            ];
            let bootstrap = tokio::task::spawn_blocking(move || {
                zellij_helper_command(executable)
                    .args(args)
                    .envs(env)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| format!("Zellij bootstrap task failed: {error}"))??;
            if !bootstrap.success() {
                self.set_phase(ZellijEnginePhase::Failed).await;
                return Err("Zellij background session could not start".to_string());
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if self.list_panes().await.is_ok() {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    self.set_phase(ZellijEnginePhase::Failed).await;
                    return Err("Zellij background session did not become ready".to_string());
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }

        #[cfg(windows)]
        {
            let pid_path = self.config.attached_pid_path();
            let mut attached_pid = None;
            let mut last_start_error = "Zellij background session did not become ready".to_string();
            for attempt in 0..WINDOWS_ATTACHED_CLIENT_START_ATTEMPTS {
                let pid = match self.spawn_windows_attached_client(&pid_path) {
                    Ok(pid) => pid,
                    Err(error) => {
                        last_start_error = error;
                        if attempt + 1 < WINDOWS_ATTACHED_CLIENT_START_ATTEMPTS {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                250 * (1_u64 << attempt),
                            ))
                            .await;
                        }
                        continue;
                    }
                };
                match self.wait_for_windows_session_ready(pid).await {
                    Ok(()) => {
                        attached_pid = Some(pid);
                        break;
                    }
                    Err(error) => {
                        last_start_error = error;
                        let _ = crate::utils::process::force_kill_process_tree(pid);
                        let _ = std::fs::remove_file(&pid_path);
                        if attempt + 1 < WINDOWS_ATTACHED_CLIENT_START_ATTEMPTS {
                            tokio::time::sleep(std::time::Duration::from_millis(
                                250 * (1_u64 << attempt),
                            ))
                            .await;
                        }
                    }
                }
            }
            let Some(pid) = attached_pid else {
                self.set_phase(ZellijEnginePhase::Failed).await;
                return Err(last_start_error);
            };
            if let Err(error) = self.close_unregistered_managed_panes().await {
                self.set_phase(ZellijEnginePhase::Failed).await;
                return Err(error);
            }
            let mut state = self.state.lock().await;
            let runtime_generation = state.allocate_attached_generation()?;
            state.attached = Some(ZellijAttachedClient {
                child: ZellijAttachedProcess::NativeConsole(pid),
            });
            state.attached_generation = Some(runtime_generation);
            state.phase = ZellijEnginePhase::Running;
            Ok(runtime_generation)
        }

        #[cfg(not(windows))]
        {
            if let Err(error) = self.close_unregistered_managed_panes().await {
                self.set_phase(ZellijEnginePhase::Failed).await;
                return Err(error);
            }
            let pty_system = portable_pty::native_pty_system();
            let pair = match pty_system.openpty(portable_pty::PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            }) {
                Ok(pair) => pair,
                Err(error) => {
                    self.set_phase(ZellijEnginePhase::Failed).await;
                    return Err(format!(
                        "Zellij attached client PTY could not open: {error}"
                    ));
                }
            };
            let mut command = portable_pty::CommandBuilder::new(&self.config.executable);
            command.arg("attach");
            command.arg(&self.config.session_name);
            command.env("ZELLIJ_CONFIG_DIR", self.config.config_dir());
            command.env("WARDIAN_HOME", &self.config.wardian_home);
            command.env("TERM", "xterm-256color");
            command.env("COLORTERM", "truecolor");

            let child = match pair.slave.spawn_command(command) {
                Ok(child) => child,
                Err(error) => {
                    self.set_phase(ZellijEnginePhase::Failed).await;
                    return Err(format!("Zellij attached client could not start: {error}"));
                }
            };
            let mut reader = pair.master.try_clone_reader().map_err(|error| {
                format!("Zellij attached client reader is unavailable: {error}")
            })?;
            let master: crate::state::terminal_session::SharedPtyMaster =
                Arc::new(std::sync::Mutex::new(pair.master));
            drop(pair.slave);

            let runtime_generation = {
                let mut state = self.state.lock().await;
                let runtime_generation = state.allocate_attached_generation()?;
                state.attached = Some(ZellijAttachedClient {
                    child: ZellijAttachedProcess::Portable(child),
                    _master: master,
                });
                state.attached_generation = Some(runtime_generation);
                state.phase = ZellijEnginePhase::Running;
                runtime_generation
            };
            let weak_engine = Arc::downgrade(self);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 8192];
                while std::io::Read::read(&mut reader, &mut buffer).is_ok_and(|read| read > 0) {
                    // The attached client is a hidden lifecycle process. Agent
                    // presentation data comes from generation-scoped pane
                    // subscriptions, so this stream only needs to be drained.
                }
                if let Some(engine) = weak_engine.upgrade() {
                    tauri::async_runtime::spawn(async move {
                        let mut state = engine.state.lock().await;
                        state.record_attached_exit(runtime_generation);
                    });
                }
            });
            Ok(runtime_generation)
        }
    }

    pub fn prepare_runtime_directories(&self) -> Result<(), String> {
        std::fs::create_dir_all(self.config.config_dir()).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(self.config.launches_dir()).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(self.config.managed_panes_dir())
            .map_err(|error| error.to_string())?;
        let config_path = self.config.config_dir().join("config.kdl");
        let config = concat!(
            "simplified_ui true\n",
            "pane_frames true\n",
            "session_serialization false\n",
            "show_startup_tips false\n",
            "default_layout \"compact\"\n",
        );
        std::fs::write(config_path, config).map_err(|error| error.to_string())?;
        self.cleanup_abandoned_manifests()
    }

    pub fn cleanup_abandoned_manifests(&self) -> Result<(), String> {
        let root = self.config.launches_dir();
        if !root.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(&root).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let path = entry.path();
            if path.parent() == Some(root.as_path())
                && path
                    .extension()
                    .is_some_and(|extension| extension == "json")
                && path.is_file()
            {
                std::fs::remove_file(path).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    pub async fn list_panes(&self) -> Result<Vec<ZellijPaneInfo>, String> {
        let output = self
            .run_action(vec![
                "action".to_string(),
                "list-panes".to_string(),
                "--all".to_string(),
                "--json".to_string(),
            ])
            .await?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Zellij returned invalid pane state: {error}"))
    }

    async fn list_panes_controlled(
        &self,
        control: &ZellijActionControl,
    ) -> Result<Vec<ZellijPaneInfo>, String> {
        let output = self
            .run_action_controlled(
                vec![
                    "action".to_string(),
                    "list-panes".to_string(),
                    "--all".to_string(),
                    "--json".to_string(),
                ],
                control,
            )
            .await?;
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Zellij returned invalid pane state: {error}"))
    }

    #[cfg(windows)]
    fn spawn_windows_attached_client(&self, pid_path: &Path) -> Result<u32, String> {
        match std::fs::remove_file(pid_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(
                    "Zellij native attached client PID handoff could not be prepared".to_string(),
                )
            }
        }
        let output = windows_attached_client_launcher_command(
            &self.config.executable,
            &[
                "attach".to_string(),
                "--create".to_string(),
                self.config.session_name.clone(),
            ],
            pid_path,
        )
        .env("ZELLIJ_CONFIG_DIR", self.config.config_dir())
        .env("WARDIAN_HOME", &self.config.wardian_home)
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("Zellij native attached client could not start: {error}"))?;

        let reported_pid = parse_windows_attached_client_pid(&output.stdout);
        let published_pid = std::fs::read_to_string(pid_path)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok());
        if !output.status.success() || reported_pid.is_none() || reported_pid != published_pid {
            if let Some(pid) = reported_pid.or(published_pid) {
                let _ = crate::utils::process::force_kill_process_tree(pid);
            }
            let _ = std::fs::remove_file(pid_path);
            return Err("Zellij native attached client PID handoff failed".to_string());
        }
        Ok(reported_pid.expect("validated attached-client PID"))
    }

    #[cfg(windows)]
    async fn wait_for_windows_session_ready(&self, pid: u32) -> Result<(), String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if self.list_panes().await.is_ok() {
                return Ok(());
            }
            if !crate::utils::process::process_exists(pid) {
                return Err(
                    "Zellij native attached client exited before its session was ready".to_string(),
                );
            }
            if std::time::Instant::now() >= deadline {
                return Err("Zellij background session did not become ready".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    fn managed_pane_markers(&self) -> Result<Vec<ManagedPaneMarker>, String> {
        let mut markers = Vec::new();
        let entries = match std::fs::read_dir(self.config.managed_panes_dir()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(markers),
            Err(_) => return Err("Managed Zellij pane registry is unavailable".to_string()),
        };
        for entry in entries {
            let entry =
                entry.map_err(|_| "Managed Zellij pane registry is unreadable".to_string())?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|_| "Managed Zellij pane marker is unreadable".to_string())?;
            if !file_type.is_file() {
                return Err("Managed Zellij pane marker is not a regular file".to_string());
            }
            let metadata = entry
                .metadata()
                .map_err(|_| "Managed Zellij pane marker is unreadable".to_string())?;
            if metadata.len() > MANAGED_PANE_MARKER_MAX_BYTES {
                return Err("Managed Zellij pane marker is oversized".to_string());
            }
            let bytes = std::fs::read(&path)
                .map_err(|_| "Managed Zellij pane marker is unreadable".to_string())?;
            let marker: ManagedPaneMarker = serde_json::from_slice(&bytes)
                .map_err(|_| "Managed Zellij pane marker is malformed".to_string())?;
            let expected_name = format!("{}.json", marker.nonce);
            if marker.schema != MANAGED_PANE_MARKER_SCHEMA
                || path.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str())
                || !valid_launch_nonce(&marker.nonce)
                || marker.session_id.trim().is_empty()
                || marker.session_id.contains('\0')
            {
                return Err("Managed Zellij pane marker identity is invalid".to_string());
            }
            markers.push(marker);
        }
        Ok(markers)
    }

    fn remove_managed_pane_marker(&self, nonce: &str) -> Result<(), String> {
        let path = self.config.managed_pane_marker_path(nonce);
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("Managed Zellij pane marker could not be removed".to_string()),
        }
    }

    async fn managed_panes(&self) -> Result<Vec<(ZellijPaneInfo, ManagedPaneMarker)>, String> {
        let markers = self.managed_pane_markers()?;
        Ok(self
            .list_panes()
            .await?
            .into_iter()
            .filter_map(|pane| {
                let marker = markers
                    .iter()
                    .find(|marker| pane_matches_managed_marker(&pane, marker))?
                    .clone();
                Some((pane, marker))
            })
            .collect())
    }

    async fn close_unregistered_managed_panes(&self) -> Result<(), String> {
        let registered = {
            let panes = self.pane_registry();
            panes
                .bindings
                .values()
                .chain(panes.replacement_bindings.values())
                .chain(panes.retired_bindings.values())
                .filter_map(|binding| binding.pane_id.clone())
                .collect::<std::collections::HashSet<_>>()
        };
        let managed = self.managed_panes().await?;
        let live_tokens = managed
            .iter()
            .map(|(_, marker)| marker.nonce.clone())
            .collect::<std::collections::HashSet<_>>();
        for (pane, marker) in managed {
            let Some(pane_id) = pane.pane_id() else {
                continue;
            };
            if registered.contains(&pane_id) {
                continue;
            }
            self.close_pane_id(&pane_id).await?;
            self.confirm_pane_closed(&pane_id).await?;
            self.remove_managed_pane_marker(&marker.nonce)?;
        }
        for marker in self.managed_pane_markers()? {
            if !live_tokens.contains(&marker.nonce) {
                self.remove_managed_pane_marker(&marker.nonce)?;
            }
        }
        Ok(())
    }

    async fn session_managed_panes(
        &self,
        session_id: &str,
    ) -> Result<Vec<(ZellijPaneId, String)>, String> {
        Ok(self
            .managed_panes()
            .await?
            .into_iter()
            .filter(|(_, marker)| marker.session_id == session_id)
            .filter_map(|(pane, marker)| pane.pane_id().map(|pane_id| (pane_id, marker.nonce)))
            .collect())
    }

    async fn confirm_pane_closed(&self, pane_id: &ZellijPaneId) -> Result<(), String> {
        let deadline = std::time::Instant::now() + PANE_CLEANUP_CONFIRMATION_TIMEOUT;
        loop {
            let still_present = self
                .list_panes()
                .await?
                .into_iter()
                .filter_map(|pane| pane.pane_id())
                .any(|current| current == *pane_id);
            if !still_present {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err("Zellij pane cleanup could not be confirmed".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    fn remove_binding_generation(&self, session_id: &str, generation: u64) {
        let managed_token = {
            let mut panes = self.pane_registry();
            if panes
                .bindings
                .get(session_id)
                .is_some_and(|binding| binding.generation == generation)
            {
                panes.bindings.remove(session_id);
            }
            if panes
                .replacement_bindings
                .get(session_id)
                .is_some_and(|binding| binding.generation == generation)
            {
                panes.replacement_bindings.remove(session_id);
                panes.replacement_reservations.remove(session_id);
            }
            panes
                .retired_bindings
                .remove(&(session_id.to_string(), generation));
            panes
                .managed_tokens
                .remove(&(session_id.to_string(), generation))
        };
        if let Some(token) = managed_token {
            let _ = self.remove_managed_pane_marker(&token);
        }
    }

    async fn close_unregistered_session_panes(&self, session_id: &str) -> Result<(), String> {
        let registered = {
            let panes = self.pane_registry();
            panes
                .bindings
                .get(session_id)
                .into_iter()
                .chain(panes.replacement_bindings.get(session_id))
                .chain(
                    panes
                        .retired_bindings
                        .iter()
                        .filter(|((retired_session_id, _), _)| retired_session_id == session_id)
                        .map(|(_, binding)| binding),
                )
                .filter_map(|binding| binding.pane_id.clone())
                .collect::<std::collections::HashSet<_>>()
        };
        let stale = self
            .session_managed_panes(session_id)
            .await?
            .into_iter()
            .filter(|(pane_id, _)| !registered.contains(pane_id))
            .collect::<Vec<_>>();
        if stale.is_empty() {
            return Ok(());
        }
        for (pane_id, nonce) in stale {
            self.close_pane_id(&pane_id).await?;
            self.confirm_pane_closed(&pane_id).await?;
            self.remove_managed_pane_marker(&nonce)?;
        }
        Ok(())
    }

    async fn close_pane_id(&self, pane_id: &ZellijPaneId) -> Result<(), String> {
        self.run_status_action(vec![
            "action".to_string(),
            "close-pane".to_string(),
            "--pane-id".to_string(),
            pane_id.as_str().to_string(),
        ])
        .await
        .map(|_| ())
    }

    async fn retry_pending_session_cleanup(&self, session_id: &str) -> Result<(), String> {
        let mut pending = {
            let panes = self.pane_registry();
            panes
                .bindings
                .get(session_id)
                .into_iter()
                .chain(panes.replacement_bindings.get(session_id))
                .chain(
                    panes
                        .retired_bindings
                        .iter()
                        .filter(|((retired_session_id, _), _)| retired_session_id == session_id)
                        .map(|(_, binding)| binding),
                )
                .filter(|binding| binding.phase == ZellijPanePhase::Closing)
                .cloned()
                .collect::<Vec<_>>()
        };
        if pending.is_empty() {
            return Ok(());
        }

        // Newer replacement generations are closed first so a failed retry
        // cannot destroy the displaced provider while its candidate survives.
        pending.sort_by_key(|binding| std::cmp::Reverse(binding.generation));
        for binding in pending.iter().filter(|binding| binding.pane_id.is_some()) {
            self.close_pane(session_id, binding.generation).await?;
        }

        let missing_identity_generations = {
            let panes = self.pane_registry();
            let bindings = panes
                .bindings
                .get(session_id)
                .into_iter()
                .chain(panes.replacement_bindings.get(session_id))
                .chain(
                    panes
                        .retired_bindings
                        .iter()
                        .filter(|((retired_session_id, _), _)| retired_session_id == session_id)
                        .map(|(_, binding)| binding),
                )
                .collect::<Vec<_>>();
            bindings
                .iter()
                .filter(|binding| {
                    binding.phase == ZellijPanePhase::Closing && binding.pane_id.is_none()
                })
                .map(|binding| binding.generation)
                .collect::<Vec<_>>()
        };
        if missing_identity_generations.is_empty() {
            return Ok(());
        }

        self.close_unregistered_session_panes(session_id).await?;
        for generation in missing_identity_generations {
            self.remove_binding_generation(session_id, generation);
        }
        Ok(())
    }

    async fn retain_start_cleanup(&self, session_id: &str, generation: u64) {
        let mut panes = self.pane_registry();
        if let Some(binding) = panes.binding_for_generation_mut(session_id, generation) {
            binding.phase = ZellijPanePhase::Closing;
            binding.pane_id = None;
        }
    }

    pub async fn create_pane(&self, launch: ZellijLaunchSpec) -> Result<ZellijPaneBinding, String> {
        validate_launch_spec(&launch)?;
        self.retry_pending_session_cleanup(&launch.session_id)
            .await?;
        let replacement_of = {
            let panes = self.pane_registry();
            panes
                .replacement_reservations
                .get(&launch.session_id)
                .copied()
        };
        let generation = {
            let mut panes = self.pane_registry();
            if replacement_of.is_none() && panes.bindings.contains_key(&launch.session_id) {
                return Err("Agent already has a Zellij pane transition in progress".to_string());
            }
            if let Some(expected) = replacement_of {
                if panes
                    .bindings
                    .get(&launch.session_id)
                    .is_none_or(|binding| {
                        binding.generation != expected || binding.phase != ZellijPanePhase::Running
                    })
                    || panes.replacement_bindings.contains_key(&launch.session_id)
                {
                    return Err("Agent Zellij replacement reservation is stale".to_string());
                }
            }
            panes.next_generation = panes.next_generation.saturating_add(1);
            let generation = panes.next_generation;
            let binding = ZellijPaneBinding {
                session_id: launch.session_id.clone(),
                pane_id: None,
                generation,
                phase: ZellijPanePhase::Starting,
            };
            if replacement_of.is_some() {
                panes
                    .replacement_bindings
                    .insert(launch.session_id.clone(), binding);
            } else {
                panes.bindings.insert(launch.session_id.clone(), binding);
            }
            generation
        };

        if replacement_of.is_none() {
            if let Err(error) = self
                .close_unregistered_session_panes(&launch.session_id)
                .await
            {
                self.rollback_start(&launch.session_id, generation).await;
                return Err(error);
            }
        }
        let known_panes = self
            .list_panes()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|pane| pane.pane_id())
            .collect::<std::collections::HashSet<_>>();

        let nonce = Uuid::new_v4().simple().to_string();
        let (launch_path, marker_path, pane_command) =
            match prepare_pane_launch(&self.config, &launch, &nonce) {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.rollback_start(&launch.session_id, generation).await;
                    return Err(error);
                }
            };

        let mut action = vec![
            "action".to_string(),
            "new-pane".to_string(),
            "--name".to_string(),
            format!("wardian:{}", launch.session_id),
            "--no-focus".to_string(),
            "--".to_string(),
        ];
        action.extend(pane_command);
        let result = self.run_status_action(action).await;

        if result.is_err() {
            let _ = std::fs::remove_file(&launch_path);
            let _ = std::fs::remove_file(&marker_path);
        }
        let output = match result {
            Ok(output) => output,
            Err(error) => {
                self.rollback_start(&launch.session_id, generation).await;
                return Err(error);
            }
        };
        let pane_id = match parse_created_pane_id(&output.stdout) {
            Ok(pane_id) => pane_id,
            Err(_) => {
                let deadline = std::time::Instant::now() + PANE_ID_RECONCILIATION_TIMEOUT;
                loop {
                    if let Ok(panes) = self.list_panes().await {
                        if let Some(pane_id) = panes.into_iter().find_map(|pane| {
                            let pane_id = pane.pane_id()?;
                            (pane_matches_managed_marker(
                                &pane,
                                &ManagedPaneMarker {
                                    schema: MANAGED_PANE_MARKER_SCHEMA,
                                    nonce: nonce.clone(),
                                    session_id: launch.session_id.clone(),
                                },
                            ) && !known_panes.contains(&pane_id))
                            .then_some(pane_id)
                        }) {
                            break pane_id;
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        match self
                            .close_unregistered_session_panes(&launch.session_id)
                            .await
                        {
                            Ok(()) => {
                                self.rollback_start(&launch.session_id, generation).await;
                                return Err(
                                    "Zellij did not report the created pane identity".to_string()
                                );
                            }
                            Err(_) => {
                                self.retain_start_cleanup(&launch.session_id, generation)
                                    .await;
                                return Err(
                                    "Zellij did not report the created pane identity; pane cleanup is pending"
                                        .to_string(),
                                );
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            }
        };
        let binding = {
            let mut panes = self.pane_registry();
            let candidate = if replacement_of.is_some() {
                panes.replacement_bindings.get_mut(&launch.session_id)
            } else {
                panes.bindings.get_mut(&launch.session_id)
            };
            let result = match candidate {
                None => Err("Zellij pane start lost its agent binding".to_string()),
                Some(binding)
                    if binding.generation != generation
                        || binding.phase != ZellijPanePhase::Starting =>
                {
                    Err("Zellij pane start was superseded".to_string())
                }
                Some(binding) => {
                    binding.pane_id = Some(pane_id.clone());
                    binding.phase = ZellijPanePhase::Running;
                    Ok(binding.clone())
                }
            };
            if result.is_ok() {
                panes
                    .managed_tokens
                    .insert((launch.session_id.clone(), generation), nonce.clone());
            }
            result
        };
        match binding {
            Ok(binding) => Ok(binding),
            Err(error) => {
                let _ = self.close_pane_id(&pane_id).await;
                let _ = self.remove_managed_pane_marker(&nonce);
                self.rollback_start(&launch.session_id, generation).await;
                Err(error)
            }
        }
    }

    async fn cleanup_failed_pane_transport(
        &self,
        binding: &ZellijPaneBinding,
        pane_id: &ZellijPaneId,
    ) -> Result<(), String> {
        {
            let mut panes = self.pane_registry();
            let current = panes
                .binding_for_generation_mut(&binding.session_id, binding.generation)
                .ok_or_else(|| "Agent Zellij pane generation is stale".to_string())?;
            current.phase = ZellijPanePhase::Closing;
        }
        self.close_pane_id(pane_id).await?;
        self.confirm_pane_closed(pane_id).await?;
        self.remove_binding_generation(&binding.session_id, binding.generation);
        Ok(())
    }

    pub async fn open_pane_transport(
        self: &Arc<Self>,
        binding: &ZellijPaneBinding,
    ) -> Result<ZellijPaneTransport, String> {
        if binding.phase != ZellijPanePhase::Running {
            return Err("Agent Zellij pane is not running".to_string());
        }
        let pane_id = binding
            .pane_id
            .clone()
            .ok_or_else(|| "Agent Zellij pane is not ready".to_string())?;
        let mut command = zellij_helper_command(&self.config.executable);
        command
            .args([
                "--session",
                &self.config.session_name,
                "subscribe",
                "--pane-id",
                pane_id.as_str(),
                "--scrollback",
                "1000",
                "--format",
                "json",
                "--ansi",
            ])
            .env("ZELLIJ_CONFIG_DIR", self.config.config_dir())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let mut subscription = match command.spawn() {
            Ok(subscription) => subscription,
            Err(error) => {
                let message = format!("Zellij pane subscription could not start: {error}");
                if self
                    .cleanup_failed_pane_transport(binding, &pane_id)
                    .await
                    .is_err()
                {
                    return Err(format!("{message}; pane cleanup is pending"));
                }
                return Err(message);
            }
        };
        let stdout = match subscription.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = subscription.kill();
                let _ = subscription.wait();
                let message = "Zellij pane subscription has no output stream".to_string();
                if self
                    .cleanup_failed_pane_transport(binding, &pane_id)
                    .await
                    .is_err()
                {
                    return Err(format!("{message}; pane cleanup is pending"));
                }
                return Err(message);
            }
        };
        let lease =
            ZellijPaneLease::new(self.clone(), binding.session_id.clone(), binding.generation);
        let (render_tx, render_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
        let (snapshot_tx, snapshot_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
        let expected_pane = pane_id.as_str().to_string();
        let subscription_cancelled = Arc::new(AtomicBool::new(false));
        let reader_cancelled = Arc::clone(&subscription_cancelled);
        let subscription_engine = Arc::downgrade(self);
        let subscription_session_id = binding.session_id.clone();
        let subscription_generation = binding.generation;
        let subscription_reader = std::thread::spawn(move || {
            use std::io::BufRead;
            let mut downstream_closed = false;
            for line in std::io::BufReader::new(stdout).lines() {
                if reader_cancelled.load(Ordering::Acquire) {
                    downstream_closed = true;
                    break;
                }
                let Ok(line) = line else {
                    break;
                };
                let Ok(update) = serde_json::from_str::<ZellijPaneUpdate>(&line) else {
                    continue;
                };
                if update.event != "pane_update" || update.pane_id != expected_pane {
                    continue;
                }
                let frame = render_zellij_snapshot(update);
                if !send_subscription_frame(&snapshot_tx, frame.clone(), &reader_cancelled)
                    || !send_subscription_frame(&render_tx, frame, &reader_cancelled)
                {
                    // A broker replacement or termination drops Wardian's
                    // local frame receiver while the provider pane remains
                    // alive in Zellij. That is not pane-exit evidence and must
                    // not invalidate a generation-scoped restart reservation.
                    downstream_closed = true;
                    break;
                }
            }
            if let Some(engine) = subscription_engine.upgrade() {
                engine.finish_pane_subscription(
                    &subscription_session_id,
                    subscription_generation,
                    downstream_closed,
                );
            }
        });

        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<
            crate::state::terminal_session::NativeTerminalWriteRequest,
        >(256);
        let input_cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&input_cancelled);
        let input_engine = self.clone();
        let input_session_id = binding.session_id.clone();
        let input_generation = binding.generation;
        let input_worker = std::thread::spawn(move || loop {
            if worker_cancelled.load(Ordering::Acquire) {
                break;
            }
            let request = match input_rx.try_recv() {
                Ok(request) => request,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    std::thread::sleep(ACTIVATION_COMMAND_POLL_INTERVAL);
                    continue;
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            };
            let result = tauri::async_runtime::block_on(input_engine.write_to_pane(
                &input_session_id,
                input_generation,
                &request.bytes,
            ));
            let failed = result.is_err();
            let _ = request.completion.send(result);
            if failed {
                break;
            }
        });
        let runtime = crate::state::terminal_session::TerminalRuntimeHandles::new_with_write_ack(
            input_tx,
            |_geometry| Ok(()),
        )
        .fixed_geometry(wardian_core::models::TerminalGeometry {
            cols: 120,
            rows: 40,
        })
        .reset_parser_on_scrollback_erase();
        Ok(ZellijPaneTransport {
            reader: Some(Box::new(ZellijSnapshotReader::new(render_rx))),
            snapshot_frames: Some(snapshot_rx),
            runtime: Some(runtime),
            subscription: Some(subscription),
            lease: Some(lease),
            subscription_cancelled,
            input_cancelled,
            subscription_reader: Some(subscription_reader),
            input_worker: Some(input_worker),
            handed_off: false,
        })
    }

    pub async fn write_to_pane(
        &self,
        session_id: &str,
        generation: u64,
        bytes: &[u8],
    ) -> Result<(), String> {
        let pane = self.live_pane(session_id, generation).await?;
        let mut args = vec![
            "action".to_string(),
            "write".to_string(),
            "--pane-id".to_string(),
            pane.as_str().to_string(),
        ];
        args.extend(bytes.iter().map(u8::to_string));
        self.run_status_action(args).await.map(|_| ())
    }

    pub async fn focus_pane(&self, session_id: &str, generation: u64) -> Result<(), String> {
        let pane = self.live_pane(session_id, generation).await?;
        self.run_status_action(vec![
            "action".to_string(),
            "focus-pane-id".to_string(),
            pane.as_str().to_string(),
        ])
        .await
        .map(|_| ())
    }

    pub fn register_activation_request(&self, request_id: &str) {
        let mut latest = self
            .latest_activation_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = latest.take() {
            previous.cancelled.store(true, Ordering::Release);
        }
        *latest = Some(ZellijActivationRequest {
            id: request_id.to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
        });
    }

    pub fn cancel_activation_request(&self, request_id: &str) -> bool {
        let mut latest = self
            .latest_activation_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(request) = latest.as_ref() else {
            return false;
        };
        if request.id != request_id {
            return false;
        }
        request.cancelled.store(true, Ordering::Release);
        *latest = None;
        true
    }

    fn activation_request_is_current(&self, request_id: &str) -> bool {
        self.latest_activation_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|request| {
                request.id == request_id && !request.cancelled.load(Ordering::Acquire)
            })
    }

    fn activation_control(&self, request_id: &str) -> Result<ZellijActionControl, String> {
        let latest = self
            .latest_activation_request
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let request = latest
            .as_ref()
            .filter(|request| {
                request.id == request_id && !request.cancelled.load(Ordering::Acquire)
            })
            .ok_or_else(|| "Terminal handoff was superseded".to_string())?;
        Ok(ZellijActionControl::for_activation(Arc::clone(
            &request.cancelled,
        )))
    }

    fn ensure_activation_request_is_current(&self, request_id: Option<&str>) -> Result<(), String> {
        if request_id.is_some_and(|request_id| !self.activation_request_is_current(request_id)) {
            return Err("Terminal handoff was superseded".to_string());
        }
        Ok(())
    }

    async fn activate_pane_locked(
        &self,
        session_id: &str,
        generation: u64,
        request_id: Option<&str>,
        control: &ZellijActionControl,
    ) -> Result<(), String> {
        let target = self.live_pane(session_id, generation).await?;
        let panes = self.list_panes_controlled(control).await?;
        if let Some(fullscreen) = panes.iter().find(|pane| {
            !pane.is_plugin
                && pane.pane_id().is_some_and(|pane_id| pane_id != target)
                && pane.is_fullscreen
        }) {
            let fullscreen_id = fullscreen.pane_id().expect("terminal pane checked above");
            self.ensure_activation_request_is_current(request_id)?;
            self.run_status_action_controlled(
                vec![
                    "action".to_string(),
                    "toggle-no-ui-fullscreen".to_string(),
                    "--pane-id".to_string(),
                    fullscreen_id.as_str().to_string(),
                ],
                control,
            )
            .await?;
        }
        self.ensure_activation_request_is_current(request_id)?;
        self.run_status_action_controlled(
            vec![
                "action".to_string(),
                "focus-pane-id".to_string(),
                target.as_str().to_string(),
            ],
            control,
        )
        .await?;
        let target_is_fullscreen = panes.iter().any(|pane| {
            pane.pane_id().is_some_and(|pane_id| pane_id == target) && pane.is_fullscreen
        });
        if !target_is_fullscreen {
            self.ensure_activation_request_is_current(request_id)?;
            self.run_status_action_controlled(
                vec![
                    "action".to_string(),
                    "toggle-no-ui-fullscreen".to_string(),
                    "--pane-id".to_string(),
                    target.as_str().to_string(),
                ],
                control,
            )
            .await?;
        }
        Ok(())
    }

    pub async fn activate_pane(&self, session_id: &str, generation: u64) -> Result<(), String> {
        let _activation_guard = self.activation_lock.lock().await;
        self.activate_pane_locked(
            session_id,
            generation,
            None,
            &ZellijActionControl::uncancelled(),
        )
        .await
    }

    pub async fn activate_pane_for_request(
        &self,
        session_id: &str,
        generation: u64,
        request_id: &str,
    ) -> Result<(), String> {
        let _activation_guard = self.activation_lock.lock().await;
        if !self.activation_request_is_current(request_id) {
            return Err("Terminal handoff was superseded".to_string());
        }
        if self.replacement_pending(session_id) {
            return Err("Agent terminal restart is still settling".to_string());
        }
        let control = self.activation_control(request_id)?;
        self.activate_pane_locked(session_id, generation, Some(request_id), &control)
            .await
    }

    pub async fn dump_pane(
        &self,
        session_id: &str,
        generation: u64,
        ansi: bool,
    ) -> Result<Vec<u8>, String> {
        let pane = self.live_pane(session_id, generation).await?;
        let mut args = vec![
            "action".to_string(),
            "dump-screen".to_string(),
            "--full".to_string(),
            "--pane-id".to_string(),
            pane.as_str().to_string(),
        ];
        if ansi {
            args.push("--ansi".to_string());
        }
        self.run_action(args).await.map(|output| output.stdout)
    }

    pub async fn close_pane(&self, session_id: &str, generation: u64) -> Result<(), String> {
        let pane = {
            let mut panes = self.pane_registry();
            match panes.binding_for_generation_mut(session_id, generation) {
                None => None,
                Some(binding) => {
                    if !matches!(
                        binding.phase,
                        ZellijPanePhase::Running
                            | ZellijPanePhase::Exited
                            | ZellijPanePhase::Closing
                    ) {
                        return Err("Agent Zellij pane generation is stale".to_string());
                    }
                    let previous_phase = binding.phase;
                    binding.phase = ZellijPanePhase::Closing;
                    Some((binding.pane_id.clone(), previous_phase))
                }
            }
        };
        let Some((pane, previous_phase)) = pane else {
            return self.close_unregistered_session_panes(session_id).await;
        };
        let pane = pane.ok_or_else(|| {
            "Agent Zellij pane identity is unavailable; cleanup is pending".to_string()
        })?;
        if previous_phase == ZellijPanePhase::Exited {
            let pane_is_present = self
                .list_panes()
                .await?
                .into_iter()
                .filter_map(|candidate| candidate.pane_id())
                .any(|candidate| candidate == pane);
            if !pane_is_present {
                self.remove_binding_generation(session_id, generation);
                return Ok(());
            }
        }
        self.close_pane_id(&pane).await?;
        self.confirm_pane_closed(&pane).await?;
        self.remove_binding_generation(session_id, generation);
        Ok(())
    }

    async fn live_pane(&self, session_id: &str, generation: u64) -> Result<ZellijPaneId, String> {
        let panes = self.pane_registry();
        let binding = match panes.binding_for_generation(session_id, generation) {
            Some(binding) => binding,
            None if panes.bindings.contains_key(session_id)
                || panes.replacement_bindings.contains_key(session_id) =>
            {
                return Err("Agent Zellij pane generation is stale".to_string())
            }
            None => return Err("Agent has no Zellij pane".to_string()),
        };
        if binding.phase != ZellijPanePhase::Running {
            return Err("Agent Zellij pane generation is stale".to_string());
        }
        binding
            .pane_id
            .clone()
            .ok_or_else(|| "Agent Zellij pane is not ready".to_string())
    }

    async fn rollback_start(&self, session_id: &str, generation: u64) {
        self.remove_binding_generation(session_id, generation);
    }

    async fn run_action(&self, action: Vec<String>) -> Result<Output, String> {
        let executable = self.config.executable.clone();
        let runner = self.runner.clone();
        let mut args = vec!["--session".to_string(), self.config.session_name.clone()];
        args.extend(action);
        let env = vec![(
            OsString::from("ZELLIJ_CONFIG_DIR"),
            self.config.config_dir().into_os_string(),
        )];
        let output = tokio::task::spawn_blocking(move || runner.run(&executable, &args, &env))
            .await
            .map_err(|error| format!("Zellij command task failed: {error}"))??;
        if output.status.success() {
            Ok(output)
        } else {
            let detail = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "Zellij command failed: {}",
                redact_zellij_error(&detail)
            ))
        }
    }

    async fn run_action_controlled(
        &self,
        action: Vec<String>,
        control: &ZellijActionControl,
    ) -> Result<Output, String> {
        let executable = self.config.executable.clone();
        let runner = self.runner.clone();
        let mut args = vec!["--session".to_string(), self.config.session_name.clone()];
        args.extend(action);
        let env = vec![(
            OsString::from("ZELLIJ_CONFIG_DIR"),
            self.config.config_dir().into_os_string(),
        )];
        let cancelled = Arc::clone(&control.cancelled);
        let deadline = control.deadline;
        let result = tokio::task::spawn_blocking(move || {
            runner.run_controlled(&executable, &args, &env, cancelled, deadline)
        })
        .await
        .map_err(|error| format!("Zellij command task failed: {error}"))?;
        self.finish_controlled_action(result).await
    }

    async fn run_status_action(&self, action: Vec<String>) -> Result<Output, String> {
        let executable = self.config.executable.clone();
        let runner = self.runner.clone();
        let mut args = vec!["--session".to_string(), self.config.session_name.clone()];
        args.extend(action);
        let env = vec![(
            OsString::from("ZELLIJ_CONFIG_DIR"),
            self.config.config_dir().into_os_string(),
        )];
        let output =
            tokio::task::spawn_blocking(move || runner.run_status(&executable, &args, &env))
                .await
                .map_err(|error| format!("Zellij command task failed: {error}"))??;
        if output.status.success() {
            Ok(output)
        } else {
            Err("Zellij command failed".to_string())
        }
    }

    async fn run_status_action_controlled(
        &self,
        action: Vec<String>,
        control: &ZellijActionControl,
    ) -> Result<Output, String> {
        let executable = self.config.executable.clone();
        let runner = self.runner.clone();
        let mut args = vec!["--session".to_string(), self.config.session_name.clone()];
        args.extend(action);
        let env = vec![(
            OsString::from("ZELLIJ_CONFIG_DIR"),
            self.config.config_dir().into_os_string(),
        )];
        let cancelled = Arc::clone(&control.cancelled);
        let deadline = control.deadline;
        let result = tokio::task::spawn_blocking(move || {
            runner.run_status_controlled(&executable, &args, &env, cancelled, deadline)
        })
        .await
        .map_err(|error| format!("Zellij command task failed: {error}"))?;
        self.finish_controlled_action(result).await
    }

    async fn finish_controlled_action(
        &self,
        result: Result<Output, String>,
    ) -> Result<Output, String> {
        match result {
            Ok(output) if output.status.success() => Ok(output),
            Ok(output) => {
                let detail = String::from_utf8_lossy(&output.stderr);
                Err(format!(
                    "Zellij command failed: {}",
                    redact_zellij_error(&detail)
                ))
            }
            Err(error) if error == "Terminal handoff timed out" => {
                self.set_phase(ZellijEnginePhase::Reattaching).await;
                Err(error)
            }
            Err(error)
                if error
                    .starts_with("Terminal handoff helper termination could not be confirmed") =>
            {
                self.set_phase(ZellijEnginePhase::Failed).await;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    fn schedule_pane_cleanup(self: &Arc<Self>, session_id: &str, generation: u64) {
        let should_schedule = {
            let mut panes = self.pane_registry();
            let Some(binding) = panes.binding_for_generation_mut(session_id, generation) else {
                return;
            };
            if !matches!(
                binding.phase,
                ZellijPanePhase::Running | ZellijPanePhase::Exited | ZellijPanePhase::Closing
            ) {
                false
            } else {
                binding.phase = ZellijPanePhase::Closing;
                true
            }
        };
        if !should_schedule {
            return;
        }
        let engine = Arc::clone(self);
        let session_id = session_id.to_string();
        let _ = std::thread::Builder::new()
            .name("wardian-zellij-pane-cleanup".to_string())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                else {
                    return;
                };
                let _ = runtime.block_on(engine.close_pane(&session_id, generation));
            });
    }
}

fn validate_launch_spec(launch: &ZellijLaunchSpec) -> Result<(), String> {
    if launch.session_id.trim().is_empty()
        || launch.session_id.contains('\0')
        || launch.executable.trim().is_empty()
        || launch.executable.contains('\0')
        || launch.cwd.as_os_str().is_empty()
        || launch.cwd.to_string_lossy().contains('\0')
        || launch.args.iter().any(|arg| arg.contains('\0'))
        || launch.env.iter().any(|(key, value)| {
            key.trim().is_empty() || key.contains(['=', '\0']) || value.contains('\0')
        })
    {
        return Err("Agent terminal launch specification is invalid".to_string());
    }
    Ok(())
}

fn valid_launch_nonce(nonce: &str) -> bool {
    nonce.len() >= 32
        && nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn pane_matches_managed_marker(pane: &ZellijPaneInfo, marker: &ManagedPaneMarker) -> bool {
    if pane.is_plugin {
        return false;
    }
    pane.pane_command.as_deref().is_some_and(|command| {
        command.contains("terminal-host")
            && command
                .split_whitespace()
                .map(|argument| argument.trim_matches(['\'', '"']))
                .any(|argument| argument == marker.nonce)
    })
}

fn write_managed_pane_marker(
    config: &ZellijTerminalConfig,
    marker: &ManagedPaneMarker,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(config.managed_panes_dir())
        .map_err(|_| "Managed Zellij pane registry is unavailable".to_string())?;
    let path = config.managed_pane_marker_path(&marker.nonce);
    let bytes = serde_json::to_vec(marker).map_err(|_| "Managed Zellij pane marker is invalid")?;
    write_private_launch_file(&path, &bytes)
        .map_err(|_| "Managed Zellij pane marker could not be persisted".to_string())?;
    Ok(path)
}

fn prepare_pane_launch(
    config: &ZellijTerminalConfig,
    launch: &ZellijLaunchSpec,
    nonce: &str,
) -> Result<(PathBuf, PathBuf, Vec<String>), String> {
    let manifest_path = config.launches_dir().join(format!("{nonce}.json"));
    let marker = ManagedPaneMarker {
        schema: MANAGED_PANE_MARKER_SCHEMA,
        nonce: nonce.to_string(),
        session_id: launch.session_id.clone(),
    };
    let marker_path = write_managed_pane_marker(config, &marker)?;
    let manifest = TerminalLaunchManifest {
        schema: TERMINAL_LAUNCH_MANIFEST_SCHEMA,
        nonce: nonce.to_string(),
        session_id: launch.session_id.clone(),
        executable: launch.executable.clone(),
        args: launch.args.clone(),
        cwd: launch.cwd.to_string_lossy().to_string(),
        env: launch.env.clone(),
    };
    if let Err(error) = write_launch_manifest(&manifest_path, &manifest) {
        let _ = std::fs::remove_file(&marker_path);
        return Err(error);
    }
    Ok((
        manifest_path.clone(),
        marker_path,
        vec![
            config.wardian_cli.to_string_lossy().to_string(),
            "terminal-host".to_string(),
            "--manifest".to_string(),
            manifest_path.to_string_lossy().to_string(),
            "--nonce".to_string(),
            nonce.to_string(),
        ],
    ))
}

fn write_private_launch_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Terminal launch file could not be created".to_string())?;
    use std::io::Write;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Terminal launch file could not be persisted".to_string())
}

fn render_zellij_snapshot(update: ZellijPaneUpdate) -> Vec<u8> {
    let mut lines = update.scrollback.unwrap_or_default();
    lines.extend(update.viewport);
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let mut frame = Vec::from(&b"\x1b[3J\x1b[2J\x1b[H"[..]);
    frame.extend(lines.join("\r\n").as_bytes());
    frame
}

fn write_launch_manifest(path: &Path, manifest: &TerminalLaunchManifest) -> Result<(), String> {
    let bytes = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
    write_private_launch_file(path, &bytes)
}

fn parse_created_pane_id(bytes: &[u8]) -> Result<ZellijPaneId, String> {
    String::from_utf8_lossy(bytes)
        .split_whitespace()
        .rev()
        .find_map(|candidate| ZellijPaneId::parse(candidate).ok())
        .ok_or_else(|| "Zellij did not return the created pane identity".to_string())
}

fn redact_zellij_error(stderr: &str) -> String {
    let first = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    if first.is_empty() {
        "terminal engine unavailable".to_string()
    } else {
        let bounded: String = first.chars().take(160).collect();
        bounded
            .split_whitespace()
            .map(|part| {
                if part.contains(['\\', '/']) || part.contains("token") || part.contains("nonce") {
                    "[redacted]"
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(windows)]
fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(windows)]
fn windows_attached_client_launcher_command(
    executable: &Path,
    args: &[String],
    pid_path: &Path,
) -> Command {
    use base64::Engine as _;

    let argument_list = args
        .iter()
        .map(|argument| powershell_single_quoted(argument))
        .collect::<Vec<_>>()
        .join(", ");
    let quoted_pid_path = powershell_single_quoted(&pid_path.to_string_lossy());
    let script = format!(
        "$ErrorActionPreference = 'Stop'; $client = $null; try {{ $client = Start-Process -FilePath {} -ArgumentList @({argument_list}) -WindowStyle Hidden -PassThru; [Console]::Out.WriteLine(('WARDIAN_STARTED_PID=' + $client.Id)); [Console]::Out.Flush(); [System.IO.File]::WriteAllText({quoted_pid_path}, [string]$client.Id, [System.Text.Encoding]::ASCII); $published = [System.IO.File]::ReadAllText({quoted_pid_path}).Trim(); [uint32]$publishedPid = 0; if ((-not [uint32]::TryParse($published, [ref]$publishedPid)) -or $publishedPid -ne $client.Id) {{ throw 'PID publication validation failed' }} }} catch {{ if ($null -ne $client -and -not $client.HasExited) {{ & taskkill.exe /PID $client.Id /T /F *> $null }}; Remove-Item -LiteralPath {quoted_pid_path} -Force -ErrorAction SilentlyContinue; exit 1 }}",
        powershell_single_quoted(&executable.to_string_lossy()),
    );
    let encoded_bytes = script
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    let encoded = base64::engine::general_purpose::STANDARD.encode(encoded_bytes);
    let mut command = zellij_helper_command("powershell.exe");
    command.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        &encoded,
    ]);
    command
}

#[cfg(windows)]
fn parse_windows_attached_client_pid(stdout: &[u8]) -> Option<u32> {
    String::from_utf8_lossy(stdout).lines().find_map(|line| {
        line.trim()
            .strip_prefix("WARDIAN_STARTED_PID=")
            .and_then(|value| value.parse::<u32>().ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::process::ExitStatus;
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
    use std::sync::{Condvar, Mutex as StdMutex};

    #[cfg(windows)]
    fn status(code: i32) -> ExitStatus {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(code as u32)
    }

    #[cfg(unix)]
    fn status(code: i32) -> ExitStatus {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(code << 8)
    }

    fn sleeping_helper() -> (PathBuf, Vec<String>) {
        #[cfg(windows)]
        {
            (
                PathBuf::from("powershell.exe"),
                vec![
                    "-NoLogo".to_string(),
                    "-NoProfile".to_string(),
                    "-NonInteractive".to_string(),
                    "-Command".to_string(),
                    "Start-Sleep -Seconds 30".to_string(),
                ],
            )
        }
        #[cfg(not(windows))]
        {
            (
                PathBuf::from("/bin/sh"),
                vec!["-c".to_string(), "exec sleep 30".to_string()],
            )
        }
    }

    #[test]
    fn delayed_attached_exit_does_not_clear_replacement_generation() {
        let mut state = ZellijEngineState::default();
        let old_generation = state.allocate_attached_generation().unwrap();
        state.attached_generation = Some(old_generation);
        state.phase = ZellijEnginePhase::Running;
        let replacement_generation = state.allocate_attached_generation().unwrap();
        state.attached_generation = Some(replacement_generation);

        assert!(!state.record_attached_exit(old_generation));
        assert_eq!(state.attached_generation, Some(replacement_generation));
        assert_eq!(state.phase, ZellijEnginePhase::Running);
    }

    #[test]
    fn controlled_process_runner_terminates_helper_at_deadline() {
        let (executable, args) = sleeping_helper();
        let started = std::time::Instant::now();
        let result = ProcessZellijCommandRunner.run_status_controlled(
            &executable,
            &args,
            &[],
            Arc::new(AtomicBool::new(false)),
            started + std::time::Duration::from_millis(75),
        );

        assert_eq!(result, Err("Terminal handoff timed out".to_string()));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the killed helper must not hold the calling thread"
        );
    }

    #[test]
    fn controlled_process_runner_terminates_helper_when_cancelled() {
        let (executable, args) = sleeping_helper();
        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancelled);
        let cancel_thread = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            signal.store(true, AtomicOrdering::Release);
        });
        let started = std::time::Instant::now();
        let result = ProcessZellijCommandRunner.run_status_controlled(
            &executable,
            &args,
            &[],
            cancelled,
            started + std::time::Duration::from_secs(5),
        );
        cancel_thread.join().unwrap();

        assert_eq!(result, Err("Terminal handoff was superseded".to_string()));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "activation cancellation must terminate the helper"
        );
    }

    #[test]
    fn subscription_frame_backpressure_is_cancellation_aware() {
        let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
        sender.send(vec![1]).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = std::thread::spawn(move || {
            send_subscription_frame(&sender, vec![2], &worker_cancelled)
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        cancelled.store(true, AtomicOrdering::Release);

        assert!(!worker.join().unwrap());
    }

    #[derive(Default)]
    struct FakeRunner {
        calls: StdMutex<Vec<Vec<String>>>,
        outputs: StdMutex<VecDeque<Output>>,
        managed_nonce: StdMutex<Option<String>>,
    }

    impl FakeRunner {
        fn succeeding(stdout: &str) -> Arc<Self> {
            Arc::new(Self {
                calls: StdMutex::new(Vec::new()),
                outputs: StdMutex::new(VecDeque::from([
                    Output {
                        status: status(0),
                        stdout: b"[]".to_vec(),
                        stderr: Vec::new(),
                    },
                    Output {
                        status: status(0),
                        stdout: b"[]".to_vec(),
                        stderr: Vec::new(),
                    },
                    Output {
                        status: status(0),
                        stdout: stdout.as_bytes().to_vec(),
                        stderr: Vec::new(),
                    },
                ])),
                managed_nonce: StdMutex::new(None),
            })
        }

        fn with_outputs(outputs: impl IntoIterator<Item = Output>) -> Arc<Self> {
            Arc::new(Self {
                calls: StdMutex::new(Vec::new()),
                outputs: StdMutex::new(outputs.into_iter().collect()),
                managed_nonce: StdMutex::new(None),
            })
        }
    }

    impl ZellijCommandRunner for FakeRunner {
        fn run(
            &self,
            _executable: &Path,
            args: &[String],
            _env: &[(OsString, OsString)],
        ) -> Result<Output, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            if let Some(nonce) = args
                .windows(2)
                .find_map(|pair| (pair[0] == "--nonce").then(|| pair[1].clone()))
            {
                *self.managed_nonce.lock().unwrap() = Some(nonce);
            }
            let mut output = self
                .outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "missing fake output".to_string())?;
            if let Some(nonce) = self.managed_nonce.lock().unwrap().as_deref() {
                output.stdout = String::from_utf8_lossy(&output.stdout)
                    .replace("WARDIAN_MANAGED_NONCE", nonce)
                    .into_bytes();
            }
            Ok(output)
        }
    }

    #[tokio::test]
    async fn failed_broker_transport_shutdown_terminates_subscription_and_workers() {
        let temp = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let engine = Arc::new(ZellijTerminalEngine::with_runner(
            config(temp.path()),
            FakeRunner::with_outputs([successful(b""), successful(b"[]")]),
        ));
        let binding = ZellijPaneBinding {
            session_id: "failed-broker".to_string(),
            pane_id: Some(ZellijPaneId::parse("terminal_7").unwrap()),
            generation: 1,
            phase: ZellijPanePhase::Running,
        };
        engine
            .pane_registry()
            .bindings
            .insert(binding.session_id.clone(), binding.clone());

        let (executable, args) = sleeping_helper();
        let mut command = zellij_helper_command(executable);
        let mut subscription = command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let subscription_pid = subscription.id();
        let stdout = subscription.stdout.take().unwrap();
        let subscription_reader = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut bytes = Vec::new();
            let _ = reader.read_to_end(&mut bytes);
        });
        let (render_tx, render_rx) = std::sync::mpsc::sync_channel(1);
        let (snapshot_tx, snapshot_rx) = std::sync::mpsc::sync_channel(1);
        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<
            crate::state::terminal_session::NativeTerminalWriteRequest,
        >(1);
        let input_cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&input_cancelled);
        let input_worker = std::thread::spawn(move || loop {
            if worker_cancelled.load(Ordering::Acquire) {
                break;
            }
            match input_rx.try_recv() {
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    std::thread::sleep(ACTIVATION_COMMAND_POLL_INTERVAL);
                }
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            }
        });
        let runtime = crate::state::terminal_session::TerminalRuntimeHandles::new_with_write_ack(
            input_tx,
            |_geometry| Ok(()),
        );
        let mut transport = ZellijPaneTransport {
            reader: Some(Box::new(ZellijSnapshotReader::new(render_rx))),
            snapshot_frames: Some(snapshot_rx),
            runtime: Some(runtime),
            subscription: Some(subscription),
            lease: Some(ZellijPaneLease::new(
                engine.clone(),
                binding.session_id.clone(),
                binding.generation,
            )),
            subscription_cancelled: Arc::new(AtomicBool::new(false)),
            input_cancelled,
            subscription_reader: Some(subscription_reader),
            input_worker: Some(input_worker),
            handed_off: false,
        };
        drop(render_tx);
        drop(snapshot_tx);

        // Model a broker-start failure that still holds the runtime clone used
        // for the attempted registration. Explicit cancellation must stop the
        // input worker without relying on every sender having been dropped.
        let retained_runtime = transport.runtime();
        transport.shutdown().await.unwrap();
        transport.shutdown().await.unwrap();
        drop(retained_runtime);

        assert!(!crate::utils::process::process_exists(subscription_pid));
        assert!(engine.binding("failed-broker").await.is_none());
    }

    struct BlockingActivationRunner {
        calls: StdMutex<Vec<Vec<String>>>,
        list_started: AtomicBool,
        release_list: (StdMutex<bool>, Condvar),
    }

    impl BlockingActivationRunner {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: StdMutex::new(Vec::new()),
                list_started: AtomicBool::new(false),
                release_list: (StdMutex::new(false), Condvar::new()),
            })
        }

        fn release(&self) {
            let (released, signal) = &self.release_list;
            *released.lock().unwrap() = true;
            signal.notify_all();
        }
    }

    impl ZellijCommandRunner for BlockingActivationRunner {
        fn run(
            &self,
            _executable: &Path,
            args: &[String],
            _env: &[(OsString, OsString)],
        ) -> Result<Output, String> {
            self.calls.lock().unwrap().push(args.to_vec());
            if args.iter().any(|arg| arg == "list-panes") {
                self.list_started.store(true, AtomicOrdering::Release);
                let (released, signal) = &self.release_list;
                let mut released = released.lock().unwrap();
                while !*released {
                    released = signal.wait(released).unwrap();
                }
                return Ok(Output {
                    status: status(0),
                    stdout: br#"[{"id":1,"is_plugin":false,"is_fullscreen":false,"title":"wardian:agent-1","exited":false,"exit_status":null,"pane_command":"provider","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}]"#.to_vec(),
                    stderr: Vec::new(),
                });
            }
            Ok(Output {
                status: status(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    fn config(root: &Path) -> ZellijTerminalConfig {
        ZellijTerminalConfig {
            executable: root.join(if cfg!(windows) {
                "zellij.exe"
            } else {
                "zellij"
            }),
            wardian_cli: root.join(if cfg!(windows) {
                "wardian-cli.exe"
            } else {
                "wardian-cli"
            }),
            runtime_root: root.join("runtime"),
            wardian_home: root.join("home"),
            session_name: "wardian-test".to_string(),
        }
    }

    #[test]
    fn pane_id_parser_ignores_startup_noise_but_requires_terminal_identity() {
        assert_eq!(
            parse_created_pane_id(b"startup notice\r\nterminal_42\r\n")
                .unwrap()
                .as_str(),
            "terminal_42"
        );
        assert!(parse_created_pane_id(b"plugin_4").is_err());
    }

    #[tokio::test]
    async fn replacement_pane_promotion_can_restore_the_displaced_generation() {
        let temp = tempfile::tempdir().unwrap();
        let engine =
            ZellijTerminalEngine::with_runner(config(temp.path()), FakeRunner::with_outputs([]));
        {
            let mut panes = engine.pane_registry();
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
        }
        engine.reserve_replacement("agent-1", 1).await.unwrap();
        {
            engine.pane_registry().replacement_bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_2").unwrap()),
                    generation: 2,
                    phase: ZellijPanePhase::Running,
                },
            );
        }

        engine.promote_replacement("agent-1", 2).await.unwrap();
        assert_eq!(engine.binding("agent-1").await.unwrap().generation, 2);
        engine
            .rollback_promoted_replacement("agent-1", 2)
            .await
            .unwrap();
        assert_eq!(engine.binding("agent-1").await.unwrap().generation, 1);
        assert_eq!(
            engine
                .pane_registry()
                .replacement_bindings
                .get("agent-1")
                .unwrap()
                .generation,
            2
        );
    }

    #[tokio::test]
    async fn rejected_replacement_promotion_leaves_both_generations_registered() {
        let temp = tempfile::tempdir().unwrap();
        let engine =
            ZellijTerminalEngine::with_runner(config(temp.path()), FakeRunner::with_outputs([]));
        {
            let mut panes = engine.pane_registry();
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
        }
        engine.reserve_replacement("agent-1", 1).await.unwrap();
        {
            engine.pane_registry().replacement_bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_2").unwrap()),
                    generation: 2,
                    phase: ZellijPanePhase::Starting,
                },
            );
        }

        assert!(engine.promote_replacement("agent-1", 2).await.is_err());
        let panes = engine.pane_registry();
        assert_eq!(panes.bindings.get("agent-1").unwrap().generation, 1);
        assert_eq!(
            panes
                .replacement_bindings
                .get("agent-1")
                .unwrap()
                .generation,
            2
        );
        assert!(panes.retired_bindings.is_empty());
    }

    #[tokio::test]
    async fn failed_replacement_cancel_is_retried_before_an_ordinary_start() {
        let temp = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let runner = FakeRunner::with_outputs([
            Output {
                status: status(1),
                stdout: Vec::new(),
                stderr: b"close failed".to_vec(),
            },
            successful(b""),
            successful(b"[]"),
            successful(b""),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_3\n"),
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(temp.path()), runner.clone());
        engine.prepare_runtime_directories().unwrap();
        {
            let mut panes = engine.pane_registry();
            panes.next_generation = 2;
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
        }
        engine.reserve_replacement("agent-1", 1).await.unwrap();
        engine.pane_registry().replacement_bindings.insert(
            "agent-1".to_string(),
            ZellijPaneBinding {
                session_id: "agent-1".to_string(),
                pane_id: Some(ZellijPaneId::parse("terminal_2").unwrap()),
                generation: 2,
                phase: ZellijPanePhase::Running,
            },
        );

        engine
            .cancel_replacement("agent-1")
            .await
            .expect_err("failed close must retain the replacement cleanup record");
        {
            let panes = engine.pane_registry();
            assert_eq!(panes.replacement_reservations.get("agent-1"), Some(&1));
            assert_eq!(
                panes.replacement_bindings.get("agent-1").unwrap().phase,
                ZellijPanePhase::Closing,
            );
        }

        let launch = ZellijLaunchSpec {
            session_id: "agent-1".to_string(),
            executable: "provider".to_string(),
            args: Vec::new(),
            cwd: temp.path().to_path_buf(),
            env: BTreeMap::new(),
        };
        assert_eq!(
            engine.create_pane(launch.clone()).await.unwrap_err(),
            "Agent already has a Zellij pane transition in progress",
        );
        {
            let panes = engine.pane_registry();
            assert!(!panes.replacement_reservations.contains_key("agent-1"));
            assert!(!panes.replacement_bindings.contains_key("agent-1"));
            assert_eq!(panes.bindings.get("agent-1").unwrap().generation, 1);
        }

        engine.close_pane("agent-1", 1).await.unwrap();
        let replacement = engine
            .create_pane(launch)
            .await
            .expect("ordinary start may proceed only after every closing generation is gone");
        assert_eq!(replacement.generation, 3);
        assert_eq!(replacement.pane_id.unwrap().as_str(), "terminal_3");
        let calls = runner.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.iter().any(|argument| argument == "new-pane"))
                .count(),
            1,
        );
    }

    #[tokio::test]
    async fn failed_replacement_cancel_is_retried_before_the_next_resume_reservation() {
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner::with_outputs([
            Output {
                status: status(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
            Output {
                status: status(0),
                stdout: b"[]".to_vec(),
                stderr: Vec::new(),
            },
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(temp.path()), runner);
        {
            let mut panes = engine.pane_registry();
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
            panes
                .replacement_reservations
                .insert("agent-1".to_string(), 1);
            panes.replacement_bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_2").unwrap()),
                    generation: 2,
                    phase: ZellijPanePhase::Closing,
                },
            );
        }

        engine.reserve_replacement("agent-1", 1).await.unwrap();

        let panes = engine.pane_registry();
        assert!(!panes.replacement_bindings.contains_key("agent-1"));
        assert_eq!(panes.replacement_reservations.get("agent-1"), Some(&1));
    }

    #[tokio::test]
    async fn stale_post_commit_reservation_is_replaced_after_retired_cleanup() {
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner::with_outputs([
            Output {
                status: status(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
            Output {
                status: status(0),
                stdout: b"[]".to_vec(),
                stderr: Vec::new(),
            },
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(temp.path()), runner);
        {
            let mut panes = engine.pane_registry();
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_2").unwrap()),
                    generation: 2,
                    phase: ZellijPanePhase::Running,
                },
            );
            panes.retired_bindings.insert(
                ("agent-1".to_string(), 1),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Closing,
                },
            );
            panes
                .replacement_reservations
                .insert("agent-1".to_string(), 1);
        }

        engine.reserve_replacement("agent-1", 2).await.unwrap();

        let panes = engine.pane_registry();
        assert!(panes.retired_bindings.is_empty());
        assert_eq!(panes.replacement_reservations.get("agent-1"), Some(&2));
    }

    #[tokio::test]
    async fn missing_replacement_identity_cleanup_preserves_the_displaced_pane() {
        let temp = tempfile::tempdir().unwrap();
        let nonce = "0123456789abcdef0123456789abcdef";
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let both_panes = format!(
            r#"[{{"id":1,"is_plugin":false,"is_fullscreen":false,"title":"provider title","exited":false,"exit_status":null,"pane_command":"provider","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}},{{"id":2,"is_plugin":false,"is_fullscreen":false,"title":"mutated replacement title","exited":false,"exit_status":null,"pane_command":"wardian-cli terminal-host --nonce {nonce}","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}}]"#
        );
        let displaced_only = br#"[{"id":1,"is_plugin":false,"is_fullscreen":false,"title":"wardian:agent-1","exited":false,"exit_status":null,"pane_command":"provider","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}]"#;
        let runner = FakeRunner::with_outputs([
            successful(both_panes.as_bytes()),
            successful(b""),
            successful(displaced_only),
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(temp.path()), runner.clone());
        engine.prepare_runtime_directories().unwrap();
        write_managed_pane_marker(
            &engine.config,
            &ManagedPaneMarker {
                schema: MANAGED_PANE_MARKER_SCHEMA,
                nonce: nonce.to_string(),
                session_id: "agent-1".to_string(),
            },
        )
        .unwrap();
        {
            let mut panes = engine.pane_registry();
            panes.next_generation = 2;
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
            panes
                .replacement_reservations
                .insert("agent-1".to_string(), 1);
            panes.replacement_bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: None,
                    generation: 2,
                    phase: ZellijPanePhase::Closing,
                },
            );
        }

        let error = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: temp.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .expect_err("the live displaced pane still blocks an ordinary start");
        assert_eq!(
            error,
            "Agent already has a Zellij pane transition in progress"
        );
        let panes = engine.pane_registry();
        assert_eq!(panes.bindings.get("agent-1").unwrap().generation, 1);
        assert!(!panes.replacement_bindings.contains_key("agent-1"));
        assert!(!panes.replacement_reservations.contains_key("agent-1"));
        drop(panes);
        let calls = runner.calls.lock().unwrap();
        assert!(calls.iter().any(|call| {
            call.windows(2)
                .any(|arguments| arguments == ["--pane-id", "terminal_2"])
        }));
        assert!(!calls.iter().any(|call| {
            call.windows(2)
                .any(|arguments| arguments == ["--pane-id", "terminal_1"])
        }));
    }

    #[test]
    fn pane_update_renders_as_a_complete_terminal_frame() {
        let frame = render_zellij_snapshot(ZellijPaneUpdate {
            event: "pane_update".to_string(),
            pane_id: "terminal_7".to_string(),
            scrollback: Some(vec!["older".to_string()]),
            viewport: vec!["current".to_string(), String::new()],
        });

        assert_eq!(frame, b"\x1b[3J\x1b[2J\x1b[Holder\r\ncurrent");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn repeated_complete_frames_replace_canonical_history() {
        let broker = Arc::new(crate::state::terminal_session::TerminalSessionBroker::default());
        let (input_tx, _input_rx) = tokio::sync::mpsc::channel(1);
        let generation = broker
            .start_or_replace_runtime(
                "agent-1",
                crate::state::terminal_session::TerminalRuntimeHandles::new(
                    input_tx,
                    |_geometry| Ok(()),
                )
                .fixed_geometry(wardian_core::models::TerminalGeometry {
                    cols: 120,
                    rows: 40,
                })
                .reset_parser_on_scrollback_erase(),
                wardian_core::models::TerminalGeometry { cols: 80, rows: 24 },
            )
            .await
            .expect("start Zellij frame runtime");

        for viewport in ["first frame", "second frame"] {
            let frame = render_zellij_snapshot(ZellijPaneUpdate {
                event: "pane_update".to_string(),
                pane_id: "terminal_7".to_string(),
                scrollback: Some(vec!["stable history".to_string()]),
                viewport: vec![viewport.to_string()],
            });
            let reader_broker = broker.clone();
            std::thread::spawn(move || {
                crate::state::terminal_session::forward_terminal_output(
                    &reader_broker,
                    "agent-1",
                    generation,
                    frame,
                )
            })
            .join()
            .expect("Zellij reader thread")
            .expect("forward complete Zellij frame");
        }

        let snapshot = broker.snapshot("agent-1").await.expect("Zellij snapshot");
        let canonical = format!(
            "{}\n{}",
            snapshot.scrollback.join("\n"),
            snapshot.visible_grid
        );
        assert_eq!(canonical.matches("stable history").count(), 1);
        assert!(!canonical.contains("first frame"));
        assert!(canonical.contains("second frame"));
    }

    #[test]
    fn launch_spec_rejects_invalid_environment_names_before_writing_a_file() {
        let launch = ZellijLaunchSpec {
            session_id: "agent-1".to_string(),
            executable: "provider".to_string(),
            args: Vec::new(),
            cwd: PathBuf::from("workspace"),
            env: BTreeMap::from([("BAD=KEY".to_string(), "value".to_string())]),
        };

        assert_eq!(
            validate_launch_spec(&launch),
            Err("Agent terminal launch specification is invalid".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn wardian_home_namespaces_zellij_sessions_deterministically() {
        let first = session_name_for_home(Path::new("C:\\Wardian\\one"));
        assert_eq!(first, session_name_for_home(Path::new("c:/wardian/one")));
        assert_ne!(first, session_name_for_home(Path::new("C:\\Wardian\\two")));
        assert!(first.starts_with("wardian-"));
        assert_eq!(first.len(), "wardian-".len() + 12);
    }

    #[cfg(windows)]
    #[test]
    fn failed_windows_pid_publication_kills_client_before_retry_is_adopted() {
        struct ProcessGuard(Vec<u32>);

        impl Drop for ProcessGuard {
            fn drop(&mut self) {
                for pid in self.0.drain(..) {
                    let _ = crate::utils::process::force_kill_process_tree(pid);
                }
            }
        }

        let root = tempfile::tempdir().unwrap();
        let arguments = vec!["-t".to_string(), "127.0.0.1".to_string()];
        let failed_pid_path = root.path().join("missing").join("attached-client.pid");
        let mut failed_command = windows_attached_client_launcher_command(
            Path::new("ping.exe"),
            &arguments,
            &failed_pid_path,
        );
        let failed_output = failed_command
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run failed-publication launcher");
        let failed_pid = parse_windows_attached_client_pid(&failed_output.stdout)
            .expect("failed launcher must report the started client");
        let mut guard = ProcessGuard(vec![failed_pid]);
        assert!(!failed_output.status.success());
        assert!(!failed_pid_path.exists());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while crate::utils::process::process_exists(failed_pid)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            !crate::utils::process::process_exists(failed_pid),
            "failed PID publication must not orphan its attached client"
        );

        let adopted_pid_path = root.path().join("attached-client.pid");
        let mut retry_command = windows_attached_client_launcher_command(
            Path::new("ping.exe"),
            &arguments,
            &adopted_pid_path,
        );
        let retry_output = retry_command
            .stdin(std::process::Stdio::null())
            .output()
            .expect("run successful retry launcher");
        let adopted_pid = parse_windows_attached_client_pid(&retry_output.stdout)
            .expect("successful launcher must report the started client");
        guard.0.push(adopted_pid);

        assert!(retry_output.status.success());
        assert_ne!(adopted_pid, failed_pid);
        assert_eq!(
            std::fs::read_to_string(&adopted_pid_path)
                .unwrap()
                .trim()
                .parse::<u32>()
                .unwrap(),
            adopted_pid
        );
        assert!(crate::utils::process::process_exists(adopted_pid));
        assert!(!crate::utils::process::process_exists(failed_pid));
    }

    #[cfg(unix)]
    #[test]
    fn posix_home_namespaces_preserve_case_and_literal_backslashes() {
        let upper = session_name_for_home(Path::new("/tmp/Wardian/A"));
        let lower = session_name_for_home(Path::new("/tmp/Wardian/a"));
        let slash = session_name_for_home(Path::new("/tmp/Wardian/path"));
        let backslash = session_name_for_home(Path::new("/tmp/Wardian\\path"));

        assert_ne!(upper, lower);
        assert_ne!(slash, backslash);
        assert_eq!(upper, session_name_for_home(Path::new("/tmp/Wardian/A")));
    }

    #[tokio::test]
    async fn create_pane_uses_one_use_host_without_putting_environment_on_command_line() {
        let root = tempfile::tempdir().unwrap();
        let runner = FakeRunner::succeeding("terminal_7\n");
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner.clone());
        engine.prepare_runtime_directories().unwrap();
        let binding = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: vec!["--flag".to_string()],
                cwd: root.path().to_path_buf(),
                env: BTreeMap::from([("SECRET_TOKEN".to_string(), "never-on-cli".to_string())]),
            })
            .await
            .unwrap();

        assert_eq!(binding.pane_id.unwrap().as_str(), "terminal_7");
        assert_eq!(binding.phase, ZellijPanePhase::Running);
        let calls = runner.calls.lock().unwrap();
        let command = calls
            .iter()
            .find(|call| call.iter().any(|arg| arg == "new-pane"))
            .expect("new pane command")
            .join(" ");
        assert!(command.contains("terminal-host --manifest"));
        assert!(!command.contains("SECRET_TOKEN"));
        assert!(!command.contains("never-on-cli"));
        let manifest_path = std::fs::read_dir(engine.config.launches_dir())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let manifest: TerminalLaunchManifest =
            serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
        assert_eq!(manifest.env["SECRET_TOKEN"], "never-on-cli");
    }

    #[tokio::test]
    async fn missing_created_pane_identity_closes_the_unregistered_candidate() {
        let root = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let candidate = br#"[{"id":9,"is_plugin":false,"is_fullscreen":false,"title":"provider title","exited":false,"exit_status":null,"pane_command":"wardian-cli terminal-host --nonce WARDIAN_MANAGED_NONCE","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}]"#;
        let runner = FakeRunner::with_outputs([
            successful(b"[]"),
            successful(b"[]"),
            successful(b"pane created\n"),
            successful(b"[]"),
            successful(candidate),
            successful(b""),
            successful(b"[]"),
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner.clone());
        engine.prepare_runtime_directories().unwrap();

        let result = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: root.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await;

        assert_eq!(
            result.unwrap_err(),
            "Zellij did not report the created pane identity"
        );
        assert!(engine.binding("agent-1").await.is_none());
        assert!(runner.calls.lock().unwrap().iter().any(|call| {
            call.windows(2)
                .any(|args| args == ["--pane-id", "terminal_9"])
        }));
    }

    #[tokio::test]
    async fn failed_identity_cleanup_remains_tracked_until_a_retry_closes_the_pane() {
        let root = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let failed = || Output {
            status: status(1),
            stdout: Vec::new(),
            stderr: b"close failed".to_vec(),
        };
        let candidate = br#"[{"id":9,"is_plugin":false,"is_fullscreen":false,"title":"provider title","exited":false,"exit_status":null,"pane_command":"wardian-cli terminal-host --nonce WARDIAN_MANAGED_NONCE","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}]"#;
        let runner = FakeRunner::with_outputs([
            successful(b"[]"),
            successful(b"[]"),
            successful(b"pane created\n"),
            successful(b"[]"),
            successful(candidate),
            failed(),
            successful(candidate),
            successful(b""),
            successful(candidate),
            successful(candidate),
            successful(b""),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_10\n"),
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner.clone());
        engine.prepare_runtime_directories().unwrap();
        let launch = ZellijLaunchSpec {
            session_id: "agent-1".to_string(),
            executable: "provider".to_string(),
            args: Vec::new(),
            cwd: root.path().to_path_buf(),
            env: BTreeMap::new(),
        };

        let error = engine
            .create_pane(launch.clone())
            .await
            .expect_err("failed close must keep a cleanup record");
        assert!(error.contains("cleanup is pending"));
        let pending = engine
            .binding("agent-1")
            .await
            .expect("pending cleanup binding");
        assert_eq!(pending.phase, ZellijPanePhase::Closing);
        assert!(pending.pane_id.is_none());

        let retry_error = engine
            .create_pane(launch.clone())
            .await
            .expect_err("successful close without confirmed removal must remain pending");
        assert!(retry_error.contains("cleanup could not be confirmed"));
        assert_eq!(
            engine
                .binding("agent-1")
                .await
                .expect("unconfirmed cleanup binding")
                .phase,
            ZellijPanePhase::Closing,
        );

        let replacement = engine
            .create_pane(launch)
            .await
            .expect("confirmed retry closes the orphan before replacement");
        assert_eq!(replacement.pane_id.unwrap().as_str(), "terminal_10");
        assert_eq!(replacement.phase, ZellijPanePhase::Running);
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call
                    .windows(2)
                    .any(|args| args == ["--pane-id", "terminal_9"]))
                .count(),
            3,
        );
    }

    #[tokio::test]
    async fn subscription_spawn_failure_closes_the_pane_and_removes_its_binding() {
        let root = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let runner = FakeRunner::with_outputs([
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_7\n"),
            successful(b""),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_8\n"),
        ]);
        let engine = Arc::new(ZellijTerminalEngine::with_runner(
            config(root.path()),
            runner.clone(),
        ));
        engine.prepare_runtime_directories().unwrap();
        let binding = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: root.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .unwrap();

        let error = engine
            .open_pane_transport(&binding)
            .await
            .err()
            .expect("missing subscription executable must fail");

        assert!(error.contains("subscription could not start"));
        assert!(engine.binding("agent-1").await.is_none());
        assert!(runner.calls.lock().unwrap().iter().any(|call| {
            call.windows(2)
                .any(|args| args == ["--pane-id", "terminal_7"])
        }));
        let replacement = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: root.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .expect("confirmed transport rollback must allow replacement");
        assert_eq!(replacement.pane_id.unwrap().as_str(), "terminal_8");
    }

    #[tokio::test]
    async fn stale_generation_cannot_write_to_replacement_pane() {
        let root = tempfile::tempdir().unwrap();
        let runner = FakeRunner::succeeding("terminal_3\n");
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner);
        engine.prepare_runtime_directories().unwrap();
        let binding = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: root.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .unwrap();
        assert_eq!(
            engine
                .write_to_pane("agent-1", binding.generation + 1, b"unsafe")
                .await,
            Err("Agent Zellij pane generation is stale".to_string())
        );
    }

    #[tokio::test]
    async fn newer_activation_request_supersedes_an_unsettled_request() {
        let root = tempfile::tempdir().unwrap();
        let runner = FakeRunner::succeeding("terminal_3\n");
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner.clone());

        engine.register_activation_request("handoff-a");
        engine.register_activation_request("handoff-b");
        assert_eq!(
            engine
                .activate_pane_for_request("agent-1", 1, "handoff-a")
                .await,
            Err("Terminal handoff was superseded".to_string())
        );
        assert!(
            runner.calls.lock().unwrap().is_empty(),
            "a superseded request must issue no Zellij focus action"
        );
    }

    #[tokio::test]
    async fn cancellation_during_pane_discovery_prevents_a_late_focus_action() {
        let root = tempfile::tempdir().unwrap();
        let runner = BlockingActivationRunner::new();
        let engine = Arc::new(ZellijTerminalEngine::with_runner(
            config(root.path()),
            runner.clone(),
        ));
        {
            let mut panes = engine.pane_registry();
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
        }
        engine.register_activation_request("handoff-a");
        let activation = {
            let engine = Arc::clone(&engine);
            tokio::spawn(async move {
                engine
                    .activate_pane_for_request("agent-1", 1, "handoff-a")
                    .await
            })
        };
        while !runner.list_started.load(AtomicOrdering::Acquire) {
            tokio::task::yield_now().await;
        }
        assert!(engine.cancel_activation_request("handoff-a"));
        runner.release();

        assert_eq!(
            activation.await.unwrap(),
            Err("Terminal handoff was superseded".to_string())
        );
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].iter().any(|arg| arg == "list-panes"));
        assert!(!calls.iter().flatten().any(|arg| arg == "focus-pane-id"));
    }

    #[tokio::test]
    async fn downstream_subscription_close_does_not_mark_a_live_pane_exited() {
        let root = tempfile::tempdir().unwrap();
        let engine =
            ZellijTerminalEngine::with_runner(config(root.path()), FakeRunner::with_outputs([]));
        engine.pane_registry().bindings.insert(
            "agent-1".to_string(),
            ZellijPaneBinding {
                session_id: "agent-1".to_string(),
                pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                generation: 1,
                phase: ZellijPanePhase::Running,
            },
        );

        engine.finish_pane_subscription("agent-1", 1, true);
        assert_eq!(
            engine.binding("agent-1").await.unwrap().phase,
            ZellijPanePhase::Running,
        );

        engine.finish_pane_subscription("agent-1", 1, false);
        assert_eq!(
            engine.binding("agent-1").await.unwrap().phase,
            ZellijPanePhase::Exited,
        );
    }

    #[tokio::test]
    async fn replacement_pending_rejects_focus_before_any_zellij_action() {
        let root = tempfile::tempdir().unwrap();
        let runner = FakeRunner::with_outputs([]);
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner.clone());
        {
            let mut panes = engine.pane_registry();
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
            panes
                .replacement_reservations
                .insert("agent-1".to_string(), 1);
        }
        engine.register_activation_request("handoff-a");

        assert_eq!(
            engine
                .activate_pane_for_request("agent-1", 1, "handoff-a")
                .await,
            Err("Agent terminal restart is still settling".to_string())
        );
        assert!(runner.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn exited_subscription_can_be_closed_before_same_session_restart() {
        let root = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let runner = FakeRunner::with_outputs([
            successful(b"[]"),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_2\n"),
        ]);
        let engine = Arc::new(ZellijTerminalEngine::with_runner(
            config(root.path()),
            runner,
        ));
        engine.prepare_runtime_directories().unwrap();
        {
            let mut panes = engine.pane_registry();
            panes.next_generation = 1;
            panes.bindings.insert(
                "agent-1".to_string(),
                ZellijPaneBinding {
                    session_id: "agent-1".to_string(),
                    pane_id: Some(ZellijPaneId::parse("terminal_1").unwrap()),
                    generation: 1,
                    phase: ZellijPanePhase::Running,
                },
            );
        }
        engine.finish_pane_subscription("agent-1", 1, false);
        ZellijPaneLease::new(engine.clone(), "agent-1".to_string(), 1)
            .close()
            .await
            .unwrap();

        let replacement = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: root.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .unwrap();

        assert_eq!(replacement.generation, 2);
        assert_eq!(replacement.phase, ZellijPanePhase::Running);
        assert_eq!(replacement.pane_id.unwrap().as_str(), "terminal_2");
    }

    #[tokio::test]
    async fn dropped_lease_keeps_cleanup_tracked_until_pane_absence_is_confirmed() {
        let root = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let candidate = br#"[{"id":3,"is_plugin":false,"is_fullscreen":false,"title":"wardian:agent-1","exited":false,"exit_status":null,"pane_command":"provider","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}]"#;
        let runner = FakeRunner::with_outputs([
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_3\n"),
            successful(b""),
            successful(candidate),
            successful(b""),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_4\n"),
            successful(b"[]"),
        ]);
        let engine = Arc::new(ZellijTerminalEngine::with_runner(
            config(root.path()),
            runner.clone(),
        ));
        engine.prepare_runtime_directories().unwrap();
        let first = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: root.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .unwrap();
        drop(ZellijPaneLease::new(
            engine.clone(),
            first.session_id.clone(),
            first.generation,
        ));
        let cleanup_deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
        loop {
            if engine.binding("agent-1").await.is_some_and(|binding| {
                binding.phase == ZellijPanePhase::Closing
                    && runner.calls.lock().unwrap().iter().any(|call| {
                        call.windows(2)
                            .any(|args| args == ["--pane-id", "terminal_3"])
                    })
            }) {
                break;
            }
            assert!(
                std::time::Instant::now() < cleanup_deadline,
                "lease drop did not start tracked cleanup"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        let replacement = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "agent-1".to_string(),
                executable: "provider".to_string(),
                args: Vec::new(),
                cwd: root.path().to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .unwrap();
        assert_eq!(replacement.pane_id.unwrap().as_str(), "terminal_4");
        assert!(replacement.generation > first.generation);
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap()
                .iter()
                .filter(|call| call
                    .windows(2)
                    .any(|args| args == ["--pane-id", "terminal_3"]))
                .count(),
            2,
            "replacement must retry the unconfirmed lease-drop cleanup"
        );
    }

    #[tokio::test]
    async fn startup_reconciliation_closes_unregistered_managed_panes() {
        let root = tempfile::tempdir().unwrap();
        let nonce = "0123456789abcdef0123456789abcdef";
        let runner = FakeRunner::with_outputs([
            Output {
                status: status(0),
                stdout: format!(r#"[{{"id":9,"is_plugin":false,"is_fullscreen":false,"title":"provider-mutated-title","exited":false,"exit_status":null,"pane_command":"wardian-cli terminal-host --nonce {nonce}","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}}]"#).into_bytes(),
                stderr: Vec::new(),
            },
            Output {
                status: status(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
            Output {
                status: status(0),
                stdout: b"[]".to_vec(),
                stderr: Vec::new(),
            },
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner.clone());
        engine.prepare_runtime_directories().unwrap();
        write_managed_pane_marker(
            &engine.config,
            &ManagedPaneMarker {
                schema: MANAGED_PANE_MARKER_SCHEMA,
                nonce: nonce.to_string(),
                session_id: "stale-agent".to_string(),
            },
        )
        .unwrap();

        engine.close_unregistered_managed_panes().await.unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls[1]
            .windows(2)
            .any(|args| args == ["--pane-id", "terminal_9"]));
        assert!(!engine.config.managed_pane_marker_path(nonce).exists());
    }

    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bundled_zellij_keeps_a_real_conpty_pane_alive_and_routes_targeted_input() {
        if std::env::var("WARDIAN_E2E_ZELLIJ").as_deref() != Ok("1") {
            return;
        }

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace = manifest_dir.parent().unwrap();
        let zellij = manifest_dir
            .join("resources")
            .join("bin")
            .join("zellij.exe");
        let wardian_cli = workspace
            .join("target")
            .join("debug")
            .join("wardian-cli.exe");
        assert!(zellij.is_file(), "stage the pinned Zellij binary first");
        assert!(
            wardian_cli.is_file(),
            "build wardian-cli before native proof"
        );

        struct TestSessionGuard {
            zellij: PathBuf,
            session_name: String,
        }
        impl Drop for TestSessionGuard {
            fn drop(&mut self) {
                let _ = Command::new(&self.zellij)
                    .args(["kill-session", &self.session_name])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
        }

        let isolated = tempfile::tempdir().unwrap();
        let runtime_root = isolated.path().join("runtime").join("zellij");
        let session_name = session_name_for_home(isolated.path());
        let _session_guard = TestSessionGuard {
            zellij: zellij.clone(),
            session_name: session_name.clone(),
        };
        let config = ZellijTerminalConfig {
            executable: zellij.clone(),
            wardian_cli,
            runtime_root: runtime_root.clone(),
            wardian_home: isolated.path().to_path_buf(),
            session_name: session_name.clone(),
        };
        let engine = Arc::new(ZellijTerminalEngine::new(config.clone()));
        let broker = Arc::new(crate::state::terminal_session::TerminalSessionBroker::default());
        engine.start_attached_client().await.unwrap();

        let start_deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            let last_start_error = match engine.list_panes().await {
                Ok(panes) if !panes.is_empty() => break,
                Ok(_) => "session returned no panes".to_string(),
                Err(error) => error,
            };
            assert!(
                std::time::Instant::now() < start_deadline,
                "Zellij session did not start: {}; phase={:?}; exit={:?}",
                last_start_error,
                engine.phase().await,
                engine.attached_exit_status().await,
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let binding = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "native-agent".to_string(),
                executable: "powershell.exe".to_string(),
                args: vec![
                    "-NoLogo".to_string(),
                    "-NoProfile".to_string(),
                    "-NoExit".to_string(),
                ],
                cwd: workspace.to_path_buf(),
                env: BTreeMap::from([(
                    "WARDIAN_ZELLIJ_NATIVE_PROBE".to_string(),
                    "pane-isolated".to_string(),
                )]),
            })
            .await
            .unwrap();
        let terminal_panes = engine
            .list_panes()
            .await
            .unwrap()
            .into_iter()
            .filter(|pane| pane.title == "wardian:native-agent")
            .filter_map(|pane| pane.pane_id())
            .collect::<Vec<_>>();
        assert_eq!(terminal_panes, vec![binding.pane_id.clone().unwrap()]);
        let transport = engine.open_pane_transport(&binding).await.unwrap();
        let runtime = transport.runtime();
        let ZellijPaneActiveTransport {
            mut reader,
            snapshot_frames,
            mut subscription,
            lease: _lease,
        } = transport.into_active();
        let runtime_generation = broker
            .start_or_replace_runtime(
                "native-agent",
                runtime,
                wardian_core::models::TerminalGeometry {
                    cols: 100,
                    rows: 30,
                },
            )
            .await
            .unwrap();
        let reader_broker = broker.clone();
        let reader_thread = std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0
                    || crate::state::terminal_session::forward_terminal_output(
                        &reader_broker,
                        "native-agent",
                        runtime_generation,
                        &buffer[..read],
                    )
                    .is_err()
                {
                    break;
                }
            }
        });
        let input_receipt = isolated.path().join("provider-input.txt");
        let input_command = format!(
            "Set-Content -LiteralPath {} -Value targeted-input; Write-Output $env:WARDIAN_ZELLIJ_NATIVE_PROBE\r",
            powershell_single_quoted(&input_receipt.to_string_lossy()),
        );
        broker
            .send_privileged_input("native-agent", input_command.into_bytes())
            .await
            .unwrap();

        let output_deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut latest_frame = Vec::new();
        loop {
            let screen = broker
                .snapshot("native-agent")
                .await
                .map(|snapshot| snapshot.visible_grid)
                .unwrap_or_default();
            if let Some(frame) = snapshot_frames.try_iter().last() {
                latest_frame = frame;
            }
            if std::fs::read_to_string(&input_receipt).ok().as_deref() == Some("targeted-input\r\n")
                && screen.contains("pane-isolated")
                && String::from_utf8_lossy(&latest_frame).contains("pane-isolated")
            {
                break;
            }
            assert!(
                std::time::Instant::now() < output_deadline,
                "pane-addressed input did not reach the broker; screen={screen:?}; panes={:?}",
                engine.list_panes().await,
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        engine
            .close_pane("native-agent", binding.generation)
            .await
            .unwrap();
        let _ = subscription.kill();
        let _ = subscription.wait();
        let _ = reader_thread.join();
        drop(_lease);

        let replacement = engine
            .create_pane(ZellijLaunchSpec {
                session_id: "native-agent".to_string(),
                executable: "powershell.exe".to_string(),
                args: vec![
                    "-NoLogo".to_string(),
                    "-NoProfile".to_string(),
                    "-NoExit".to_string(),
                ],
                cwd: workspace.to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .expect("same-session replacement pane");
        assert!(replacement.generation > binding.generation);
        assert_eq!(
            engine
                .list_panes()
                .await
                .unwrap()
                .into_iter()
                .filter(|pane| pane.title == "wardian:native-agent")
                .count(),
            1,
            "same-session restart must leave one provider pane"
        );
        engine
            .close_pane("native-agent", replacement.generation)
            .await
            .unwrap();

        engine
            .create_pane(ZellijLaunchSpec {
                session_id: "stale-after-backend-restart".to_string(),
                executable: "powershell.exe".to_string(),
                args: vec![
                    "-NoLogo".to_string(),
                    "-NoProfile".to_string(),
                    "-NoExit".to_string(),
                ],
                cwd: workspace.to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .expect("stale pane fixture");
        drop(engine);

        let recovered = Arc::new(ZellijTerminalEngine::new(config));
        let recovered_broker =
            Arc::new(crate::state::terminal_session::TerminalSessionBroker::default());
        recovered
            .start_attached_client()
            .await
            .expect("reattach existing Zellij session");
        assert!(
            recovered
                .list_panes()
                .await
                .unwrap()
                .into_iter()
                .all(|pane| pane.title != "wardian:stale-after-backend-restart"),
            "backend restart must close unregistered provider panes"
        );

        let recovered_binding = recovered
            .create_pane(ZellijLaunchSpec {
                session_id: "recovered-agent".to_string(),
                executable: "powershell.exe".to_string(),
                args: vec![
                    "-NoLogo".to_string(),
                    "-NoProfile".to_string(),
                    "-NoExit".to_string(),
                ],
                cwd: workspace.to_path_buf(),
                env: BTreeMap::new(),
            })
            .await
            .expect("provider pane after backend recovery");
        let transport = recovered
            .open_pane_transport(&recovered_binding)
            .await
            .unwrap();
        let runtime = transport.runtime();
        let ZellijPaneActiveTransport {
            reader: _reader,
            snapshot_frames: _snapshot_frames,
            mut subscription,
            lease,
        } = transport.into_active();
        recovered_broker
            .start_or_replace_runtime(
                "recovered-agent",
                runtime,
                wardian_core::models::TerminalGeometry {
                    cols: 100,
                    rows: 30,
                },
            )
            .await
            .expect("register recovered provider runtime in a fresh broker");
        if let Err(error) = recovered
            .activate_pane("recovered-agent", recovered_binding.generation)
            .await
        {
            panic!(
                "focus recovered provider pane: {error}; binding={:?}; subscription={:?}; panes={:?}",
                recovered.binding("recovered-agent").await,
                subscription.try_wait(),
                recovered.list_panes().await,
            );
        }
        let recovered_input = isolated.path().join("recovered-input.txt");
        recovered_broker
            .send_privileged_input(
                "recovered-agent",
                format!(
                    "Set-Content -LiteralPath {} -Value recovered\r",
                    powershell_single_quoted(&recovered_input.to_string_lossy()),
                )
                .into_bytes(),
            )
            .await
            .expect("route input through the fresh broker");
        let recovered_deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::fs::read_to_string(&recovered_input).ok().as_deref() != Some("recovered\r\n") {
            assert!(
                std::time::Instant::now() < recovered_deadline,
                "fresh broker input did not reach the recovered pane"
            );
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        recovered
            .close_pane("recovered-agent", recovered_binding.generation)
            .await
            .unwrap();
        let _ = subscription.kill();
        let _ = subscription.wait();
        drop(lease);
    }
}
