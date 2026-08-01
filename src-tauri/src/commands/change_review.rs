use crate::commands::git::{
    git_diff_numstat_for_cwd, git_status_for_cwd, run_git, GitNumstatEntry,
};
use crate::state::AppState;
use crate::utils::fs::get_wardian_home;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use tauri::State;
use wardian_core::conversations::{ConversationIndexEntry, ConversationTurnRecord};
use wardian_core::models::git::GitStatusResult;

const CHANGE_REVIEW_SCHEMA: u8 = 1;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReviewBaseline {
    LastEffectiveTurn,
    ConversationStart,
    BranchPoint,
    Head,
    Unreviewed,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReviewEvidence {
    Attributed,
    Inferred,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReviewChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeReviewFileEntry {
    pub path: String,
    pub change_kind: ChangeReviewChangeKind,
    pub old_path: Option<String>,
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
    pub evidence: ChangeReviewEvidence,
    pub agent_ids: Vec<String>,
    pub turn_indices: Vec<u64>,
    pub binary: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeReviewSummary {
    pub schema: u8,
    pub baseline: ChangeReviewBaseline,
    pub baseline_ref: Option<String>,
    pub from_turn_index: Option<u64>,
    pub to_turn_index: Option<u64>,
    pub files: Vec<ChangeReviewFileEntry>,
    pub computed_at: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeReviewWatermark {
    pub schema: u8,
    pub agent_id: String,
    pub workspace: String,
    pub reviewed_turn_index: u64,
    pub reviewed_at: String,
    pub reviewed_head: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoadChangeReviewRequest {
    pub cwd: String,
    pub baseline: ChangeReviewBaseline,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangeReviewLoadResponse {
    pub summary: ChangeReviewSummary,
    pub git_available: bool,
    pub head_ref: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Attribution {
    agent_ids: BTreeSet<String>,
    turn_indices: BTreeSet<u64>,
}

#[derive(Debug, Clone)]
struct TurnWithContext {
    entry: ConversationIndexEntry,
    turn: ConversationTurnRecord,
}

type WatermarkIndex = BTreeMap<String, ChangeReviewWatermark>;

fn normalized_path(cwd: &str, path: &str) -> String {
    let mut value = path.trim().replace('\\', "/");
    let normalized_cwd = cwd
        .trim()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    let cwd_compare = normalized_cwd.to_ascii_lowercase();
    let value_compare = value.to_ascii_lowercase();
    if value_compare == cwd_compare {
        value.clear();
    } else if value_compare.starts_with(&(cwd_compare.clone() + "/")) {
        value = value[normalized_cwd.len() + 1..].to_string();
    }
    while let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    if cfg!(windows) {
        value.to_ascii_lowercase()
    } else {
        value
    }
}

fn same_workspace(cwd: &str, workspace: &str) -> bool {
    if workspace.trim().is_empty() {
        return false;
    }
    normalized_path(cwd, workspace).is_empty()
        || normalized_path(workspace, cwd).is_empty()
        || normalized_path(cwd, workspace) == normalized_path(workspace, cwd)
}

fn watermark_key(agent_id: &str, workspace: &str) -> String {
    format!("{}\n{}", agent_id.trim(), workspace.trim())
}

fn watermark_path(home: &Path) -> std::path::PathBuf {
    home.join("changes").join("watermarks.json")
}

fn load_watermark_index(home: &Path) -> WatermarkIndex {
    let path = watermark_path(home);
    let Ok(content) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    serde_json::from_str(&content).unwrap_or_default()
}

pub(crate) fn remove_change_review_watermarks_for_agent(
    home: &Path,
    agent_id: &str,
) -> Result<(), String> {
    let path = watermark_path(home);
    if !path.exists() {
        return Ok(());
    }
    let mut index = load_watermark_index(home);
    index.retain(|_, watermark| watermark.agent_id != agent_id);
    let json = serde_json::to_string_pretty(&index).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())
}

fn load_watermark(agent_id: Option<&str>, workspace: &str) -> Option<ChangeReviewWatermark> {
    let agent_id = agent_id?.trim();
    if agent_id.is_empty() {
        return None;
    }
    let home = get_wardian_home()?;
    load_watermark_index(&home).remove(&watermark_key(agent_id, workspace))
}

fn read_turns_for_workspace(state: &AppState, cwd: &str) -> Result<Vec<TurnWithContext>, String> {
    let entries = state
        .conversation_archive
        .list(None, true)
        .map_err(|error| error.to_string())?;
    let matching_entries = entries
        .into_iter()
        .filter(|entry| same_workspace(cwd, &entry.workspace))
        .collect::<Vec<_>>();
    state
        .conversation_archive
        .turn_records_for_conversations(&matching_entries)
        .map(|records| {
            records
                .into_iter()
                .map(|(entry, turn)| TurnWithContext { entry, turn })
                .collect()
        })
        .map_err(|error| error.to_string())
}

fn add_claim(
    claims: &mut HashMap<String, Attribution>,
    cwd: &str,
    path: &str,
    agent_id: &str,
    turn_index: u64,
) {
    let key = normalized_path(cwd, path);
    if key.is_empty() {
        return;
    }
    let attribution = claims.entry(key).or_default();
    if !agent_id.trim().is_empty() {
        attribution.agent_ids.insert(agent_id.trim().to_string());
    }
    attribution.turn_indices.insert(turn_index);
}

fn attribution_for_turns(
    cwd: &str,
    turns: &[TurnWithContext],
) -> (HashMap<String, Attribution>, Option<u64>, Option<u64>) {
    let mut claims = HashMap::new();
    let mut first_turn = None;
    let mut latest_effective_turn = None;

    for record in turns {
        first_turn = Some(first_turn.map_or(record.turn.turn_index, |value: u64| {
            value.min(record.turn.turn_index)
        }));
        let mut claimed_any_path = false;
        for path in &record.turn.files.written {
            add_claim(
                &mut claims,
                cwd,
                path,
                &record.entry.agent_id,
                record.turn.turn_index,
            );
            claimed_any_path = true;
        }
        for side_effect in &record.turn.external_side_effects {
            for path in &side_effect.paths {
                add_claim(
                    &mut claims,
                    cwd,
                    path,
                    &record.entry.agent_id,
                    record.turn.turn_index,
                );
                claimed_any_path = true;
            }
        }
        if claimed_any_path {
            latest_effective_turn = Some(
                latest_effective_turn.map_or(record.turn.turn_index, |value: u64| {
                    value.max(record.turn.turn_index)
                }),
            );
        }
    }

    (claims, first_turn, latest_effective_turn)
}

fn status_change_kind(status: &str) -> ChangeReviewChangeKind {
    match status {
        "?" => ChangeReviewChangeKind::Untracked,
        "A" => ChangeReviewChangeKind::Added,
        "D" => ChangeReviewChangeKind::Deleted,
        "R" => ChangeReviewChangeKind::Renamed,
        _ => ChangeReviewChangeKind::Modified,
    }
}

fn status_entries(status: &GitStatusResult) -> BTreeMap<String, (String, ChangeReviewChangeKind)> {
    let mut entries = BTreeMap::new();
    for file in &status.files {
        let key = file.path.replace('\\', "/");
        let kind = status_change_kind(&file.status);
        entries
            .entry(key.clone())
            .and_modify(|(_, current_kind)| {
                if *current_kind == ChangeReviewChangeKind::Untracked
                    && kind != ChangeReviewChangeKind::Untracked
                {
                    *current_kind = kind;
                }
            })
            .or_insert((file.path.clone(), kind));
    }
    entries
}

fn numstat_entries(entries: Vec<GitNumstatEntry>) -> BTreeMap<String, GitNumstatEntry> {
    entries
        .into_iter()
        .map(|entry| (entry.path.replace('\\', "/"), entry))
        .collect()
}

fn build_files(
    cwd: &str,
    status: &GitStatusResult,
    numstats: Vec<GitNumstatEntry>,
    claims: &HashMap<String, Attribution>,
) -> Vec<ChangeReviewFileEntry> {
    let mut status_by_path = status_entries(status);
    let numstats = numstat_entries(numstats);
    let mut paths = BTreeSet::new();
    paths.extend(status_by_path.keys().cloned());
    paths.extend(numstats.keys().cloned());

    paths
        .into_iter()
        .filter_map(|path_key| {
            let status_entry = status_by_path.remove(&path_key);
            let numstat = numstats.get(&path_key);
            let path = status_entry
                .as_ref()
                .map(|(path, _)| path.clone())
                .or_else(|| numstat.map(|entry| entry.path.clone()))?;
            let kind = status_entry
                .as_ref()
                .map(|(_, kind)| *kind)
                .or_else(|| {
                    numstat.and_then(|entry| {
                        entry
                            .old_path
                            .as_ref()
                            .map(|_| ChangeReviewChangeKind::Renamed)
                    })
                })
                .unwrap_or(ChangeReviewChangeKind::Modified);
            let claim = claims.get(&normalized_path(cwd, &path));
            Some(ChangeReviewFileEntry {
                path,
                change_kind: kind,
                old_path: numstat.and_then(|entry| entry.old_path.clone()),
                insertions: numstat.and_then(|entry| entry.insertions),
                deletions: numstat.and_then(|entry| entry.deletions),
                evidence: if claim.is_some_and(|value| {
                    !value.agent_ids.is_empty() || !value.turn_indices.is_empty()
                }) {
                    ChangeReviewEvidence::Attributed
                } else {
                    ChangeReviewEvidence::Inferred
                },
                agent_ids: claim
                    .map(|value| value.agent_ids.iter().cloned().collect())
                    .unwrap_or_default(),
                turn_indices: claim
                    .map(|value| value.turn_indices.iter().copied().collect())
                    .unwrap_or_default(),
                binary: numstat.is_some_and(|entry| entry.binary),
                truncated: false,
            })
        })
        .collect()
}

fn build_non_git_files(claims: &HashMap<String, Attribution>) -> Vec<ChangeReviewFileEntry> {
    let mut paths = claims.keys().cloned().collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let claim = &claims[&path];
            ChangeReviewFileEntry {
                path,
                change_kind: ChangeReviewChangeKind::Modified,
                old_path: None,
                insertions: None,
                deletions: None,
                evidence: ChangeReviewEvidence::Attributed,
                agent_ids: claim.agent_ids.iter().cloned().collect(),
                turn_indices: claim.turn_indices.iter().copied().collect(),
                binary: false,
                truncated: false,
            }
        })
        .collect()
}

fn current_head(cwd: &str) -> Option<String> {
    run_git(cwd, &["rev-parse", "HEAD"])
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn branch_point(cwd: &str, head: Option<&str>) -> Option<String> {
    let head = head?;
    let symbolic_default = run_git(
        cwd,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
    let candidates = symbolic_default.into_iter().chain([
        "origin/main".to_string(),
        "origin/master".to_string(),
        "main".to_string(),
        "master".to_string(),
    ]);
    for candidate in candidates {
        if let Ok(value) = run_git(cwd, &["merge-base", head, &candidate]) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    Some(head.to_string())
}

fn revision_for_baseline(
    cwd: &str,
    baseline: ChangeReviewBaseline,
    head: Option<&str>,
    watermark: Option<&ChangeReviewWatermark>,
) -> Option<String> {
    match baseline {
        ChangeReviewBaseline::BranchPoint => branch_point(cwd, head),
        ChangeReviewBaseline::Head
        | ChangeReviewBaseline::LastEffectiveTurn
        | ChangeReviewBaseline::ConversationStart => head.map(ToString::to_string),
        ChangeReviewBaseline::Unreviewed => watermark
            .and_then(|value| value.reviewed_head.as_deref())
            .filter(|revision| run_git(cwd, &["rev-parse", "--verify", revision]).is_ok())
            .map(ToString::to_string)
            .or_else(|| head.map(ToString::to_string)),
    }
}

#[tauri::command]
pub async fn load_change_review(
    request: LoadChangeReviewRequest,
    state: State<'_, AppState>,
) -> Result<ChangeReviewLoadResponse, String> {
    let cwd = request.cwd.trim();
    if cwd.is_empty() {
        return Err("workspace is required".to_string());
    }

    let turns = read_turns_for_workspace(&state, cwd)?;
    let (claims, first_turn, latest_effective_turn) = attribution_for_turns(cwd, &turns);
    let watermark = load_watermark(request.agent_id.as_deref(), cwd);
    let head = current_head(cwd);
    let diff_revision =
        revision_for_baseline(cwd, request.baseline, head.as_deref(), watermark.as_ref());
    let baseline_ref = match request.baseline {
        ChangeReviewBaseline::LastEffectiveTurn | ChangeReviewBaseline::ConversationStart => None,
        _ => diff_revision.clone(),
    };
    let from_turn_index = match request.baseline {
        ChangeReviewBaseline::LastEffectiveTurn => latest_effective_turn,
        ChangeReviewBaseline::ConversationStart => first_turn,
        ChangeReviewBaseline::Unreviewed => watermark
            .as_ref()
            .map(|value| value.reviewed_turn_index.saturating_add(1)),
        ChangeReviewBaseline::BranchPoint | ChangeReviewBaseline::Head => None,
    };
    let to_turn_index = latest_effective_turn.or(first_turn);

    let status = match git_status_for_cwd(cwd) {
        Ok(status) => status,
        Err(_) => {
            return Ok(ChangeReviewLoadResponse {
                summary: ChangeReviewSummary {
                    schema: CHANGE_REVIEW_SCHEMA,
                    baseline: request.baseline,
                    baseline_ref: None,
                    from_turn_index,
                    to_turn_index,
                    files: build_non_git_files(&claims),
                    computed_at: chrono::Utc::now().to_rfc3339(),
                    truncated: false,
                },
                git_available: false,
                head_ref: None,
            });
        }
    };

    let numstats = git_diff_numstat_for_cwd(cwd, diff_revision.as_deref()).unwrap_or_default();
    let mut files = build_files(cwd, &status, numstats, &claims);
    if request.baseline == ChangeReviewBaseline::Unreviewed
        && watermark.as_ref().is_some_and(|value| {
            value.reviewed_turn_index >= latest_effective_turn.unwrap_or(0)
                && value.reviewed_head.is_some()
                && value.reviewed_head.as_deref() == head.as_deref()
        })
    {
        files.clear();
    }

    Ok(ChangeReviewLoadResponse {
        summary: ChangeReviewSummary {
            schema: CHANGE_REVIEW_SCHEMA,
            baseline: request.baseline,
            baseline_ref,
            from_turn_index,
            to_turn_index,
            files,
            computed_at: chrono::Utc::now().to_rfc3339(),
            truncated: false,
        },
        git_available: true,
        head_ref: head,
    })
}

#[tauri::command]
pub async fn load_change_review_watermark(
    agent_id: String,
    workspace: String,
) -> Result<Option<ChangeReviewWatermark>, String> {
    Ok(load_watermark(Some(&agent_id), &workspace))
}

#[tauri::command]
pub async fn save_change_review_watermark(watermark: ChangeReviewWatermark) -> Result<(), String> {
    let home = get_wardian_home().ok_or_else(|| "Could not find home directory".to_string())?;
    let changes_dir = home.join("changes");
    std::fs::create_dir_all(&changes_dir).map_err(|error| error.to_string())?;
    let path = watermark_path(&home);
    let mut index = load_watermark_index(&home);
    index.insert(
        watermark_key(&watermark.agent_id, &watermark.workspace),
        ChangeReviewWatermark {
            schema: CHANGE_REVIEW_SCHEMA,
            ..watermark
        },
    );
    let json = serde_json::to_string_pretty(&index).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardian_core::models::git::GitFileEntry;

    fn status_for(path: &str, status: &str) -> GitStatusResult {
        GitStatusResult {
            branch: "main".to_string(),
            upstream: None,
            has_upstream: false,
            files: vec![GitFileEntry {
                path: path.to_string(),
                status: status.to_string(),
                is_staged: false,
            }],
            ahead: 0,
            behind: 0,
            rebase_in_progress: false,
        }
    }

    #[test]
    fn unclaimed_git_file_is_inferred_with_empty_attribution() {
        let files = build_files(
            "C:/repo",
            &status_for("src/shell-written.ts", "M"),
            vec![GitNumstatEntry {
                path: "src/shell-written.ts".to_string(),
                old_path: None,
                insertions: Some(2),
                deletions: Some(1),
                binary: false,
            }],
            &HashMap::new(),
        );

        assert_eq!(files[0].evidence, ChangeReviewEvidence::Inferred);
        assert!(files[0].agent_ids.is_empty());
        assert!(files[0].turn_indices.is_empty());
    }

    #[test]
    fn claimed_git_file_is_attributed_without_filtering_other_files() {
        let mut claims = HashMap::new();
        add_claim(&mut claims, "C:/repo", "src/agent.ts", "agent-1", 7);
        let status = GitStatusResult {
            files: vec![
                GitFileEntry {
                    path: "src/agent.ts".to_string(),
                    status: "M".to_string(),
                    is_staged: false,
                },
                GitFileEntry {
                    path: "src/shell.ts".to_string(),
                    status: "M".to_string(),
                    is_staged: false,
                },
            ],
            ..status_for("unused", "M")
        };
        let files = build_files("C:/repo", &status, Vec::new(), &claims);

        assert_eq!(files.len(), 2);
        let attributed = files
            .iter()
            .find(|file| file.path == "src/agent.ts")
            .unwrap();
        assert_eq!(attributed.evidence, ChangeReviewEvidence::Attributed);
        assert_eq!(attributed.agent_ids, vec!["agent-1"]);
        assert_eq!(attributed.turn_indices, vec![7]);
        let inferred = files
            .iter()
            .find(|file| file.path == "src/shell.ts")
            .unwrap();
        assert_eq!(inferred.evidence, ChangeReviewEvidence::Inferred);
    }
}
