use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use wardian_core::models::{TerminalLaunchManifest, TERMINAL_LAUNCH_MANIFEST_SCHEMA};

pub const ZELLIJ_VERSION: &str = "0.45.0";

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
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

impl Drop for ZellijPaneLease {
    fn drop(&mut self) {
        self.engine
            .close_pane_best_effort(&self.session_id, self.generation);
    }
}

pub struct ZellijPaneTransport {
    pub reader: Box<dyn std::io::Read + Send>,
    pub snapshot_frames: std::sync::mpsc::Receiver<Vec<u8>>,
    pub runtime: crate::state::terminal_session::TerminalRuntimeHandles,
    pub subscription: std::process::Child,
    pub lease: ZellijPaneLease,
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
}

struct ZellijEngineState {
    phase: ZellijEnginePhase,
    attached: Option<ZellijAttachedClient>,
}

#[derive(Default)]
struct ZellijPaneRegistry {
    next_generation: u64,
    bindings: HashMap<String, ZellijPaneBinding>,
}

struct ZellijAttachedClient {
    child: ZellijAttachedProcess,
    runtime_generation: u64,
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
        }
    }
}

pub struct ZellijTerminalEngine {
    config: ZellijTerminalConfig,
    runner: Arc<dyn ZellijCommandRunner>,
    state: Mutex<ZellijEngineState>,
    panes: std::sync::Mutex<ZellijPaneRegistry>,
    start_lock: Mutex<()>,
    activation_lock: Mutex<()>,
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

