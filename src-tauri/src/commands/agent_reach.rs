//! Which workspace roots each agent has written under.
//!
//! Garden seats districts on a ring lattice, and the lattice was built so that
//! centrality could mean something (`ringLattice.ts`). This is what gives it
//! something to mean: an agent that writes into another district's territory is
//! coordinating it, and a district full of such agents belongs nearer the middle
//! than one that only ever touches its own files.
//!
//! ## Why not change review
//!
//! `load_change_review` attributes a path to an agent by matching the
//! *conversation's* workspace against the repository being reviewed — see
//! `read_turns_for_workspace`. An agent writing across a boundary does so from a
//! conversation rooted on its own side, so its writes never appear in the other
//! repository's attribution. Cross-root reach is structurally invisible there.
//!
//! ## Why written paths rather than conversation workspaces
//!
//! The conversation index carries a `workspace` per conversation and is much
//! cheaper to read. It was measured and rejected: on a 140-agent archive only
//! seven agents had conversations in more than one workspace, and six of those
//! were the same project's worktrees or a directory that had been renamed. Turn
//! records carry the paths that were actually written, which is the thing being
//! asked about.

use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tauri::State;
use wardian_core::conversations::ConversationIndexEntry;

const AGENT_REACH_SCHEMA: u8 = 1;

/// Most-recent conversations read per agent.
///
/// Reach is a breadth question, so it wants more history than change review's
/// per-workspace window — but the archive grows without bound and this runs over
/// every agent at once. Sixty-four covers years of occasional cross-project work
/// while keeping the read proportional to the roster rather than to its age.
const AGENT_REACH_CONVERSATION_LIMIT: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentReachEntry {
    pub agent_id: String,
    /// Roots this agent has written under, spelled exactly as they were requested.
    pub roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentReachResponse {
    pub schema: u8,
    /// Agents with at least one matched root, sorted by id.
    pub agents: Vec<AgentReachEntry>,
    /// Turn records that could not be parsed, so a caller can tell "wrote
    /// nowhere" apart from "could not be read".
    pub skipped_turn_records: u64,
}

/// A requested root, kept in both spellings.
///
/// `identity` is the case-folded, slash-normalized form used for matching;
/// `requested` is what the caller sent and what comes back, so the frontend can
/// look the result up in its own map without re-normalizing.
struct RootPrefix {
    requested: String,
    identity: String,
}

fn path_identity(path: &str) -> String {
    #[cfg(windows)]
    {
        path.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.to_string()
    }
}

fn normalize(path: &str) -> String {
    path_identity(path.trim().replace('\\', "/").trim_end_matches('/'))
}

/// True when `path` looks rooted rather than relative to a workspace.
///
/// Turn records mix the two: providers report some writes relative to the
/// session's cwd and others absolutely. Treating a relative path as rooted would
/// silently match it against no root at all.
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    if path.starts_with('/') {
        return true;
    }
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// The longest requested root that contains `path`.
///
/// Longest rather than first, because roots nest: a worktree under a repository
/// is a root in its own right, and a write inside it belongs to the worktree.
fn root_for<'a>(roots: &'a [RootPrefix], path: &str) -> Option<&'a RootPrefix> {
    let candidate = normalize(path);
    let mut best: Option<&RootPrefix> = None;
    for root in roots {
        if root.identity.is_empty() {
            continue;
        }
        let contained = candidate == root.identity
            || candidate.starts_with(&format!("{}/", root.identity));
        if !contained {
            continue;
        }
        if best.is_none_or(|current| root.identity.len() > current.identity.len()) {
            best = Some(root);
        }
    }
    best
}

/// Resolve a written path against the conversation it was written in.
fn resolve(workspace: &str, path: &str) -> String {
    let trimmed = path.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return String::new();
    }
    if is_absolute(&trimmed) {
        return trimmed;
    }
    let base = workspace.trim().replace('\\', "/");
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        return String::new();
    }
    format!("{}/{}", base, trimmed.trim_start_matches("./"))
}

fn recency(entry: &ConversationIndexEntry) -> &str {
    entry
        .ended_at
        .as_deref()
        .unwrap_or(entry.started_at.as_str())
}

/// Most-recent conversations per agent, newest first, capped.
fn recent_by_agent(
    entries: Vec<ConversationIndexEntry>,
    limit: usize,
) -> BTreeMap<String, Vec<ConversationIndexEntry>> {
    let mut by_agent: BTreeMap<String, Vec<ConversationIndexEntry>> = BTreeMap::new();
    for entry in entries {
        by_agent
            .entry(entry.agent_id.clone())
            .or_default()
            .push(entry);
    }
    for conversations in by_agent.values_mut() {
        conversations.sort_by(|left, right| {
            recency(right)
                .cmp(recency(left))
                .then_with(|| right.conversation_id.cmp(&left.conversation_id))
        });
        conversations.truncate(limit);
    }
    by_agent
}

#[tauri::command]
pub async fn load_agent_reach(
    roots: Vec<String>,
    state: State<'_, AppState>,
) -> Result<AgentReachResponse, String> {
    load_agent_reach_for_state(&state, &roots)
}

