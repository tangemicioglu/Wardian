use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::library::deployments::{collect_skill_sources, scan_deployments, DeploymentScan};
use crate::library::frontmatter::extract_description;
use crate::library::metadata::MetadataStore;
use crate::library::section::LibrarySectionId;
use crate::models::{
    LibraryEntry, LibraryEntryPage, LibraryIndex, LibraryIndexFolder, LibraryIndexNode,
    LibrarySection,
};

/// Distinguishes how a section's directory tree maps onto index entries.
/// Skills are directories that contain a `SKILL.md`; prompts and workflows
/// are individual `.md` files. Both shapes allow arbitrary folder nesting
/// and share the same recursive tree walker.
enum SectionShape {
    SkillDirs,
    MarkdownFiles,
}

const MAX_LIBRARY_NODES_PER_SECTION: usize = 1_000;
const MAX_LIBRARY_DEPTH: usize = 64;

struct LibraryBuildBudget {
    remaining: usize,
    truncated: bool,
}

impl LibraryBuildBudget {
    fn new() -> Self {
        Self {
            remaining: MAX_LIBRARY_NODES_PER_SECTION,
            truncated: false,
        }
    }

    fn take(&mut self) -> bool {
        if self.remaining == 0 {
            self.truncated = true;
            return false;
        }
        self.remaining -= 1;
        true
    }
}

/// Build the full metadata-only library index: one tree per section, the
/// deployment map (section-qualified), and any orphaned deployment
/// directories. Content is only ever read to extract a description; it is
/// never stored in the returned index.
pub fn build_library_index(home: &Path) -> Result<LibraryIndex, String> {
    let sources = collect_skill_sources(home);
    let scan = scan_deployments(home, &sources);
    let metadata = MetadataStore::load(home);

    let skill_deployment_count = |rel_path: &str| -> u32 {
        scan.deployments
            .get(rel_path)
            .map(|targets| targets.len() as u32)
            .unwrap_or(0)
    };
    let no_deployment_count = |_: &str| -> u32 { 0 };

    let mut sections = HashMap::new();
    let mut skills_section = build_section(
        home,
        LibrarySectionId::Skills,
        SectionShape::SkillDirs,
        "skill",
        &metadata,
        &skill_deployment_count,
    )?;
    skills_section.truncated |= scan.truncated;
    sections.insert("skills".to_string(), skills_section);
    sections.insert(
        "prompts".to_string(),
        build_section(
            home,
            LibrarySectionId::Prompts,
            SectionShape::MarkdownFiles,
            "prompt",
            &metadata,
            &no_deployment_count,
        )?,
    );
    sections.insert(
        "workflows".to_string(),
        build_section(
            home,
            LibrarySectionId::Workflows,
            SectionShape::MarkdownFiles,
            "workflow",
            &metadata,
            &no_deployment_count,
        )?,
    );
    let mut classes_section = build_classes_section(home, &metadata, &scan)?;
    classes_section.truncated |= scan.truncated;
    sections.insert("classes".to_string(), classes_section);
    sections.insert(
        "mcps".to_string(),
        LibrarySection {
            tree: LibraryIndexFolder {
                path: String::new(),
                name: "mcps".to_string(),
                children: Vec::new(),
            },
            stubbed: true,
            truncated: false,
        },
    );

    let deployments = scan
        .deployments
        .iter()
        .map(|(rel_path, targets)| (format!("skills/{rel_path}"), targets.clone()))
        .collect();

    Ok(LibraryIndex {
        sections,
        deployments,
        orphans: scan.orphans,
    })
}

