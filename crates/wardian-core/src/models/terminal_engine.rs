use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const TERMINAL_LAUNCH_MANIFEST_SCHEMA: u32 = 1;

/// One-use launch data consumed by the hidden Wardian terminal host
/// inside a Zellij pane. The manifest is deleted before the provider starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLaunchManifest {
    pub schema: u32,
    pub nonce: String,
    pub session_id: String,
    pub executable: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: BTreeMap<String, String>,
}

impl TerminalLaunchManifest {
    pub fn validate(&self, expected_nonce: &str) -> Result<(), String> {
        if self.schema != TERMINAL_LAUNCH_MANIFEST_SCHEMA {
            return Err(format!(
                "unsupported terminal launch manifest schema {}",
                self.schema
            ));
        }
        if expected_nonce.len() < 32 || self.nonce != expected_nonce {
            return Err("terminal launch nonce mismatch".to_string());
        }
        for (field, value) in [
            ("nonce", self.nonce.as_str()),
            ("session_id", self.session_id.as_str()),
            ("executable", self.executable.as_str()),
            ("cwd", self.cwd.as_str()),
        ] {
            if value.trim().is_empty() || value.contains('\0') {
                return Err(format!("terminal launch {field} is invalid"));
            }
        }
        if self.args.iter().any(|arg| arg.contains('\0')) {
            return Err("terminal launch argument contains NUL".to_string());
        }
        if self.env.iter().any(|(key, value)| {
            key.trim().is_empty() || key.contains(['=', '\0']) || value.contains('\0')
        }) {
            return Err("terminal launch environment is invalid".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> TerminalLaunchManifest {
        TerminalLaunchManifest {
            schema: TERMINAL_LAUNCH_MANIFEST_SCHEMA,
            nonce: "0123456789abcdef0123456789abcdef".to_string(),
            session_id: "agent-1".to_string(),
            executable: "provider".to_string(),
            args: vec!["--resume".to_string(), "exact value".to_string()],
            cwd: "/workspace".to_string(),
            env: BTreeMap::from([("WARDIAN_SESSION_ID".to_string(), "agent-1".to_string())]),
        }
    }

    #[test]
    fn terminal_launch_manifest_accepts_exact_argument_and_environment_vectors() {
        let manifest = manifest();
        assert_eq!(manifest.validate(&manifest.nonce), Ok(()));
    }

    #[test]
    fn terminal_launch_manifest_rejects_wrong_nonce_schema_and_nul_values() {
        let mut candidate = manifest();
        assert_eq!(
            candidate.validate("fedcba9876543210fedcba9876543210"),
            Err("terminal launch nonce mismatch".to_string())
        );
        candidate.schema = 2;
        assert_eq!(
            candidate.validate("0123456789abcdef0123456789abcdef"),
            Err("unsupported terminal launch manifest schema 2".to_string())
        );
        candidate.schema = TERMINAL_LAUNCH_MANIFEST_SCHEMA;
        candidate.args.push("bad\0arg".to_string());
        assert_eq!(
            candidate.validate("0123456789abcdef0123456789abcdef"),
            Err("terminal launch argument contains NUL".to_string())
        );
    }
}