pub fn load_agent_reach_for_state(
    state: &AppState,
    roots: &[String],
) -> Result<AgentReachResponse, String> {
    let prefixes = roots
        .iter()
        .map(|root| RootPrefix {
            requested: root.clone(),
            identity: normalize(root),
        })
        .filter(|root| !root.identity.is_empty())
        .collect::<Vec<_>>();
    if prefixes.is_empty() {
        return Ok(AgentReachResponse {
            schema: AGENT_REACH_SCHEMA,
            agents: Vec::new(),
            skipped_turn_records: 0,
        });
    }

    let entries = state
        .conversation_archive
        .list(None, true)
        .map_err(|error| error.to_string())?;

    let mut reached: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut skipped_turn_records = 0u64;

    for (agent_id, conversations) in recent_by_agent(entries, AGENT_REACH_CONVERSATION_LIMIT) {
        // Resilient, for the same reason change review is: one legacy turn record
        // must not blank an agent's whole history.
        let (records, skipped) = state
            .conversation_archive
            .turn_records_for_conversations_resilient(&conversations)
            .map_err(|error| error.to_string())?;
        skipped_turn_records += skipped as u64;

        for (entry, turn) in records {
            let written = turn.files.written.iter().chain(
                turn.external_side_effects
                    .iter()
                    .flat_map(|effect| effect.paths.iter()),
            );
            for path in written {
                let resolved = resolve(&entry.workspace, path);
                if resolved.is_empty() {
                    continue;
                }
                if let Some(root) = root_for(&prefixes, &resolved) {
                    reached
                        .entry(agent_id.clone())
                        .or_default()
                        .insert(root.requested.clone());
                }
            }
        }
    }

    Ok(AgentReachResponse {
        schema: AGENT_REACH_SCHEMA,
        agents: reached
            .into_iter()
            .map(|(agent_id, roots)| AgentReachEntry {
                agent_id,
                roots: roots.into_iter().collect(),
            })
            .collect(),
        skipped_turn_records,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefixes(roots: &[&str]) -> Vec<RootPrefix> {
        roots
            .iter()
            .map(|root| RootPrefix {
                requested: (*root).to_string(),
                identity: normalize(root),
            })
            .collect()
    }

    #[test]
    fn relative_paths_resolve_against_the_conversation_workspace() {
        assert_eq!(
            resolve("D:/Development/Wardian", "src/main.rs"),
            "D:/Development/Wardian/src/main.rs"
        );
        assert_eq!(
            resolve("D:/Development/Wardian", "./src/main.rs"),
            "D:/Development/Wardian/src/main.rs"
        );
    }

    #[test]
    fn absolute_paths_are_left_where_they_point() {
        assert_eq!(
            resolve("D:/Development/Wardian", "C:/Users/x/notes/plan.md"),
            "C:/Users/x/notes/plan.md"
        );
        assert_eq!(resolve("/home/x/repo", "/tmp/scratch"), "/tmp/scratch");
    }

    #[test]
    fn a_write_outside_every_root_matches_nothing() {
        // This is what keeps `.claude/`, `AppData/`, and provider scratch
        // directories out without maintaining a list of them.
        let roots = prefixes(&["D:/Development/Wardian", "C:/Users/x/academic"]);
        assert!(root_for(&roots, "C:/Users/x/AppData/Local/tmp/scratch.py").is_none());
    }

    #[test]
    fn a_nested_worktree_claims_its_own_writes() {
        let roots = prefixes(&["D:/dev/app", "D:/dev/app.wt/feature"]);
        let matched = root_for(&roots, "D:/dev/app.wt/feature/src/main.rs").expect("root");
        assert_eq!(matched.requested, "D:/dev/app.wt/feature");
    }

    #[test]
    fn the_deepest_containing_root_wins() {
        let roots = prefixes(&["D:/dev", "D:/dev/app"]);
        let matched = root_for(&roots, "D:/dev/app/src/main.rs").expect("root");
        assert_eq!(matched.requested, "D:/dev/app");
    }

    #[test]
    fn matching_ignores_case_and_separator_on_windows() {
        let roots = prefixes(&["D:/Development/Wardian"]);
        let matched = root_for(&roots, "d:\\development\\wardian\\src\\main.rs");
        #[cfg(windows)]
        assert_eq!(matched.expect("root").requested, "D:/Development/Wardian");
        #[cfg(not(windows))]
        assert!(matched.is_none());
    }

    #[test]
    fn a_root_matches_itself_but_not_a_sibling_sharing_its_prefix() {
        let roots = prefixes(&["D:/dev/app"]);
        assert!(root_for(&roots, "D:/dev/app").is_some());
        assert!(root_for(&roots, "D:/dev/application/src/main.rs").is_none());
    }

    #[test]
    fn only_the_most_recent_conversations_are_read() {
        let entries = (0..100)
            .map(|index| conversation(&format!("2026-08-{:02}T00:00:00Z", index % 28 + 1), index))
            .collect::<Vec<_>>();
        let by_agent = recent_by_agent(entries, AGENT_REACH_CONVERSATION_LIMIT);
        assert_eq!(
            by_agent["agent-1"].len(),
            AGENT_REACH_CONVERSATION_LIMIT,
            "the window must bound the read against archive age"
        );
    }

    fn conversation(started_at: &str, index: usize) -> ConversationIndexEntry {
        ConversationIndexEntry {
            schema: 1,
            conversation_id: format!("conv-{index:03}"),
            agent_id: "agent-1".to_string(),
            agent_name: "Agent".to_string(),
            agent_class: String::new(),
            workspace: "D:/dev/app".to_string(),
            provider: "mock".to_string(),
            provider_session_ids: Vec::new(),
            started_at: started_at.to_string(),
            ended_at: None,
            status: wardian_core::conversations::ConversationStatus::Closed,
            boundary_reason: wardian_core::conversations::ConversationBoundaryReason::Clear,
            first_prompt_excerpt: None,
            last_record_excerpt: None,
            record_count: 0,
            turn_count: 0,
            has_turns: true,
            lifecycle_only: false,
            artifact_count: 0,
            path: String::new(),
        }
    }
}