/// Return one bounded page of entries from a section.
///
/// This deliberately does not build the normal capped tree first: doing so
/// would make page two impossible to reach. The walk skips earlier entries
/// without reading their content, retains only the requested page, and stops
/// as soon as it has observed one entry beyond the page.
pub fn list_library_entries_page(
    home: &Path,
    section: LibrarySectionId,
    offset: usize,
    limit: usize,
) -> Result<LibraryEntryPage, String> {
    if section == LibrarySectionId::Mcps || limit == 0 {
        return Ok(LibraryEntryPage {
            entries: Vec::new(),
            truncated: false,
            next_offset: None,
        });
    }

    let sources = collect_skill_sources(home);
    let scan = scan_deployments(home, &sources);
    let metadata = MetadataStore::load(home);
    let skill_deployment_count = |rel_path: &str| -> u32 {
        scan.deployments
            .get(rel_path)
            .map(|targets| targets.len() as u32)
            .unwrap_or(0)
    };
    let no_deployment_count = |_: &str| -> u32 { 0 };
    let mut page = EntryPageCollector::new(offset, limit);

    if section == LibrarySectionId::Classes {
        let root = section.root_for_home(home);
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        for path in sorted_directory_entries(&root) {
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            let (description, error) = read_description(&path.join("AGENTS.md"), true);
            let entry_ref = format!("classes/{name}");
            let meta = metadata.get(&entry_ref);
            page.push(LibraryEntry {
                kind: "class".to_string(),
                path: name.clone(),
                entry_ref,
                name: name.clone(),
                description,
                tags: meta.map(|m| m.tags.clone()).unwrap_or_default(),
                is_starred: meta.map(|m| m.is_starred).unwrap_or(false),
                deployment_count: scan
                    .deployments
                    .values()
                    .flatten()
                    .filter(|target| target.target_type == "class" && target.target_id == name)
                    .count() as u32,
                error,
            });
            if page.done() {
                break;
            }
        }
    } else {
        let deployment_count_for: &dyn Fn(&str) -> u32 = match section {
            LibrarySectionId::Skills => &skill_deployment_count,
            LibrarySectionId::Prompts | LibrarySectionId::Workflows => &no_deployment_count,
            LibrarySectionId::Classes | LibrarySectionId::Mcps => unreachable!(),
        };
        let (shape, kind) = match section {
            LibrarySectionId::Skills => (SectionShape::SkillDirs, "skill"),
            LibrarySectionId::Prompts => (SectionShape::MarkdownFiles, "prompt"),
            LibrarySectionId::Workflows => (SectionShape::MarkdownFiles, "workflow"),
            LibrarySectionId::Classes | LibrarySectionId::Mcps => unreachable!(),
        };
        let root = section.root_for_home(home);
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        collect_section_page(
            &root,
            "",
            section.as_str(),
            kind,
            &shape,
            &metadata,
            deployment_count_for,
            &mut page,
            0,
        );
    }

    page.entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    let truncated = page.truncated || (section == LibrarySectionId::Skills && scan.truncated);
    Ok(LibraryEntryPage {
        next_offset: page.truncated.then_some(offset + page.entries.len()),
        entries: page.entries,
        truncated,
    })
}

struct EntryPageCollector {
    skip: usize,
    remaining: usize,
    entries: Vec<LibraryEntry>,
    truncated: bool,
}

impl EntryPageCollector {
    fn new(skip: usize, limit: usize) -> Self {
        Self {
            skip,
            remaining: limit,
            entries: Vec::with_capacity(limit),
            truncated: false,
        }
    }

    fn push(&mut self, entry: LibraryEntry) {
        if self.skip > 0 {
            self.skip -= 1;
        } else if self.remaining > 0 {
            self.remaining -= 1;
            self.entries.push(entry);
        } else {
            self.truncated = true;
        }
    }

    fn done(&self) -> bool {
        self.truncated
    }
}

