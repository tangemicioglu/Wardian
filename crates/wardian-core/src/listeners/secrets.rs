//! Credentials for listeners, kept out of the inspectable listener config.
//!
//! `listeners.json` is meant to be read: printed by the CLI, rendered in the
//! UI, inspected on disk, pasted into an issue. Webhook secrets and
//! credential-bearing poll headers cannot live there and keep that property,
//! and HMAC verification needs the raw secret so hashing is not an option.
//! Separating the file is what makes the config safe to show.

use fs2::FileExt;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;

/// Bytes of entropy in a generated webhook secret.
const SECRET_BYTES: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerSecret {
    /// Shared token or HMAC key for a webhook listener.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_secret: Option<String>,
    /// Credential-bearing request headers for a poll listener, such as
    /// `Authorization`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
}

impl ListenerSecret {
    pub fn is_empty(&self) -> bool {
        self.webhook_secret.is_none() && self.headers.is_empty()
    }
}

type SecretStore = BTreeMap<String, ListenerSecret>;

/// A URL-safe secret with 256 bits of entropy.
pub fn generate_secret() -> String {
    let mut bytes = [0_u8; SECRET_BYTES];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_store(path: &std::path::Path) -> SecretStore {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}

/// Serialize the whole read-modify-write, matching the listener config's lock
/// discipline so a CLI and app write cannot lose each other's change.
pub fn mutate_secrets<T>(
    mutate: impl FnOnce(&mut SecretStore) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let path = crate::paths::listener_secrets_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Wardian home is unavailable")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    FileExt::lock_exclusive(&lock)?;
    let mut store = read_store(&path);
    let result = mutate(&mut store)?;
    write_store(&path, &store)?;
    Ok(result)
}

fn write_store(path: &std::path::Path, store: &SecretStore) -> std::io::Result<()> {
    let body = serde_json::to_string_pretty(store)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    restrict_to_owner(&tmp)?;
    std::fs::rename(&tmp, path)?;
    restrict_to_owner(path)?;
    Ok(())
}

/// Narrow the file to the owning user where the platform expresses that in
/// permission bits. On Windows the Wardian home is already user-scoped, which
/// is the posture `remote/storage.rs` relies on for device credentials.
#[cfg(unix)]
fn restrict_to_owner(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

pub fn load_secret(listener_id: &str) -> Option<ListenerSecret> {
    let path = crate::paths::listener_secrets_path()?;
    read_store(&path).get(listener_id).cloned()
}

pub fn set_secret(listener_id: &str, secret: ListenerSecret) -> std::io::Result<()> {
    mutate_secrets(|store| {
        if secret.is_empty() {
            store.remove(listener_id);
        } else {
            store.insert(listener_id.to_string(), secret);
        }
        Ok(())
    })
}

/// Drop a listener's credentials. Called when a listener is removed so a
/// deleted webhook does not leave a live secret behind.
pub fn remove_secret(listener_id: &str) -> std::io::Result<()> {
    mutate_secrets(|store| {
        store.remove(listener_id);
        Ok(())
    })
}

/// Delete credentials for listeners that no longer exist.
pub fn prune_secrets(live_ids: &[String]) -> std::io::Result<usize> {
    mutate_secrets(|store| {
        let before = store.len();
        store.retain(|id, _| live_ids.iter().any(|live| live == id));
        Ok(before - store.len())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHome {
        _guard: std::sync::MutexGuard<'static, ()>,
        _home: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let guard = crate::tests::env_lock();
            let home = tempfile::tempdir().expect("temp wardian home");
            let previous = std::env::var_os("WARDIAN_HOME");
            std::env::set_var("WARDIAN_HOME", home.path());
            Self {
                _guard: guard,
                _home: home,
                previous,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("WARDIAN_HOME", value),
                None => std::env::remove_var("WARDIAN_HOME"),
            }
        }
    }

    #[test]
    fn generated_secrets_are_unique_and_long_enough() {
        let first = generate_secret();
        assert_eq!(first.len(), SECRET_BYTES * 2);
        assert!(first.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first, generate_secret());
    }

    #[test]
    fn secrets_round_trip_and_are_removable() {
        let _home = TestHome::new();
        let secret = ListenerSecret {
            webhook_secret: Some("s3cret".into()),
            headers: BTreeMap::from([("Authorization".to_string(), "Bearer x".to_string())]),
        };
        set_secret("hook", secret.clone()).expect("set");
        assert_eq!(load_secret("hook"), Some(secret));

        remove_secret("hook").expect("remove");
        assert_eq!(load_secret("hook"), None);
    }

    #[test]
    fn secrets_never_reach_the_inspectable_listener_config() {
        let _home = TestHome::new();
        set_secret(
            "hook",
            ListenerSecret {
                webhook_secret: Some("s3cret".into()),
                ..ListenerSecret::default()
            },
        )
        .expect("set");
        crate::listeners::save_listeners(&[crate::listeners::test_support::listener(
            "hook",
            crate::listeners::test_support::file_trigger("/tmp/watched"),
        )])
        .expect("save listeners");

        let config = std::fs::read_to_string(crate::paths::listeners_path().expect("path"))
            .expect("read config");
        assert!(
            !config.contains("s3cret"),
            "the listener config must stay safe to display"
        );
    }

    #[test]
    fn pruning_drops_only_secrets_for_removed_listeners() {
        let _home = TestHome::new();
        for id in ["kept", "removed"] {
            set_secret(
                id,
                ListenerSecret {
                    webhook_secret: Some(format!("{id}-secret")),
                    ..ListenerSecret::default()
                },
            )
            .expect("set");
        }
        assert_eq!(prune_secrets(&["kept".to_string()]).expect("prune"), 1);
        assert!(load_secret("kept").is_some());
        assert!(load_secret("removed").is_none());
    }

    #[test]
    fn an_empty_secret_record_is_stored_as_absence() {
        let _home = TestHome::new();
        set_secret("hook", ListenerSecret::default()).expect("set");
        assert_eq!(load_secret("hook"), None);
    }
}
