use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::section::LibrarySectionId;
use crate::models::LibraryItemMetadata;
use crate::paths::library_metadata_path_for_home;

const SECTION_PREFIXES: [&str; 4] = ["skills/", "prompts/", "automations/", "classes/"];

#[derive(Debug, Default)]
pub struct MetadataStore {
    items: BTreeMap<String, LibraryItemMetadata>,
}

impl MetadataStore {
    pub fn load(home: &Path) -> MetadataStore {
        let path = library_metadata_path_for_home(home);
        let raw: BTreeMap<String, LibraryItemMetadata> = fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();

        let mut items = BTreeMap::new();
        let mut migrated = false;
        for (key, value) in raw {
            if let Some(suffix) = key.strip_prefix("workflows/") {
                migrated = true;
                let automation_key = format!("automations/{suffix}");
                if let Some(existing) = items.get_mut(&automation_key) {
                    merge_metadata(existing, value);
                } else {
                    items.insert(automation_key, value);
                }
            } else if SECTION_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
            {
                items.insert(key, value);
            } else if let Some(qualified) = qualify_legacy_key(home, &key) {
                migrated = true;
                items.insert(qualified, value);
            } else {
                migrated = true; // dropped key still means the file must be rewritten
            }
        }

        let store = MetadataStore { items };
        if migrated {
            let _ = store.save(home);
        }
        store
    }

    pub fn get(&self, entry_ref: &str) -> Option<&LibraryItemMetadata> {
        self.items.get(entry_ref)
    }

    pub fn set(&mut self, entry_ref: String, metadata: LibraryItemMetadata) {
        self.items.insert(entry_ref, metadata);
    }

    pub fn rename(&mut self, old_ref: &str, new_ref: &str) {
        if let Some(metadata) = self.items.remove(old_ref) {
            self.items.insert(new_ref.to_string(), metadata);
        }
    }

    pub fn remove(&mut self, entry_ref: &str) {
        self.items.remove(entry_ref);
    }

    pub fn save(&self, home: &Path) -> Result<(), String> {
        let path = library_metadata_path_for_home(home);
        crate::atomic_file::write_json_atomic(&path, &self.items).map_err(|e| e.to_string())
    }
}

/// Merge metadata when both legacy and canonical keys describe the same
/// automation. Canonical identity wins, while user-visible collections and
/// activity state retain information from both entries.
fn merge_metadata(canonical: &mut LibraryItemMetadata, legacy: LibraryItemMetadata) {
    if canonical.id.is_empty() {
        canonical.id = legacy.id;
    }
    for tag in legacy.tags {
        if !canonical.tags.contains(&tag) {
            canonical.tags.push(tag);
        }
    }
    canonical.is_starred |= legacy.is_starred;
    canonical.last_used = match (canonical.last_used.take(), legacy.last_used) {
        (Some(current), Some(previous)) => Some(current.max(previous)),
        (Some(current), None) => Some(current),
        (None, Some(previous)) => Some(previous),
        (None, None) => None,
    };
}