fn sorted_directory_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[allow(clippy::too_many_arguments)]
fn collect_section_page(
    dir: &Path,
    rel_path: &str,
    section_prefix: &str,
    kind: &str,
    shape: &SectionShape,
    metadata: &MetadataStore,
    deployment_count_for: &dyn Fn(&str) -> u32,
    page: &mut EntryPageCollector,
    depth: usize,
) -> bool {
    if depth > MAX_LIBRARY_DEPTH || page.done() {
        return page.done();
    }
    for path in sorted_directory_entries(dir) {
        let entry_name = path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        match shape {
            SectionShape::SkillDirs => {
                if !path.is_dir() {
                    continue;
                }
                let child_rel = join_rel(rel_path, &entry_name);
                if path.join("SKILL.md").is_file() {
                    page.push(build_entry(
                        &path.join("SKILL.md"),
                        &child_rel,
                        &entry_name,
                        section_prefix,
                        kind,
                        metadata,
                        deployment_count_for,
                    ));
                } else if collect_section_page(
                    &path,
                    &child_rel,
                    section_prefix,
                    kind,
                    shape,
                    metadata,
                    deployment_count_for,
                    page,
                    depth + 1,
                ) {
                    return true;
                }
            }
            SectionShape::MarkdownFiles => {
                if path.is_dir() {
                    let child_rel = join_rel(rel_path, &entry_name);
                    if collect_section_page(
                        &path,
                        &child_rel,
                        section_prefix,
                        kind,
                        shape,
                        metadata,
                        deployment_count_for,
                        page,
                        depth + 1,
                    ) {
                        return true;
                    }
                } else if path
                    .extension()
                    .map(|ext| ext.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
                {
                    let child_rel = join_rel(rel_path, &entry_name);
                    let stem = path
                        .file_stem()
                        .map(|value| value.to_string_lossy().to_string())
                        .unwrap_or_else(|| entry_name.clone());
                    page.push(build_entry(
                        &path,
                        &child_rel,
                        &stem,
                        section_prefix,
                        kind,
                        metadata,
                        deployment_count_for,
                    ));
                }
            }
        }
        if page.done() {
            return true;
        }
    }
    false
}

/// Ensure the section directory exists (mcps is handled separately and
/// never reaches this function) and walk it into a tree.
fn build_section(
    home: &Path,
    section: LibrarySectionId,
    shape: SectionShape,
    kind: &str,
    metadata: &MetadataStore,
    deployment_count_for: &dyn Fn(&str) -> u32,
) -> Result<LibrarySection, String> {
    let root = section.root_for_home(home);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let mut budget = LibraryBuildBudget::new();
    let tree = build_folder(
        &root,
        "",
        section.as_str(),
        section.as_str(),
        kind,
        &shape,
        metadata,
        deployment_count_for,
        &mut budget,
        0,
    );
    Ok(LibrarySection {
        tree,
        stubbed: false,
        truncated: budget.truncated,
    })
}

/// Recursively build one folder node. `rel_path` is this directory's own
/// section-relative path (`""` for the section root).
#[allow(clippy::too_many_arguments)]
fn build_folder(
    dir: &Path,
    rel_path: &str,
    name: &str,
    section_prefix: &str,
    kind: &str,
    shape: &SectionShape,
    metadata: &MetadataStore,
    deployment_count_for: &dyn Fn(&str) -> u32,
    budget: &mut LibraryBuildBudget,
    depth: usize,
) -> LibraryIndexFolder {
    let mut children = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let entry_name = entry.file_name().to_string_lossy().to_string();
            match shape {
                SectionShape::SkillDirs => {
                    if !path.is_dir() {
                        continue;
                    }
                    let skill_md = path.join("SKILL.md");
                    let child_rel = join_rel(rel_path, &entry_name);
                    if skill_md.is_file() {
                        if !budget.take() {
                            break;
                        }
                        children.push(LibraryIndexNode::Entry(build_entry(
                            &skill_md,
                            &child_rel,
                            &entry_name,
                            section_prefix,
                            kind,
                            metadata,
                            deployment_count_for,
                        )));
                    } else {
                        if depth >= MAX_LIBRARY_DEPTH || !budget.take() {
                            budget.truncated = true;
                            break;
                        }
                        children.push(LibraryIndexNode::Folder(build_folder(
                            &path,
                            &child_rel,
                            &entry_name,
                            section_prefix,
                            kind,
                            shape,
                            metadata,
                            deployment_count_for,
                            budget,
                            depth + 1,
                        )));
                    }
                }
                SectionShape::MarkdownFiles => {
                    if path.is_dir() {
                        let child_rel = join_rel(rel_path, &entry_name);
                        if depth >= MAX_LIBRARY_DEPTH || !budget.take() {
                            budget.truncated = true;
                            break;
                        }
                        children.push(LibraryIndexNode::Folder(build_folder(
                            &path,
                            &child_rel,
                            &entry_name,
                            section_prefix,
                            kind,
                            shape,
                            metadata,
                            deployment_count_for,
                            budget,
                            depth + 1,
                        )));
                    } else {
                        let is_markdown = Path::new(&entry_name)
                            .extension()
                            .map(|ext| ext.eq_ignore_ascii_case("md"))
                            .unwrap_or(false);
                        if !is_markdown {
                            continue;
                        }
                        let child_rel = join_rel(rel_path, &entry_name);
                        let stem = Path::new(&entry_name)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| entry_name.clone());
                        if !budget.take() {
                            break;
                        }
                        children.push(LibraryIndexNode::Entry(build_entry(
                            &path,
                            &child_rel,
                            &stem,
                            section_prefix,
                            kind,
                            metadata,
                            deployment_count_for,
                        )));
                    }
                }
            }
        }
    }
    sort_children(&mut children);
    LibraryIndexFolder {
        path: rel_path.to_string(),
        name: name.to_string(),
        children,
    }
}

