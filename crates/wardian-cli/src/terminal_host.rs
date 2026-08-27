use crate::args::TerminalHostArgs;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use wardian_core::models::TerminalLaunchManifest;

const EXIT_LAUNCH_REJECTED: i32 = 78;
const EXIT_LAUNCH_FAILED: i32 = 70;
static MANIFEST_CLAIM_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn run(args: TerminalHostArgs) -> i32 {
    match consume_manifest(&args.manifest, &args.nonce).and_then(launch_provider) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Wardian could not start this agent terminal: {error}");
            if error.starts_with("provider launch failed") {
                EXIT_LAUNCH_FAILED
            } else {
                EXIT_LAUNCH_REJECTED
            }
        }
    }
}

fn launch_directory() -> Result<PathBuf, String> {
    let home = wardian_core::paths::wardian_home()
        .ok_or_else(|| "Wardian home is unavailable".to_string())?;
    Ok(home.join("runtime").join("zellij").join("launches"))
}

fn canonical_manifest_path(
    path: &Path,
    nonce: &str,
    launch_directory: &Path,
) -> Result<PathBuf, String> {
    if nonce.len() < 32
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("terminal launch nonce is invalid".to_string());
    }
    let launch_root = launch_directory
        .canonicalize()
        .map_err(|_| "terminal launch directory is unavailable".to_string())?;
    let canonical = path
        .canonicalize()
        .map_err(|_| "terminal launch manifest is unavailable".to_string())?;
    if canonical.parent() != Some(launch_root.as_path()) {
        return Err("terminal launch manifest is outside the launch directory".to_string());
    }
    let expected_name = format!("{nonce}.json");
    if canonical.file_name().and_then(|name| name.to_str()) != Some(expected_name.as_str()) {
        return Err("terminal launch manifest name does not match its nonce".to_string());
    }
    Ok(canonical)
}

fn consume_manifest(path: &Path, nonce: &str) -> Result<TerminalLaunchManifest, String> {
    consume_manifest_from_directory(path, nonce, &launch_directory()?)
}

struct ClaimedManifest {
    path: PathBuf,
}

impl Drop for ClaimedManifest {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn consume_manifest_from_directory(
    path: &Path,
    nonce: &str,
    launch_directory: &Path,
) -> Result<TerminalLaunchManifest, String> {
    let canonical = canonical_manifest_path(path, nonce, launch_directory)?;
    let claim_sequence = MANIFEST_CLAIM_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let claimed = canonical.with_file_name(format!(
        "{nonce}.claimed-{}-{claim_sequence}.json",
        std::process::id()
    ));
    fs::rename(&canonical, &claimed)
        .map_err(|_| "terminal launch manifest could not be consumed".to_string())?;
    let claimed = ClaimedManifest { path: claimed };
    let claimed_type = fs::symlink_metadata(&claimed.path)
        .map_err(|_| "terminal launch manifest could not be read".to_string())?
        .file_type();
    if !claimed_type.is_file() {
        return Err("terminal launch manifest is not a regular file".to_string());
    }
    let bytes = fs::read(&claimed.path)
        .map_err(|_| "terminal launch manifest could not be read".to_string())?;
    let manifest: TerminalLaunchManifest = serde_json::from_slice(&bytes)
        .map_err(|_| "terminal launch manifest is malformed".to_string())?;
    manifest.validate(nonce)?;
    Ok(manifest)
}

#[cfg(unix)]
fn launch_provider(manifest: TerminalLaunchManifest) -> Result<i32, String> {
    use std::os::unix::process::CommandExt;

    let mut command = provider_command(&manifest);
    let error = command.exec();
    Err(format!("provider launch failed: {error}"))
}

#[cfg(windows)]
fn launch_provider(manifest: TerminalLaunchManifest) -> Result<i32, String> {
    let status = provider_command(&manifest)
        .status()
        .map_err(|error| format!("provider launch failed: {error}"))?;
    Ok(status.code().unwrap_or(1))
}

fn provider_command(manifest: &TerminalLaunchManifest) -> Command {
    let mut command = Command::new(&manifest.executable);
    command
        .args(&manifest.args)
        .current_dir(&manifest.cwd)
        .envs(&manifest.env);
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Barrier};
    use wardian_core::models::TERMINAL_LAUNCH_MANIFEST_SCHEMA;

    #[test]
    fn launch_manifest_is_claimed_by_exactly_one_concurrent_consumer() {
        let root = tempfile::tempdir().unwrap();
        let launch_directory = root.path().join("runtime").join("zellij").join("launches");
        fs::create_dir_all(&launch_directory).unwrap();
        let nonce = "0123456789abcdef0123456789abcdef";
        let path = launch_directory.join(format!("{nonce}.json"));
        let manifest = TerminalLaunchManifest {
            schema: TERMINAL_LAUNCH_MANIFEST_SCHEMA,
            nonce: nonce.to_string(),
            session_id: "agent-1".to_string(),
            executable: "provider".to_string(),
            args: vec!["--resume".to_string()],
            cwd: root.path().to_string_lossy().to_string(),
            env: BTreeMap::from([("WARDIAN_SESSION_ID".to_string(), "agent-1".to_string())]),
        };
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        let barrier = Arc::new(Barrier::new(3));
        let contenders = (0..2)
            .map(|_| {
                let barrier = barrier.clone();
                let path = path.clone();
                let launch_directory = launch_directory.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    consume_manifest_from_directory(&path, nonce, &launch_directory)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = contenders
            .into_iter()
            .map(|contender| contender.join().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert_eq!(fs::read_dir(&launch_directory).unwrap().count(), 0);
    }
}
