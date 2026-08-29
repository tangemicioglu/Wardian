//! Migration of the persisted automation storage layout.
//!
//! The first automation release used `workflows` in its on-disk directory
//! names.  Current code has one canonical layout, but existing installations
//! must be moved before any reader opens the new paths.  Directory moves use
//! rename where possible and reconcile an already-partially-migrated tree
//! without overwriting a conflicting entry.

use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct AutomationStorageMigrationReport {
    pub library_moved: bool,
    pub logs_moved: bool,
    pub blueprints_rewritten: usize,
}

/// Migrate one Wardian home to the canonical `automations` storage layout.
///
/// The operation is safe to retry.  A source directory is removed only after
/// every entry has either been moved or proven byte-for-byte identical to the
/// destination.  A conflicting entry returns an error and leaves both copies
/// untouched for operator recovery.
pub fn migrate_home(home: &Path) -> io::Result<AutomationStorageMigrationReport> {
    let mut report = AutomationStorageMigrationReport::default();
    let old_library = home.join("library").join("workflows");
    let new_library = home.join("library").join("automations");
    let old_root_logs = home.join("workflow_logs");
    let old_logs = home.join("logs").join("workflows");
    let new_logs = home.join("logs").join("automations");

    if reconcile_directory(&old_library, &new_library)? {
        report.library_moved = true;
    }
    for legacy_logs in [&old_root_logs, &old_logs] {
        if reconcile_directory(legacy_logs, &new_logs)? {
            report.logs_moved = true;
        }
    }

    if new_library.is_dir() {
        report.blueprints_rewritten = rewrite_legacy_blueprint_fields(&new_library)?;
    }

    Ok(report)
}

/// Resolve the current process home and run the migration, if a home exists.
pub fn migrate_current_home() -> io::Result<AutomationStorageMigrationReport> {
    let Some(home) = crate::paths::wardian_home() else {
        return Ok(AutomationStorageMigrationReport::default());
    };
    migrate_home(&home)
}

/// Reconcile `source` into `destination` without replacing destination data.
/// Returns whether source data was found or changed.
fn reconcile_directory(source: &Path, destination: &Path) -> io::Result<bool> {
    if !source.exists() {
        return Ok(false);
    }
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "legacy automation path is not a directory: {}",
                source.display()
            ),
        ));
    }
    validate_reconciliation(source, destination)?;

    if !destination.exists() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(source, destination)?;
        return Ok(true);
    }
    if !destination.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "automation migration destination is not a directory: {}",
                destination.display()
            ),
        ));
    }

    let mut changed = false;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_entry = entry.path();
        let destination_entry = destination.join(entry.file_name());
        reconcile_entry(&source_entry, &destination_entry)?;
        changed = true;
    }

    if fs::read_dir(source)?.next().is_none() {
        fs::remove_dir(source)?;
    }
    Ok(changed)
}

fn validate_reconciliation(source: &Path, destination: &Path) -> io::Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "legacy automation path is not a directory: {}",
                source.display()
            ),
        ));
    }
    if !destination.exists() {
        return Ok(());
    }
    if !destination.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "automation migration destination is not a directory: {}",
                destination.display()
            ),
        ));
    }

    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_entry = entry.path();
        let destination_entry = destination.join(entry.file_name());
        if !destination_entry.exists() {
            continue;
        }
        if source_entry.is_dir() && destination_entry.is_dir() {
            validate_reconciliation(&source_entry, &destination_entry)?;
        } else if source_entry.is_file() && destination_entry.is_file() {
            if fs::read(&source_entry)? != fs::read(&destination_entry)? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "conflicting automation migration entries: {} and {}",
                        source_entry.display(),
                        destination_entry.display()
                    ),
                ));
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "conflicting automation migration entry types: {} and {}",
                    source_entry.display(),
                    destination_entry.display()
                ),
            ));
        }
    }
    Ok(())
}

fn reconcile_entry(source: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        fs::rename(source, destination)
    } else if source.is_dir() && destination.is_dir() {
        reconcile_directory(source, destination).map(|_| ())
    } else if source.is_file() && destination.is_file() {
        if fs::read(source)? == fs::read(destination)? {
            fs::remove_file(source)
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "conflicting automation migration entries: {} and {}",
                    source.display(),
                    destination.display()
                ),
            ))
        }
    } else {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "conflicting automation migration entry types: {} and {}",
                source.display(),
                destination.display()
            ),
        ))
    }
}

fn rewrite_legacy_blueprint_fields(root: &Path) -> io::Result<usize> {
    let mut rewritten = 0;
    rewrite_blueprint_tree(root, &mut rewritten)?;
    Ok(rewritten)
}

fn rewrite_blueprint_tree(path: &Path, rewritten: &mut usize) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            rewrite_blueprint_tree(&child, rewritten)?;
        } else if child.extension().is_some_and(|extension| extension == "md")
            && rewrite_blueprint_file(&child)?
        {
            *rewritten += 1;
        }
    }
    Ok(())
}

