//! Matching and path-safety rules for file-watch listeners.
//!
//! The dangerous failure here is not authenticity, it is amplification: a run
//! that writes into a watched tree retriggers itself, spawning agent sessions
//! until the machine dies. Containment is layered — static refusals at config
//! time (this module), default ignores (this module), and a runtime rate
//! ceiling in the parent module for the loops static checks cannot see.

use super::{FileChangeKind, FileWatchTrigger, MAX_DEBOUNCE_MS};
use std::path::{Path, PathBuf};

/// Ignore globs applied under any user-supplied ones.
///
/// These are the directories that generate enormous, uninteresting event
/// volume. Watching `node_modules` is never what someone meant.
pub const DEFAULT_IGNORE_GLOBS: &[&str] = &[
    "**/.git/**",
    "**/node_modules/**",
    "**/target/**",
    "**/dist/**",
    "**/build/**",
    "**/.venv/**",
    "**/__pycache__/**",
    "**/.next/**",
    "**/*.tmp",
    "**/*.swp",
    "**/~*",
];

fn glob_options() -> glob::MatchOptions {
    glob::MatchOptions {
        case_sensitive: !cfg!(windows),
        // `*` must not cross a path separator, so `*.rs` means "in this
        // directory" and `**/*.rs` means "anywhere below".
        require_literal_separator: true,
        require_literal_leading_dot: false,
    }
}

fn compile(pattern: &str) -> Result<glob::Pattern, String> {
    glob::Pattern::new(pattern).map_err(|error| format!("invalid glob `{pattern}`: {error}"))
}

/// Normalize a path to the forward-slash relative form globs are written
/// against, so one pattern behaves the same on Windows and Unix.
pub fn relative_glob_path(root: &Path, changed: &Path) -> Option<String> {
    let relative = changed.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            // A watch root is absolute and canonical, so a relative path
            // containing `..` or a prefix is not something we can reason
            // about; refuse rather than guess.
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Whether one changed path fires this trigger.
pub fn matches(trigger: &FileWatchTrigger, changed: &Path, kind: FileChangeKind) -> bool {
    if !trigger.events.is_empty() && !trigger.events.contains(&kind) {
        return false;
    }
    let root = PathBuf::from(&trigger.path);
    // Watching a single file: the event path is the root itself.
    let relative = if root == changed {
        match changed.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => return false,
        }
    } else {
        match relative_glob_path(&root, changed) {
            Some(relative) => relative,
            None => return false,
        }
    };
    let options = glob_options();

    for pattern in DEFAULT_IGNORE_GLOBS
        .iter()
        .map(|pattern| (*pattern).to_string())
        .chain(trigger.ignore.iter().cloned())
    {
        if let Ok(compiled) = compile(&pattern) {
            if compiled.matches_with(&relative, options) {
                return false;
            }
        }
    }

    if trigger.patterns.is_empty() {
        return true;
    }
    trigger.patterns.iter().any(|pattern| {
        compile(pattern)
            .map(|compiled| compiled.matches_with(&relative, options))
            .unwrap_or(false)
    })
}

/// Reject watch roots that would make a listener trigger itself or drown the
/// machine. Fail-closed, so the CLI and the UI inherit the same refusals.
pub fn validate_watch_root(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("file listeners require a watch path".to_string());
    }
    let candidate = PathBuf::from(trimmed);
    if !candidate.is_absolute() {
        return Err(format!("watch path must be absolute: `{trimmed}`"));
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("watch path is not accessible: {error}"))?;
    let canonical = strip_windows_verbatim(canonical);

    if canonical.parent().is_none() {
        return Err(format!(
            "refusing to watch the filesystem root `{}`",
            canonical.display()
        ));
    }
    if let Some(user_home) = dirs::home_dir().and_then(|home| home.canonicalize().ok()) {
        if canonical == strip_windows_verbatim(user_home) {
            return Err("refusing to watch the entire user home directory".to_string());
        }
    }
    if let Some(wardian_home) =
        crate::paths::wardian_home().and_then(|home| home.canonicalize().ok())
    {
        let wardian_home = strip_windows_verbatim(wardian_home);
        // Either containment direction is a loop: watching inside the Wardian
        // home sees the run's own log writes, and watching a parent of it sees
        // them too.
        if canonical.starts_with(&wardian_home) || wardian_home.starts_with(&canonical) {
            return Err(
                "refusing to watch a path that contains or sits inside the Wardian home; automation runs write there, so the listener would trigger itself"
                    .to_string(),
            );
        }
    }
    Ok(canonical)
}