/// Build one entry node. Content at `content_path` is read only to derive
/// the description; an unreadable file yields an entry with `error` set
/// and an empty description rather than failing the whole build.
fn build_entry(
    content_path: &Path,
    rel_path: &str,
    name: &str,
    section_prefix: &str,
    kind: &str,
    metadata: &MetadataStore,
    deployment_count_for: &dyn Fn(&str) -> u32,
) -> LibraryEntry {
    let entry_ref = format!("{section_prefix}/{rel_path}");
    let (description, error) = read_description(content_path, false);
    let meta = metadata.get(&entry_ref);
    LibraryEntry {
        kind: kind.to_string(),
        path: rel_path.to_string(),
        entry_ref,
        name: name.to_string(),
        description,
        tags: meta.map(|m| m.tags.clone()).unwrap_or_default(),
        is_starred: meta.map(|m| m.is_starred).unwrap_or(false),
        deployment_count: deployment_count_for(rel_path),
        error,
    }
}

/// Classes are a flat section: each directory under `classes/` is an entry
/// (no recursion into a class's own contents), described by its
/// `AGENTS.md`. A missing `AGENTS.md` is not an error — it just yields an
/// empty description.
fn build_classes_section(
    home: &Path,
    metadata: &MetadataStore,
    scan: &DeploymentScan,
) -> Result<LibrarySection, String> {
    let root = LibrarySectionId::Classes.root_for_home(home);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;

    let mut budget = LibraryBuildBudget::new();
    let mut children = Vec::new();
    if let Ok(entries) = fs::read_dir(&root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if !budget.take() {
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let (description, error) = read_description(&path.join("AGENTS.md"), true);
            let entry_ref = format!("classes/{name}");
            let meta = metadata.get(&entry_ref);
            let deployment_count = scan
                .deployments
                .values()
                .flatten()
                .filter(|target| target.target_type == "class" && target.target_id == name)
                .count() as u32;
            children.push(LibraryIndexNode::Entry(LibraryEntry {
                kind: "class".to_string(),
                path: name.clone(),
                entry_ref,
                name: name.clone(),
                description,
                tags: meta.map(|m| m.tags.clone()).unwrap_or_default(),
                is_starred: meta.map(|m| m.is_starred).unwrap_or(false),
                deployment_count,
                error,
            }));
        }
    }
    sort_children(&mut children);
    Ok(LibrarySection {
        tree: LibraryIndexFolder {
            path: String::new(),
            name: "classes".to_string(),
            children,
        },
        stubbed: false,
        truncated: budget.truncated,
    })
}