/// Rewrite only structured front-matter keys. Markdown prose and user values
/// remain byte-for-byte unchanged.
fn rewrite_blueprint_file(path: &Path) -> io::Result<bool> {
    let mut hook = crate::atomic_file::NoAtomicFault;
    rewrite_blueprint_file_with_hook(path, &mut hook)
}

fn rewrite_blueprint_file_with_hook(
    path: &Path,
    hook: &mut impl crate::atomic_file::AtomicFaultHook,
) -> io::Result<bool> {
    let original = fs::read_to_string(path)?;
    let (bom, without_bom) = original
        .strip_prefix('\u{feff}')
        .map_or(("", original.as_str()), |rest| ("\u{feff}", rest));
    let Some((opening_newline, after_open)) = without_bom
        .strip_prefix("---\r\n")
        .map(|rest| ("\r\n", rest))
        .or_else(|| without_bom.strip_prefix("---\n").map(|rest| ("\n", rest)))
    else {
        return Ok(false);
    };
    let Some(end) = after_open
        .split_inclusive('\n')
        .position(|line| line.trim_end_matches(['\r', '\n']) == "---")
    else {
        return Ok(false);
    };

    let yaml_end = after_open
        .split_inclusive('\n')
        .take(end + 1)
        .map(str::len)
        .sum::<usize>();
    let (yaml, rest) = after_open.split_at(yaml_end);
    let mut changed = false;
    let rewritten_yaml = yaml
        .split_inclusive('\n')
        .map(|line| {
            let newline = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            let content = line.strip_suffix(newline).unwrap_or(line);
            let indent = content.len() - content.trim_start().len();
            let trimmed = content.trim_start();
            let replacement = if trimmed.starts_with("type: sub_workflow")
                && trimmed["type: sub_workflow".len()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
            {
                Some(content.replacen("sub_workflow", "sub_automation", 1))
            } else if indent > 0 && trimmed.starts_with("workflow:") {
                Some(content.replacen("workflow:", "automation:", 1))
            } else {
                None
            };
            if replacement.is_some() {
                changed = true;
            }
            format!("{}{}", replacement.as_deref().unwrap_or(content), newline)
        })
        .collect::<String>();

    if changed {
        let mut output = String::with_capacity(original.len());
        output.push_str(bom);
        output.push_str("---");
        output.push_str(opening_newline);
        output.push_str(&rewritten_yaml);
        output.push_str(rest);
        crate::atomic_file::write_bytes_atomic_durable_with_hook(
            path,
            output.as_bytes(),
            crate::atomic_file::AtomicWriteRole::Other,
            hook,
        )?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn home() -> TempDir {
        tempfile::tempdir().expect("temporary home")
    }

    #[test]
    fn moves_populated_legacy_storage_and_rewrites_structured_blueprint_fields() {
        let home = home();
        let legacy_library = home.path().join("library/workflows/nested");
        let legacy_logs = home.path().join("logs/workflows/demo/run-1");
        fs::create_dir_all(&legacy_library).unwrap();
        fs::create_dir_all(&legacy_logs).unwrap();
        fs::write(
            legacy_library.join("demo.md"),
            "---\nschema: 2\nid: demo\nname: Demo\nnodes:\n  - id: child\n    type: sub_workflow\n    fields:\n      workflow: nested\n---\n\nThis workflow prose stays unchanged.\n",
        )
        .unwrap();
        fs::write(legacy_logs.join("events.jsonl"), "event").unwrap();

        let report = migrate_home(home.path()).unwrap();

        assert!(report.library_moved);
        assert!(report.logs_moved);
        assert_eq!(report.blueprints_rewritten, 1);
        assert!(!home.path().join("library/workflows").exists());
        assert!(!home.path().join("logs/workflows").exists());
        assert!(home
            .path()
            .join("library/automations/nested/demo.md")
            .exists());
        assert!(home
            .path()
            .join("logs/automations/demo/run-1/events.jsonl")
            .exists());
        let content =
            fs::read_to_string(home.path().join("library/automations/nested/demo.md")).unwrap();
        assert!(content.contains("type: sub_automation"));
        assert!(content.contains("automation: nested"));
        assert!(content.contains("This workflow prose stays unchanged."));
    }

    #[test]
    fn retry_converges_an_already_partially_migrated_home() {
        let home = home();
        let legacy = home.path().join("library/workflows");
        let current = home.path().join("library/automations");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&current).unwrap();
        fs::write(legacy.join("old.md"), "old").unwrap();
        fs::write(current.join("same.md"), "same").unwrap();
        fs::write(legacy.join("same.md"), "same").unwrap();

        migrate_home(home.path()).unwrap();
        migrate_home(home.path()).unwrap();

        assert!(!legacy.exists());
        assert_eq!(fs::read_to_string(current.join("old.md")).unwrap(), "old");
        assert_eq!(fs::read_to_string(current.join("same.md")).unwrap(), "same");
    }

    #[test]
    fn reconciles_root_legacy_logs_after_schema_migration_is_current() {
        let home = home();
        fs::create_dir_all(home.path().join("settings")).unwrap();
        fs::write(
            home.path().join("settings/migrations.json"),
            r#"{"version":1}"#,
        )
        .unwrap();
        let root_legacy = home.path().join("workflow_logs/root/run-1");
        let nested_legacy = home.path().join("logs/workflows/nested/run-2");
        fs::create_dir_all(&root_legacy).unwrap();
        fs::create_dir_all(&nested_legacy).unwrap();
        fs::write(root_legacy.join("events.jsonl"), "root event").unwrap();
        fs::write(nested_legacy.join("events.jsonl"), "nested event").unwrap();

        let report = migrate_home(home.path()).unwrap();

        assert!(report.logs_moved);
        assert!(!home.path().join("workflow_logs").exists());
        assert!(!home.path().join("logs/workflows").exists());
        assert!(home
            .path()
            .join("logs/automations/root/run-1/events.jsonl")
            .exists());
        assert!(home
            .path()
            .join("logs/automations/nested/run-2/events.jsonl")
            .exists());
    }

    #[test]
    fn conflicting_entries_are_preserved_and_reported() {
        let home = home();
        let legacy = home.path().join("library/workflows");
        let current = home.path().join("library/automations");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&current).unwrap();
        fs::write(legacy.join("conflict.md"), "old").unwrap();
        fs::write(current.join("conflict.md"), "new").unwrap();

        let error = migrate_home(home.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(legacy.join("conflict.md")).unwrap(),
            "old"
        );
        assert_eq!(
            fs::read_to_string(current.join("conflict.md")).unwrap(),
            "new"
        );
    }

    #[test]
    fn conflicting_directory_entries_do_not_partially_migrate() {
        let home = home();
        let legacy = home.path().join("logs/workflows");
        let current = home.path().join("logs/automations");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&current).unwrap();
        fs::write(legacy.join("a-movable.jsonl"), "old movable").unwrap();
        fs::write(legacy.join("z-conflict.jsonl"), "old conflict").unwrap();
        fs::write(current.join("z-conflict.jsonl"), "new conflict").unwrap();

        let error = migrate_home(home.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(legacy.join("a-movable.jsonl").exists());
        assert!(!current.join("a-movable.jsonl").exists());
        assert_eq!(
            fs::read_to_string(legacy.join("z-conflict.jsonl")).unwrap(),
            "old conflict"
        );
        assert_eq!(
            fs::read_to_string(current.join("z-conflict.jsonl")).unwrap(),
            "new conflict"
        );
    }

    struct FailBeforeBlueprintReplace;

    impl crate::atomic_file::AtomicFaultHook for FailBeforeBlueprintReplace {
        fn check(&mut self, point: crate::atomic_file::AtomicFaultPoint) -> io::Result<()> {
            if point
                == crate::atomic_file::AtomicFaultPoint::BeforeReplace(
                    crate::atomic_file::AtomicWriteRole::Other,
                )
            {
                Err(io::Error::other("injected blueprint replacement failure"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn failed_blueprint_replacement_preserves_original_bytes() {
        let home = home();
        let blueprint = home.path().join("library/automations/review.md");
        fs::create_dir_all(blueprint.parent().unwrap()).unwrap();
        let original = "---\nschema: 2\nid: review\ntype: sub_workflow\n---\n";
        fs::write(&blueprint, original).unwrap();

        let error = rewrite_blueprint_file_with_hook(&blueprint, &mut FailBeforeBlueprintReplace)
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(fs::read_to_string(&blueprint).unwrap(), original);
        crate::atomic_file::cleanup_atomic_temps(&blueprint).unwrap();
    }

    #[test]
    fn rewrites_bom_prefixed_crlf_blueprints_without_changing_line_endings() {
        let home = home();
        let blueprint = home.path().join("library/automations/windows.md");
        fs::create_dir_all(blueprint.parent().unwrap()).unwrap();
        fs::write(
            &blueprint,
            "\u{feff}---\r\nschema: 2\r\nid: windows\r\nnodes:\r\n  - id: child\r\n    type: sub_workflow\r\n    fields:\r\n      workflow: nested\r\n---\r\n\r\nWindows prose stays unchanged.\r\n",
        )
        .unwrap();

        let report = migrate_home(home.path()).unwrap();

        assert_eq!(report.blueprints_rewritten, 1);
        let content = fs::read_to_string(blueprint).unwrap();
        assert!(content.starts_with("\u{feff}---\r\n"));
        assert!(content.contains("type: sub_automation\r\n"));
        assert!(content.contains("      automation: nested\r\n"));
        assert!(content.contains("Windows prose stays unchanged.\r\n"));
        assert!(
            !content.contains("\n")
                || content.matches('\n').count() == content.matches("\r\n").count()
        );
    }
}