#[cfg(windows)]
fn strip_windows_verbatim(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    value
        .strip_prefix(r"\\?\")
        .map(PathBuf::from)
        .unwrap_or(path)
}

#[cfg(not(windows))]
fn strip_windows_verbatim(path: PathBuf) -> PathBuf {
    path
}

pub fn validate(trigger: &FileWatchTrigger) -> Result<(), String> {
    validate_watch_root(&trigger.path)?;
    if trigger.debounce_ms > MAX_DEBOUNCE_MS {
        return Err(format!(
            "debounce_ms must be no greater than {MAX_DEBOUNCE_MS}"
        ));
    }
    for pattern in trigger.patterns.iter().chain(trigger.ignore.iter()) {
        compile(pattern)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listeners::DEFAULT_DEBOUNCE_MS;

    fn trigger(
        patterns: &[&str],
        ignore: &[&str],
        events: Vec<FileChangeKind>,
    ) -> FileWatchTrigger {
        FileWatchTrigger {
            path: if cfg!(windows) {
                r"C:\work\repo".into()
            } else {
                "/work/repo".into()
            },
            recursive: true,
            patterns: patterns.iter().map(|value| (*value).to_string()).collect(),
            ignore: ignore.iter().map(|value| (*value).to_string()).collect(),
            events,
            debounce_ms: DEFAULT_DEBOUNCE_MS,
        }
    }

    fn changed(relative: &str) -> PathBuf {
        let root = if cfg!(windows) {
            PathBuf::from(r"C:\work\repo")
        } else {
            PathBuf::from("/work/repo")
        };
        relative.split('/').fold(root, |acc, part| acc.join(part))
    }

    #[test]
    fn an_empty_pattern_list_matches_every_path() {
        let subject = trigger(&[], &[], Vec::new());
        assert!(matches(
            &subject,
            &changed("src/main.rs"),
            FileChangeKind::Modified
        ));
    }

    #[test]
    fn patterns_restrict_matching_and_do_not_cross_separators() {
        let subject = trigger(&["*.rs"], &[], Vec::new());
        assert!(matches(
            &subject,
            &changed("main.rs"),
            FileChangeKind::Modified
        ));
        assert!(
            !matches(&subject, &changed("src/main.rs"), FileChangeKind::Modified),
            "`*` must not cross a path separator"
        );

        let recursive = trigger(&["**/*.rs"], &[], Vec::new());
        assert!(matches(
            &recursive,
            &changed("src/deep/main.rs"),
            FileChangeKind::Modified
        ));
    }

    #[test]
    fn built_in_ignores_apply_without_being_configured() {
        let subject = trigger(&[], &[], Vec::new());
        for noisy in [
            "node_modules/pkg/index.js",
            "target/debug/build.rs",
            ".git/HEAD",
            "src/.git/HEAD",
            "notes.tmp",
        ] {
            assert!(
                !matches(&subject, &changed(noisy), FileChangeKind::Modified),
                "{noisy} should be ignored by default"
            );
        }
    }

    #[test]
    fn user_ignores_apply_over_a_matching_pattern() {
        let subject = trigger(&["**/*.rs"], &["**/generated/**"], Vec::new());
        assert!(matches(
            &subject,
            &changed("src/main.rs"),
            FileChangeKind::Modified
        ));
        assert!(!matches(
            &subject,
            &changed("src/generated/api.rs"),
            FileChangeKind::Modified
        ));
    }

    #[test]
    fn event_kinds_filter_when_declared() {
        let subject = trigger(&[], &[], vec![FileChangeKind::Created]);
        assert!(matches(
            &subject,
            &changed("new.txt"),
            FileChangeKind::Created
        ));
        assert!(!matches(
            &subject,
            &changed("new.txt"),
            FileChangeKind::Removed
        ));
    }

    #[test]
    fn a_path_outside_the_watch_root_never_matches() {
        let subject = trigger(&[], &[], Vec::new());
        let outside = if cfg!(windows) {
            PathBuf::from(r"C:\elsewhere\main.rs")
        } else {
            PathBuf::from("/elsewhere/main.rs")
        };
        assert!(!matches(&subject, &outside, FileChangeKind::Modified));
    }

    #[test]
    fn a_relative_watch_path_is_refused() {
        let error = validate_watch_root("relative/path").unwrap_err();
        assert!(error.contains("absolute"), "{error}");
    }

    #[test]
    fn watching_the_wardian_home_or_its_parent_is_refused() {
        let _guard = crate::tests::env_lock();
        let home = tempfile::tempdir().expect("temp home");
        let previous = std::env::var_os("WARDIAN_HOME");
        std::env::set_var("WARDIAN_HOME", home.path());

        let inside = home.path().join("library");
        std::fs::create_dir_all(&inside).expect("inside dir");

        let self_watch = validate_watch_root(&home.path().to_string_lossy()).unwrap_err();
        assert!(self_watch.contains("trigger itself"), "{self_watch}");

        let nested = validate_watch_root(&inside.to_string_lossy()).unwrap_err();
        assert!(nested.contains("trigger itself"), "{nested}");

        match previous {
            Some(value) => std::env::set_var("WARDIAN_HOME", value),
            None => std::env::remove_var("WARDIAN_HOME"),
        }
    }

    #[test]
    fn an_invalid_glob_is_refused_at_config_time() {
        let mut subject = trigger(&[], &[], Vec::new());
        let dir = tempfile::tempdir().expect("temp watch root");
        subject.path = dir.path().to_string_lossy().into_owned();
        subject.patterns = vec!["[".into()];
        let _guard = crate::tests::env_lock();
        let error = validate(&subject).unwrap_err();
        assert!(error.contains("invalid glob"), "{error}");
    }
}