    fn pane_registry(&self) -> std::sync::MutexGuard<'_, ZellijPaneRegistry> {
        self.panes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub async fn attached_runtime_generation(&self) -> Option<u64> {
        self.state
            .lock()
            .await
            .attached
            .as_ref()
            .map(|client| client.runtime_generation)
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
                    return Ok(attached.runtime_generation);
                }
                state.attached = None;
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
                state.attached = Some(ZellijAttachedClient {
                    child: ZellijAttachedProcess::NativeConsole(pid),
                    runtime_generation: 1,
                });
                state.phase = ZellijEnginePhase::Running;
                return Ok(1);
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
            for attempt in 0..2 {
                let pid = match self.spawn_windows_attached_client(&pid_path) {
                    Ok(pid) => pid,
                    Err(error) => {
                        last_start_error = error;
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
                        if attempt == 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
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
            state.attached = Some(ZellijAttachedClient {
                child: ZellijAttachedProcess::NativeConsole(pid),
                runtime_generation: 1,
            });
            state.phase = ZellijEnginePhase::Running;
            Ok(1)
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

            let weak_engine = Arc::downgrade(self);
            let runtime_generation = 1;
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
                        if state
                            .attached
                            .as_ref()
                            .is_some_and(|client| client.runtime_generation == runtime_generation)
                        {
                            state.attached = None;
                            state.phase = ZellijEnginePhase::Reattaching;
                        }
                    });
                }
            });

            let mut state = self.state.lock().await;
            state.attached = Some(ZellijAttachedClient {
                child: ZellijAttachedProcess::Portable(child),
                runtime_generation,
                _master: master,
            });
            state.phase = ZellijEnginePhase::Running;
            Ok(runtime_generation)
        }
    }

    pub fn prepare_runtime_directories(&self) -> Result<(), String> {
        std::fs::create_dir_all(self.config.config_dir()).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(self.config.launches_dir()).map_err(|error| error.to_string())?;
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

    #[cfg(windows)]
    fn spawn_windows_attached_client(&self, pid_path: &Path) -> Result<u32, String> {
        use base64::Engine as _;

        let _ = std::fs::remove_file(pid_path);
        let script = format!(
            "$client = Start-Process -FilePath {} -ArgumentList @('attach', '--create', {}) -WindowStyle Hidden -PassThru; Set-Content -LiteralPath {} -Value $client.Id -Encoding ascii -NoNewline",
            powershell_single_quoted(&self.config.executable.to_string_lossy()),
            powershell_single_quoted(&self.config.session_name),
            powershell_single_quoted(&pid_path.to_string_lossy()),
        );
        let encoded_bytes = script
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let encoded = base64::engine::general_purpose::STANDARD.encode(encoded_bytes);
        let status = zellij_helper_command("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &encoded,
            ])
            .env("ZELLIJ_CONFIG_DIR", self.config.config_dir())
            .env("WARDIAN_HOME", &self.config.wardian_home)
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|error| format!("Zellij native attached client could not start: {error}"))?;
        if !status.success() {
            return Err("Zellij native attached client could not start".to_string());
        }
        std::fs::read_to_string(pid_path)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .ok_or_else(|| "Zellij native attached client did not report its process".to_string())
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

    async fn close_unregistered_managed_panes(&self) -> Result<(), String> {
        let registered = self
            .pane_registry()
            .bindings
            .values()
            .filter_map(|binding| binding.pane_id.clone())
            .collect::<std::collections::HashSet<_>>();
        let stale = self
            .list_panes()
            .await?
            .into_iter()
            .filter(|pane| pane.title.starts_with("wardian:"))
            .filter_map(|pane| pane.pane_id())
            .filter(|pane_id| !registered.contains(pane_id))
            .collect::<Vec<_>>();
        for pane_id in stale {
            self.close_pane_id(&pane_id).await?;
        }
        Ok(())
    }

    async fn close_unregistered_session_panes(&self, session_id: &str) -> Result<(), String> {
        let expected_title = format!("wardian:{session_id}");
        let stale = self
            .list_panes()
            .await?
            .into_iter()
            .filter(|pane| pane.title == expected_title)
            .filter_map(|pane| pane.pane_id())
            .collect::<Vec<_>>();
        for pane_id in stale {
            self.close_pane_id(&pane_id).await?;
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

    pub async fn create_pane(&self, launch: ZellijLaunchSpec) -> Result<ZellijPaneBinding, String> {
        validate_launch_spec(&launch)?;
        let generation = {
            let mut panes = self.pane_registry();
            if panes.bindings.contains_key(&launch.session_id) {
                return Err("Agent already has a Zellij pane transition in progress".to_string());
            }
            panes.next_generation = panes.next_generation.saturating_add(1);
            let generation = panes.next_generation;
            panes.bindings.insert(
                launch.session_id.clone(),
                ZellijPaneBinding {
                    session_id: launch.session_id.clone(),
                    pane_id: None,
                    generation,
                    phase: ZellijPanePhase::Starting,
                },
            );
            generation
        };

        if let Err(error) = self
            .close_unregistered_session_panes(&launch.session_id)
            .await
        {
            self.rollback_start(&launch.session_id, generation).await;
            return Err(error);
        }
        let known_panes = self
            .list_panes()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|pane| pane.pane_id())
            .collect::<std::collections::HashSet<_>>();

        let nonce = Uuid::new_v4().simple().to_string();
        let (launch_path, pane_command) = match prepare_pane_launch(&self.config, &launch, &nonce) {
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
                let expected_title = format!("wardian:{}", launch.session_id);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    if let Ok(panes) = self.list_panes().await {
                        if let Some(pane_id) = panes.into_iter().find_map(|pane| {
                            let pane_id = pane.pane_id()?;
                            (pane.title == expected_title && !known_panes.contains(&pane_id))
                                .then_some(pane_id)
                        }) {
                            break pane_id;
                        }
                    }
                    if std::time::Instant::now() >= deadline {
                        self.rollback_start(&launch.session_id, generation).await;
                        return Err("Zellij did not report the created pane identity".to_string());
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
            }
        };
        let binding = {
            let mut panes = self.pane_registry();
            let Some(binding) = panes.bindings.get_mut(&launch.session_id) else {
                return Err("Zellij pane start lost its agent binding".to_string());
            };
            if binding.generation != generation || binding.phase != ZellijPanePhase::Starting {
                return Err("Zellij pane start was superseded".to_string());
            }
            binding.pane_id = Some(pane_id);
            binding.phase = ZellijPanePhase::Running;
            binding.clone()
        };
        Ok(binding)
    }

    pub fn open_pane_transport(
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
        let mut subscription = command
            .spawn()
            .map_err(|error| format!("Zellij pane subscription could not start: {error}"))?;
        let stdout = subscription
            .stdout
            .take()
            .ok_or_else(|| "Zellij pane subscription has no output stream".to_string())?;
        let (render_tx, render_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
        let (snapshot_tx, snapshot_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(8);
        let expected_pane = pane_id.as_str().to_string();
        let subscription_engine = Arc::downgrade(self);
        let subscription_session_id = binding.session_id.clone();
        let subscription_generation = binding.generation;
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
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
                if snapshot_tx.send(frame.clone()).is_err() || render_tx.send(frame).is_err() {
                    break;
                }
            }
            if let Some(engine) = subscription_engine.upgrade() {
                let mut panes = engine.pane_registry();
                if let Some(binding) = panes.bindings.get_mut(&subscription_session_id) {
                    if binding.generation == subscription_generation
                        && binding.phase == ZellijPanePhase::Running
                    {
                        binding.phase = ZellijPanePhase::Exited;
                    }
                }
            }
        });

        let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<
            crate::state::terminal_session::NativeTerminalWriteRequest,
        >(256);
        let input_engine = self.clone();
        let input_session_id = binding.session_id.clone();
        let input_generation = binding.generation;
        std::thread::spawn(move || {
            while let Some(request) = input_rx.blocking_recv() {
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
            reader: Box::new(ZellijSnapshotReader::new(render_rx)),
            snapshot_frames: snapshot_rx,
            runtime,
            subscription,
            lease: ZellijPaneLease::new(
                self.clone(),
                binding.session_id.clone(),
                binding.generation,
            ),
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

    pub async fn activate_pane(&self, session_id: &str, generation: u64) -> Result<(), String> {
        let _activation_guard = self.activation_lock.lock().await;
        let target = self.live_pane(session_id, generation).await?;
        let panes = self.list_panes().await?;
        if let Some(fullscreen) = panes.iter().find(|pane| {
            !pane.is_plugin
                && pane.pane_id().is_some_and(|pane_id| pane_id != target)
                && pane.is_fullscreen
        }) {
            let fullscreen_id = fullscreen.pane_id().expect("terminal pane checked above");
            self.run_status_action(vec![
                "action".to_string(),
                "toggle-no-ui-fullscreen".to_string(),
                "--pane-id".to_string(),
                fullscreen_id.as_str().to_string(),
            ])
            .await?;
        }
        self.focus_pane(session_id, generation).await?;
        let target_is_fullscreen = panes.iter().any(|pane| {
            pane.pane_id().is_some_and(|pane_id| pane_id == target) && pane.is_fullscreen
        });
        if !target_is_fullscreen {
            self.run_status_action(vec![
                "action".to_string(),
                "toggle-no-ui-fullscreen".to_string(),
                "--pane-id".to_string(),
                target.as_str().to_string(),
            ])
            .await?;
        }
        Ok(())
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
            let binding = panes
                .bindings
                .get_mut(session_id)
                .ok_or_else(|| "Agent has no Zellij pane".to_string())?;
            if binding.generation != generation
                || !matches!(
                    binding.phase,
                    ZellijPanePhase::Running | ZellijPanePhase::Exited
                )
            {
                return Err("Agent Zellij pane generation is stale".to_string());
            }
            binding.phase = ZellijPanePhase::Closing;
            binding
                .pane_id
                .clone()
                .ok_or_else(|| "Agent Zellij pane is not ready".to_string())?
        };
        let result = self.close_pane_id(&pane).await;
        let mut panes = self.pane_registry();
        if panes
            .bindings
            .get(session_id)
            .is_some_and(|binding| binding.generation == generation)
        {
            panes.bindings.remove(session_id);
        }
        result
    }

    async fn live_pane(&self, session_id: &str, generation: u64) -> Result<ZellijPaneId, String> {
        let panes = self.pane_registry();
        let binding = panes
            .bindings
            .get(session_id)
            .ok_or_else(|| "Agent has no Zellij pane".to_string())?;
        if binding.generation != generation || binding.phase != ZellijPanePhase::Running {
            return Err("Agent Zellij pane generation is stale".to_string());
        }
        binding
            .pane_id
            .clone()
            .ok_or_else(|| "Agent Zellij pane is not ready".to_string())
    }

    async fn rollback_start(&self, session_id: &str, generation: u64) {
        let mut panes = self.pane_registry();
        if panes
            .bindings
            .get(session_id)
            .is_some_and(|binding| binding.generation == generation)
        {
            panes.bindings.remove(session_id);
        }
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

    fn close_pane_best_effort(&self, session_id: &str, generation: u64) {
        let binding = {
            let mut panes = self.pane_registry();
            if panes
                .bindings
                .get(session_id)
                .is_some_and(|binding| binding.generation == generation)
            {
                panes.bindings.remove(session_id)
            } else {
                None
            }
        };
        let Some(binding) = binding else {
            return;
        };
        let Some(pane_id) = binding.pane_id else {
            return;
        };
        let _ = zellij_helper_command(&self.config.executable)
            .args([
                "--session",
                &self.config.session_name,
                "action",
                "close-pane",
                "--pane-id",
                pane_id.as_str(),
            ])
            .env("ZELLIJ_CONFIG_DIR", self.config.config_dir())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
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

fn prepare_pane_launch(
    config: &ZellijTerminalConfig,
    launch: &ZellijLaunchSpec,
    nonce: &str,
) -> Result<(PathBuf, Vec<String>), String> {
    let manifest_path = config.launches_dir().join(format!("{nonce}.json"));
    let manifest = TerminalLaunchManifest {
        schema: TERMINAL_LAUNCH_MANIFEST_SCHEMA,
        nonce: nonce.to_string(),
        session_id: launch.session_id.clone(),
        executable: launch.executable.clone(),
        args: launch.args.clone(),
        cwd: launch.cwd.to_string_lossy().to_string(),
        env: launch.env.clone(),
    };
    write_launch_manifest(&manifest_path, &manifest)?;
    Ok((
        manifest_path.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::process::ExitStatus;
    use std::sync::Mutex as StdMutex;

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

    #[derive(Default)]
    struct FakeRunner {
        calls: StdMutex<Vec<Vec<String>>>,
        outputs: StdMutex<VecDeque<Output>>,
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
            })
        }

        fn with_outputs(outputs: impl IntoIterator<Item = Output>) -> Arc<Self> {
            Arc::new(Self {
                calls: StdMutex::new(Vec::new()),
                outputs: StdMutex::new(outputs.into_iter().collect()),
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
            self.outputs
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "missing fake output".to_string())
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
                wardian_core::models::TerminalGeometry {
                    cols: 80,
                    rows: 24,
                },
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
        let canonical = format!("{}\n{}", snapshot.scrollback.join("\n"), snapshot.visible_grid);
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
    async fn dropped_lease_removes_binding_so_the_same_session_can_restart() {
        let root = tempfile::tempdir().unwrap();
        let successful = |stdout: &[u8]| Output {
            status: status(0),
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
        };
        let runner = FakeRunner::with_outputs([
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_3\n"),
            successful(b"[]"),
            successful(b"[]"),
            successful(b"terminal_4\n"),
        ]);
        let engine = Arc::new(ZellijTerminalEngine::with_runner(
            config(root.path()),
            runner,
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
        assert!(engine.binding("agent-1").await.is_none());

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
    }

    #[tokio::test]
    async fn startup_reconciliation_closes_unregistered_managed_panes() {
        let root = tempfile::tempdir().unwrap();
        let runner = FakeRunner::with_outputs([
            Output {
                status: status(0),
                stdout: br#"[{"id":9,"is_plugin":false,"is_fullscreen":false,"title":"wardian:stale-agent","exited":false,"exit_status":null,"pane_command":"provider","pane_cwd":"workspace","pane_rows":40,"pane_columns":120}]"#.to_vec(),
                stderr: Vec::new(),
            },
            Output {
                status: status(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            },
        ]);
        let engine = ZellijTerminalEngine::with_runner(config(root.path()), runner.clone());

        engine.close_unregistered_managed_panes().await.unwrap();

        let calls = runner.calls.lock().unwrap();
        assert!(calls[1]
            .windows(2)
            .any(|args| args == ["--pane-id", "terminal_9"]));
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
        let ZellijPaneTransport {
            mut reader,
            snapshot_frames,
            runtime,
            mut subscription,
            lease: _lease,
        } = engine.open_pane_transport(&binding).unwrap();
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
        let ZellijPaneTransport {
            reader: _reader,
            snapshot_frames: _snapshot_frames,
            runtime,
            mut subscription,
            lease,
        } = recovered.open_pane_transport(&recovered_binding).unwrap();
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
        let recovered_deadline =
            std::time::Instant::now() + std::time::Duration::from_secs(8);
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