/// Read `path` and extract its description. When `treat_missing_as_empty`
/// is set, a missing file is not an error — it's the expected shape for a
/// class directory that hasn't grown an `AGENTS.md` yet.
fn read_description(path: &Path, treat_missing_as_empty: bool) -> (String, Option<String>) {
    match fs::read_to_string(path) {
        Ok(content) => (extract_description(&content), None),
        Err(e) if treat_missing_as_empty && e.kind() == std::io::ErrorKind::NotFound => {
            (String::new(), None)
        }
        Err(e) => (String::new(), Some(e.to_string())),
    }
}

fn join_rel(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}/{name}")
    }
}

fn node_name(node: &LibraryIndexNode) -> &str {
    match node {
        LibraryIndexNode::Folder(folder) => &folder.name,
        LibraryIndexNode::Entry(entry) => &entry.name,
    }
}

/// Folders first, then entries; both groups sorted alphabetically,
/// case-insensitive — deterministic output for tests and UI.
fn sort_children(children: &mut [LibraryIndexNode]) {
    children.sort_by(|a, b| {
        let a_is_folder = matches!(a, LibraryIndexNode::Folder(_));
        let b_is_folder = matches!(b, LibraryIndexNode::Folder(_));
        match (a_is_folder, b_is_folder) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => node_name(a)
                .to_lowercase()
                .cmp(&node_name(b).to_lowercase()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn index_covers_all_sections_with_metadata_and_deployments() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path();
        // skill with frontmatter + linked deployment to class
        let skill = home
            .join("library")
            .join("skills")
            .join("dev")
            .join("planner");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\ndescription: Plans work\n---\nbody",
        )
        .unwrap();
        crate::library::links::create_directory_link(
            &skill,
            &home
                .join("classes")
                .join("Architect")
                .join(".agents")
                .join("skills")
                .join("planner"),
        )
        .unwrap();
        // prompt
        fs::create_dir_all(home.join("library").join("prompts")).unwrap();
        fs::write(
            home.join("library").join("prompts").join("greet.md"),
            "# Greeting\nHello",
        )
        .unwrap();
        // workflow
        fs::create_dir_all(home.join("library").join("workflows")).unwrap();
        fs::write(
            home.join("library").join("workflows").join("triage.md"),
            "---\ndescription: Triage\n---\n",
        )
        .unwrap();
        // class AGENTS.md
        fs::write(
            home.join("classes").join("Architect").join("AGENTS.md"),
            "# Role: Architect\nDesigns",
        )
        .unwrap();
        // starred metadata (already-qualified key)
        fs::write(
            home.join("library").join("library.json"),
            r#"{"skills/dev/planner": {"id":"m1","tags":["dev"],"is_starred":true,"last_used":null}}"#,
        ).unwrap();

        let index = build_library_index(home).expect("index");

        let skills = &index.sections["skills"];
        let dev = match &skills.tree.children[0] {
            LibraryIndexNode::Folder(f) => f,
            _ => panic!("dev folder"),
        };
        let planner = match &dev.children[0] {
            LibraryIndexNode::Entry(e) => e,
            _ => panic!("planner entry"),
        };
        assert_eq!(planner.entry_ref, "skills/dev/planner");
        assert_eq!(planner.description, "Plans work");
        assert!(planner.is_starred);
        assert_eq!(planner.tags, vec!["dev".to_string()]);
        assert_eq!(planner.deployment_count, 1);

        let prompts = &index.sections["prompts"];
        let greet = match &prompts.tree.children[0] {
            LibraryIndexNode::Entry(e) => e,
            _ => panic!("greet"),
        };
        assert_eq!(greet.kind, "prompt");
        assert_eq!(greet.name, "greet");
        assert_eq!(greet.description, "Greeting");

        assert_eq!(index.sections["workflows"].tree.children.len(), 1);

        let classes = &index.sections["classes"];
        let architect = match &classes.tree.children[0] {
            LibraryIndexNode::Entry(e) => e,
            _ => panic!("class"),
        };
        assert_eq!(architect.entry_ref, "classes/Architect");
        assert_eq!(architect.description, "Role: Architect");
        assert_eq!(architect.deployment_count, 1);

        assert!(index.sections["mcps"].stubbed);
        assert!(index.sections["mcps"].tree.children.is_empty());
        assert!(
            !home.join("library").join("mcps").exists(),
            "stub creates no dir"
        );

        assert_eq!(index.deployments["skills/dev/planner"].len(), 1);
        assert!(index.orphans.is_empty());
    }

    #[test]
    fn unreadable_entries_carry_error_flag() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path();
        let skill = home.join("library").join("skills").join("broken");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), [0xFF, 0xFE, 0x00]).unwrap(); // invalid UTF-8

        let index = build_library_index(home).expect("index survives");
        let broken = match &index.sections["skills"].tree.children[0] {
            LibraryIndexNode::Entry(e) => e,
            _ => panic!("entry expected"),
        };
        assert!(broken.error.is_some() || broken.description.is_empty());
    }

    #[test]
    fn built_index_round_trips_through_json() {
        let temp = tempfile::tempdir().expect("temp");
        let home = temp.path();
        let skill = home.join("library").join("skills").join("planner");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\ndescription: Plans work\n---\n",
        )
        .unwrap();
        crate::library::links::create_directory_link(
            &skill,
            &home
                .join("common")
                .join(".agents")
                .join("skills")
                .join("planner"),
        )
        .unwrap();

        let index = build_library_index(home).expect("index");
        assert_eq!(index.deployments["skills/planner"].len(), 1);

        let json = serde_json::to_string(&index).expect("serialize");
        let round_tripped: LibraryIndex = serde_json::from_str(&json).expect("deserialize");

        for key in ["skills", "prompts", "workflows", "classes", "mcps"] {
            assert!(
                round_tripped.sections.contains_key(key),
                "missing section {key}"
            );
        }
        assert_eq!(
            round_tripped.deployments["skills/planner"].len(),
            index.deployments["skills/planner"].len()
        );
    }

    #[test]
    fn library_section_reports_truncation_at_the_node_budget() {
        let temp = tempfile::tempdir().expect("temp");
        let prompts = temp.path().join("library").join("prompts");
        fs::create_dir_all(&prompts).unwrap();
        for index in 0..=MAX_LIBRARY_NODES_PER_SECTION {
            fs::write(prompts.join(format!("prompt-{index:04}.md")), "# Prompt").unwrap();
        }

        let index = build_library_index(temp.path()).expect("index");
        let section = &index.sections["prompts"];
        assert!(section.truncated);
        assert_eq!(section.tree.children.len(), MAX_LIBRARY_NODES_PER_SECTION);
    }

    #[test]
    fn library_entry_pages_continue_without_returning_the_cumulative_index() {
        let temp = tempfile::tempdir().expect("temp");
        let prompts = temp.path().join("library").join("prompts");
        fs::create_dir_all(&prompts).unwrap();
        for index in 0..=MAX_LIBRARY_NODES_PER_SECTION {
            fs::write(prompts.join(format!("prompt-{index:04}.md")), "# Prompt").unwrap();
        }

        let first = list_library_entries_page(temp.path(), LibrarySectionId::Prompts, 0, 1_000)
            .expect("first page");
        assert_eq!(first.entries.len(), 1_000);
        assert!(first.truncated);
        assert_eq!(first.next_offset, Some(1_000));

        let second = list_library_entries_page(
            temp.path(),
            LibrarySectionId::Prompts,
            first.next_offset.unwrap(),
            1_000,
        )
        .expect("second page");
        assert_eq!(second.entries.len(), 1);
        assert!(!second.truncated);
        assert_eq!(second.next_offset, None);
    }
}
