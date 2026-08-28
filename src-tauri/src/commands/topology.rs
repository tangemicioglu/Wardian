use fs2::FileExt;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tauri::{AppHandle, Emitter};
use wardian_core::limits::{MAX_ACTIVITY_PAIRS, MAX_ACTIVITY_RECORDS};
use wardian_core::topology::{
    load_reconciled_topology, load_topology, pair_activity_from_records, resolve_neighbors,
    save_topology, PairActivity, Topology,
};

#[derive(Debug, Clone, Serialize)]
pub struct TopologyEdgeDto {
    pub a: String,
    pub b: String,
    /// Always "manual" in schema v3: team edges are seeded as manual edges at
    /// write time, never computed from rules at read time.
    pub origin: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TopologySnapshot {
    pub edges: Vec<TopologyEdgeDto>,
    pub ignored_pairs: Vec<[String; 2]>,
    /// Groups of agent UUIDs visible to each other only via workspace-fallback.
    /// Not currently consumed by the frontend (the halo rendering it fed was
    /// removed); kept for API stability until a consumer returns or it's retired.
    pub fallback_groups: Vec<Vec<String>>,
}

fn home() -> Result<std::path::PathBuf, String> {
    crate::utils::fs::get_wardian_home().ok_or_else(|| "WARDIAN_HOME not resolvable".to_string())
}

#[tauri::command]
pub async fn get_topology(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<TopologySnapshot, String> {
    let home = home()?;
    let _topology_lock = topology_process_lock(&home)?;
    let refs = agent_refs(&state).await;
    let topology = match wardian_core::db::get_all_agents() {
        Ok(persisted_agents) => {
            // State restoration happens asynchronously at startup. The persisted
            // roster prevents an early Graph request from treating not-yet-restored
            // agents as deleted; live state also covers a newly spawned agent.
            let mut known_agents: BTreeSet<String> = persisted_agents
                .into_iter()
                .map(|agent| agent.session_id)
                .collect();
            known_agents.extend(refs.iter().map(|agent| agent.uuid.clone()));
            load_reconciled_topology(&home, &known_agents)
                .map(|(topology, _)| topology)
                .map_err(|error| error.to_string())?
        }
        Err(error) => {
            crate::manager::log_debug(&format!(
                "[WARDIAN] topology reconciliation skipped because the persisted roster could not be read: {error}"
            ));
            load_topology(&home)
        }
    };

    let edges = snapshot_edges(&topology);

    // Fallback groups: agents whose neighbors come only from workspace-fallback.
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for agent in &refs {
        let view = resolve_neighbors(&agent.uuid, &topology, &refs);
        let only_fallback = !view.members.is_empty()
            && view.members.iter().all(|m| {
                m.reasons
                    .iter()
                    .all(|r| r.starts_with("rule:workspace-fallback"))
            });
        if only_fallback {
            if let Some(ws) = agent.workspace.clone() {
                groups.entry(ws).or_default().push(agent.uuid.clone());
            }
        }
    }

    Ok(TopologySnapshot {
        edges,
        ignored_pairs: topology
            .ignored_pairs
            .iter()
            .map(|p| [p.a.clone(), p.b.clone()])
            .collect(),
        fallback_groups: groups.into_values().filter(|g| g.len() > 1).collect(),
    })
}

/// Manual edges only. Teams have been seeded as manual edges at write time.
pub(crate) fn snapshot_edges(topology: &Topology) -> Vec<TopologyEdgeDto> {
    topology
        .edges
        .iter()
        .map(|edge| TopologyEdgeDto {
            a: edge.a.clone(),
            b: edge.b.clone(),
            origin: "manual".into(),
        })
        .collect()
}

#[tauri::command]
pub async fn add_topology_edge(app: AppHandle, a: String, b: String) -> Result<bool, String> {
    mutate(&app, |topology| {
        topology.add_edge(&a, &b, &chrono::Utc::now().to_rfc3339())
    })
}

#[tauri::command]
pub async fn remove_topology_edge(app: AppHandle, a: String, b: String) -> Result<bool, String> {
    let home = home()?;
    let teams = wardian_core::topology::load_team_memberships(&home);
    mutate(&app, |topology| {
        topology.remove_edge_and_suppress_seed_if_team_pair(&a, &b, &teams)
    })
}

#[tauri::command]
pub async fn ignore_topology_pair(app: AppHandle, a: String, b: String) -> Result<bool, String> {
    mutate(&app, |topology| topology.ignore_pair(&a, &b))
}

#[tauri::command]
pub async fn unignore_topology_pair(app: AppHandle, a: String, b: String) -> Result<bool, String> {
    mutate(&app, |topology| topology.unignore_pair(&a, &b))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PairActivityResult {
    pub pairs: Vec<PairActivity>,
    pub truncated: bool,
    pub next_offset: Option<usize>,
}

#[tauri::command]
pub async fn get_pair_activity(offset: Option<usize>) -> Result<PairActivityResult, String> {
    let offset = offset.unwrap_or(0);
    let mut records =
        wardian_core::db::list_recent_interaction_records_page(MAX_ACTIVITY_RECORDS + 1, offset)
            .map_err(|e| e.to_string())?;
    let mut truncated = records.len() > MAX_ACTIVITY_RECORDS;
    records.truncate(MAX_ACTIVITY_RECORDS);
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut pairs = pair_activity_from_records(&records, now_ms);
    pairs.sort_by(|left, right| right.last_message_at.cmp(&left.last_message_at));
    if pairs.len() > MAX_ACTIVITY_PAIRS {
        pairs.truncate(MAX_ACTIVITY_PAIRS);
        truncated = true;
    }
    Ok(PairActivityResult {
        pairs,
        truncated,
        next_offset: truncated.then_some(offset + MAX_ACTIVITY_RECORDS),
    })
}

fn mutate(app: &AppHandle, apply: impl FnOnce(&mut Topology) -> bool) -> Result<bool, String> {
    let home = home()?;
    let _topology_lock = topology_process_lock(&home)?;
    let mut topology = load_topology(&home);
    let changed = apply(&mut topology);
    if changed {
        save_topology(&home, &topology).map_err(|e| e.to_string())?;
        let _ = app.emit("topology-changed", ());
    }
    Ok(changed)
}

pub(crate) fn topology_process_lock(home: &Path) -> Result<std::fs::File, String> {
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.join("topology.lock"))
        .map_err(|error| error.to_string())?;
    lock.lock_exclusive().map_err(|error| error.to_string())?;
    Ok(lock)
}

async fn agent_refs(
    state: &tauri::State<'_, crate::state::AppState>,
) -> Vec<wardian_core::topology::AgentRef> {
    state.topology_agent_refs().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_edges_manual_only() {
        let topology = Topology {
            version: 2,
            edges: vec![wardian_core::topology::TopologyEdge {
                a: "a".to_string(),
                b: "b".to_string(),
                created_at: "2026-07-02T00:00:00Z".to_string(),
            }],
            ignored_pairs: vec![],
            suppressed_seed_pairs: vec![],
        };

        let edges = snapshot_edges(&topology);

        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].a, "a");
        assert_eq!(edges[0].b, "b");
        assert_eq!(edges[0].origin, "manual");
    }

    #[test]
    fn pair_activity_result_caps_pair_rows_and_marks_partial() {
        let pairs = (0..=MAX_ACTIVITY_PAIRS)
            .map(|index| PairActivity {
                a: format!("a-{index}"),
                b: format!("b-{index}"),
                last_message_at: format!("2026-08-25T00:{:02}:00Z", index % 60),
                active_ask: false,
                awaiting_reply_from: None,
            })
            .collect::<Vec<_>>();

        let mut bounded = pairs;
        let mut truncated = false;
        if bounded.len() > MAX_ACTIVITY_PAIRS {
            bounded.truncate(MAX_ACTIVITY_PAIRS);
            truncated = true;
        }

        assert_eq!(bounded.len(), MAX_ACTIVITY_PAIRS);
        assert!(truncated);
    }
}