fn qualify_legacy_key(home: &Path, key: &str) -> Option<String> {
    let skills_probe = LibrarySectionId::Skills.root_for_home(home).join(key);
    if skills_probe.is_dir() {
        return Some(format!("skills/{key}"));
    }
    let prompts_probe = LibrarySectionId::Prompts.root_for_home(home).join(key);
    if prompts_probe.is_file() {
        return Some(format!("prompts/{key}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LibraryItemMetadata;
    use std::fs;

    fn meta(id: &str) -> LibraryItemMetadata {
        LibraryItemMetadata {
            id: id.to_string(),
            tags: vec![],
            is_starred: true,
            last_used: None,
        }
    }

    #[test]
    fn migrates_legacy_keys_by_probing_sections() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path();
        fs::create_dir_all(
            home.join("library")
                .join("skills")
                .join("dev")
                .join("planner"),
        )
        .unwrap();
        fs::create_dir_all(home.join("library").join("prompts")).unwrap();
        fs::write(home.join("library").join("prompts").join("greet.md"), "hi").unwrap();
        let legacy = serde_json::json!({
            "dev/planner": {"id": "s1", "tags": [], "is_starred": true, "last_used": null},
            "greet.md": {"id": "p1", "tags": [], "is_starred": false, "last_used": null},
            "ghost.md": {"id": "g1", "tags": [], "is_starred": false, "last_used": null}
        });
        fs::write(
            home.join("library").join("library.json"),
            legacy.to_string(),
        )
        .unwrap();

        let store = MetadataStore::load(home);
        assert_eq!(
            store.get("skills/dev/planner").expect("skill migrated").id,
            "s1"
        );
        assert_eq!(
            store.get("prompts/greet.md").expect("prompt migrated").id,
            "p1"
        );
        assert!(store.get("ghost.md").is_none(), "unresolvable keys drop");

        // Migration writes back once: reloading needs no probing.
        let raw = fs::read_to_string(home.join("library").join("library.json")).unwrap();
        assert!(raw.contains("skills/dev/planner"));
    }

    #[test]
    fn migrates_workflow_metadata_to_automation_keys() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path();
        let legacy = serde_json::json!({
            "workflows/review.md": {
                "id": "review",
                "tags": ["release", "nightly"],
                "is_starred": true,
                "last_used": "2026-08-29T10:00:00Z"
            }
        });
        fs::create_dir_all(home.join("library")).unwrap();
        fs::write(
            home.join("library").join("library.json"),
            legacy.to_string(),
        )
        .unwrap();

        let store = MetadataStore::load(home);
        let metadata = store
            .get("automations/review.md")
            .expect("automation metadata migrated");
        assert_eq!(metadata.tags, ["release", "nightly"]);
        assert!(metadata.is_starred);
        assert_eq!(metadata.last_used.as_deref(), Some("2026-08-29T10:00:00Z"));

        let persisted = fs::read_to_string(home.join("library").join("library.json")).unwrap();
        assert!(persisted.contains("automations/review.md"));
        assert!(!persisted.contains("workflows/review.md"));
    }

    #[test]
    fn canonical_metadata_wins_identity_and_merges_legacy_activity() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path();
        let metadata = serde_json::json!({
            "automations/review.md": {
                "id": "canonical",
                "tags": ["canonical"],
                "is_starred": false,
                "last_used": "2026-08-28T10:00:00Z"
            },
            "workflows/review.md": {
                "id": "legacy",
                "tags": ["legacy"],
                "is_starred": true,
                "last_used": "2026-08-29T10:00:00Z"
            }
        });
        fs::create_dir_all(home.join("library")).unwrap();
        fs::write(
            home.join("library").join("library.json"),
            metadata.to_string(),
        )
        .unwrap();

        let store = MetadataStore::load(home);
        let merged = store
            .get("automations/review.md")
            .expect("canonical metadata remains");
        assert_eq!(merged.id, "canonical");
        assert_eq!(merged.tags, ["canonical", "legacy"]);
        assert!(merged.is_starred);
        assert_eq!(merged.last_used.as_deref(), Some("2026-08-29T10:00:00Z"));
        assert!(store.get("workflows/review.md").is_none());
    }

    #[test]
    fn rename_moves_metadata() {
        let temp = tempfile::tempdir().expect("temp");
        let mut store = MetadataStore::default();
        store.set("skills/old".into(), meta("m1"));
        store.rename("skills/old", "skills/new");
        assert!(store.get("skills/old").is_none());
        assert_eq!(store.get("skills/new").expect("moved").id, "m1");
        store.save(temp.path()).expect("save");
        assert!(MetadataStore::load(temp.path()).get("skills/new").is_some());
    }

    #[test]
    fn save_replaces_metadata_with_complete_pretty_json() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("library").join("library.json");
        fs::create_dir_all(path.parent().expect("parent")).expect("metadata dir");
        fs::write(&path, "truncated").expect("old metadata");
        let mut store = MetadataStore::default();
        store.set("skills/planner".into(), meta("planner"));

        store.save(temp.path()).expect("save metadata");

        let raw = fs::read_to_string(&path).expect("metadata json");
        assert!(raw.starts_with("{\n"));
        assert!(raw.ends_with("\n"));
        serde_json::from_str::<serde_json::Value>(&raw).expect("valid metadata json");
        assert!(!path.with_file_name(".library.json.tmp").exists());
    }
}
