pub mod artifacts;
mod atomic_file;
pub mod browser;
pub mod classes;
pub mod control;
pub mod conversation_lease;
pub mod conversations;
pub mod db;
pub mod engine;
pub mod files;
pub mod identity;
pub mod library;
pub mod memory;
pub mod models;
pub mod paths;
pub mod schedule;
pub mod session_close;
pub mod telemetry;
pub mod topology;
pub mod workbench;
pub mod workflow;
mod workflow_approval_lock;
pub mod workflow_execution_lock;

#[cfg(test)]
mod tests {
    use once_cell::sync::Lazy;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    pub fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
