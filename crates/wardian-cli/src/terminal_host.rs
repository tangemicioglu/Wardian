use crate::args::TerminalHostArgs;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use wardian_core::models::TerminalLaunchManifest;

const EXIT_LAUNCH_REJECTED: i32 = 78;
const EXIT_LAUNCH_FAILED: i32 = 70;

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

fn canonical_manifest_path(path: &Path, nonce: &str) -> Result<PathBuf, String> {
    if nonce.len() < 32
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("terminal launch nonce is invalid".to_string());
    }
    let launch_root = launch_directory()?
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
    let canonical = canonical_manifest_path(path, nonce)?;
    let bytes = fs::read(&canonical)
        .map_err(|_| "terminal launch manifest could not be read".to_string())?;
    fs::remove_file(&canonical)
        .map_err(|_| "terminal launch manifest could not be consumed".to_string())?;
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
