//! Source-bound enrichment shared by capture and live archive replay.
//!
//! IDs and verified aliases identify observations. Text, timestamps and tail
//! sequence numbers are deliberately not identity evidence.
use std::collections::BTreeSet;
use std::io;

use serde_json::Value;
use wardian_core::conversations::ConversationNarrativeRecord;
use wardian_core::models::chat::{AgentChatEvent, AgentChatEventKind, AgentChatRole};

use super::{event_identity_ids, narrative_from_chat_event};

const PROVENANCE_KEYS: &[&str] = &[
    "input_origin",
    "input_purpose",
    "request_root_id",
    "causal_ref",
    "context_observation",
    "provider_turn_id",
    "provider_step_source",
];

fn string<'a>(event: &'a AgentChatEvent, key: &str) -> Option<&'a str> {
    event
        .metadata
        .get(key)?
        .as_str()
        .filter(|s| !s.trim().is_empty())
}

/// Require Wardian identity, provider and a common native source binding even
/// for an equal event ID. A conflicting explicit native session fails closed.
pub(crate) fn same_observation(old: &AgentChatEvent, current: &AgentChatEvent) -> bool {
    if old.session_id != current.session_id
        || old.provider != current.provider
        || old.kind != current.kind
        || current.metadata["provider_log"] != true
    {
        return false;
    }
    let old_session =
        string(old, "provider_session_id").or_else(|| string(old, "opencode_session_id"));
    let new_session =
        string(current, "provider_session_id").or_else(|| string(current, "opencode_session_id"));
    if matches!((old_session, new_session), (Some(a), Some(b)) if a != b) {
        return false;
    }
    let paths_match = string(old, "log_path")
        .zip(string(current, "log_path"))
        .is_some_and(|(a, b)| a == b);
    let sessions_match = old_session.zip(new_session).is_some_and(|(a, b)| a == b);
    if !paths_match && !sessions_match {
        return false;
    }
    let current_ids = event_identity_ids(current);
    event_identity_ids(old)
        .iter()
        .any(|id| !id.is_empty() && current_ids.contains(id))
}

pub(crate) fn canonicalize_role(event: &mut AgentChatEvent) {
    if event.kind == AgentChatEventKind::Message
        && event.role == Some(AgentChatRole::User)
        && matches!(
            string(event, "input_origin"),
            Some("context_injection" | "provider_internal")
        )
    {
        event.role = Some(AgentChatRole::System);
    } else if event.kind == AgentChatEventKind::ToolResult
        && event.role == Some(AgentChatRole::User)
    {
        event.role = Some(AgentChatRole::Tool);
    }
}

fn enrich(old: &mut AgentChatEvent, current: &AgentChatEvent) -> io::Result<()> {
    // An adapter downgrade must not replace native source evidence with its
    // older role-based fallback. Missing fields never erase known evidence.
    let weaker = old.metadata.get("provider_step_source").is_some()
        && current.metadata.get("provider_step_source").is_none();
    if let (Some(a), Some(b)) = (
        old.metadata.get("provider_step_source"),
        current.metadata.get("provider_step_source"),
    ) {
        if !a.is_null() && !b.is_null() && a != b {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conflicting native archive step source",
            ));
        }
    }
    let broker_input = old.metadata["generated"] == true;
    if !weaker && !broker_input {
        for key in ["request_root_id", "provider_step_source"] {
            if let (Some(a), Some(b)) = (old.metadata.get(key), current.metadata.get(key)) {
                if !a.is_null() && !b.is_null() && a != b {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("conflicting native archive provenance: {key}"),
                    ));
                }
            }
        }
        let classification_changed = string(current, "input_origin").is_some()
            && string(current, "input_origin") != string(old, "input_origin");
        let metadata = old.metadata.as_object_mut().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "archive metadata must be an object",
            )
        })?;
        for key in PROVENANCE_KEYS {
            if let Some(value) = current.metadata.get(*key).filter(|v| !v.is_null()) {
                // `unreported` conveys no new observation.
                if *key == "context_observation"
                    && value == "unreported"
                    && metadata.contains_key(*key)
                {
                    continue;
                }
                metadata.insert((*key).into(), value.clone());
            }
        }
        if classification_changed
            && matches!(string(current, "input_origin"), Some("provider_internal"))
            && current.metadata.get("request_root_id").is_none()
        {
            old.metadata
                .as_object_mut()
                .unwrap()
                .remove("request_root_id");
        }
        if old.turn_id.is_none() {
            old.turn_id = current.turn_id.clone();
        }
    }
    // Source fields are observations, not inferred from text or role. They
    // also make a reconciled broker row visible to provider-native consumers.
    for key in [
        "provider_log",
        "log_source",
        "log_path",
        "source_path",
        "provider_session_id",
        "opencode_session_id",
        "step_index",
        "raw_type",
        "provider_step_source",
    ] {
        if let Some(value) = current.metadata.get(key).filter(|value| !value.is_null()) {
            old.metadata[key] = value.clone();
        }
    }
    if current.metadata["provider_log"] == true {
        old.source = current.source.clone().or(old.source.clone());
        if broker_input {
            old.turn_id = current.turn_id.clone().or(old.turn_id.clone());
        }
    }
    let aliases: BTreeSet<String> = event_identity_ids(old)
        .into_iter()
        .chain(event_identity_ids(current))
        .filter(|id| *id != old.id)
        .map(str::to_owned)
        .collect();
    if !aliases.is_empty() {
        old.metadata["legacy_event_ids"] =
            Value::Array(aliases.into_iter().map(Value::String).collect());
    }
    canonicalize_role(old);
    Ok(())
}

/// Enrich in archive order, retaining original IDs and archive-only history.
/// A current event may collapse two legacy aliases of that same observation.
/// Build a candidate first, so conflicting evidence cannot partially mutate it.
pub(crate) fn refresh_events(
    archived: &mut Vec<AgentChatEvent>,
    current: &[AgentChatEvent],
) -> io::Result<bool> {
    let mut result = archived.clone();
    for observation in current {
        let matches: Vec<usize> = result
            .iter()
            .enumerate()
            .filter_map(|(i, old)| same_observation(old, observation).then_some(i))
            .collect();
        let Some(&first) = matches.first() else {
            continue;
        };
        let mut canonical = result[first].clone();
        for &index in matches.iter().skip(1) {
            let duplicate = &result[index];
            // Preserve archive-only metadata from either historical alias.
            if let (Some(target), Some(source)) = (
                canonical.metadata.as_object_mut(),
                duplicate.metadata.as_object(),
            ) {
                for (key, value) in source {
                    target.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
            enrich(&mut canonical, duplicate)?;
        }
        enrich(&mut canonical, observation)?;
        result[first] = canonical;
        for index in matches.into_iter().skip(1).rev() {
            result.remove(index);
        }
    }
    let changed = result != *archived;
    *archived = result;
    Ok(changed)
}

/// Merge a live capture with durable history without text-based deduplication.
/// Logging-disabled callers use this projection without writing an archive.
pub fn merge_current_capture(
    current: Vec<AgentChatEvent>,
    mut archived: Vec<AgentChatEvent>,
) -> io::Result<Vec<AgentChatEvent>> {
    refresh_events(&mut archived, &current)?;
    for mut event in current {
        if !archived.iter().any(|old| {
            same_observation(old, &event)
                || (old.metadata["provider_log"] != true
                    && event.metadata["provider_log"] != true
                    && old.id == event.id
                    && old.session_id == event.session_id
                    && old.provider == event.provider
                    && old.source == event.source)
        }) {
            canonicalize_role(&mut event);
            archived.push(event);
        }
    }
    for (index, event) in archived.iter_mut().enumerate() {
        canonicalize_role(event);
        // Preserve archive-first replay order when a bounded live tail has
        // restarted its sequence counter. This is presentation, not identity.
        event.sequence = Some(index as u64 + 1);
    }
    Ok(archived)
}

pub(super) fn refresh_records(
    records: &mut Vec<ConversationNarrativeRecord>,
    events: &[AgentChatEvent],
) {
    for event in events {
        let ids = event_identity_ids(event);
        let matches: Vec<usize> = records
            .iter()
            .enumerate()
            .filter_map(|(i, record)| {
                record
                    .event_refs
                    .iter()
                    .any(|id| ids.contains(&id.as_str()))
                    .then_some(i)
            })
            .collect();
        let Some(&first) = matches.first() else {
            continue;
        };
        let Some(projection) = narrative_from_chat_event(event, records[first].seq) else {
            continue;
        };
        let mut canonical = records[first].clone();
        canonical.turn_id = projection.turn_id.or(canonical.turn_id);
        // Broker-authored delivery provenance remains authoritative. Native
        // observation aliases enrich it without turning an agent input human.
        if !canonical
            .event_refs
            .iter()
            .any(|id| id.starts_with("generated:"))
        {
            canonical.role = projection.role;
            canonical.speaker_type = projection.speaker_type;
            canonical.input_origin = projection.input_origin;
            canonical.input_purpose = projection.input_purpose;
            if let Some(root) = string(event, "request_root_id") {
                canonical.request_root_id = Some(root.to_string());
            } else if string(event, "input_origin") == Some("provider_internal") {
                canonical.request_root_id = None;
            }
            canonical.causal_ref = projection.causal_ref.or(canonical.causal_ref);
        }
        for id in ids {
            if !canonical.event_refs.iter().any(|old| old == id) {
                canonical.event_refs.push(id.into());
            }
        }
        for &index in matches.iter().skip(1) {
            let duplicate = &records[index];
            for (target, source) in [
                (&mut canonical.event_refs, &duplicate.event_refs),
                (&mut canonical.source_refs, &duplicate.source_refs),
                (&mut canonical.artifact_refs, &duplicate.artifact_refs),
            ] {
                for value in source {
                    if !target.contains(value) {
                        target.push(value.clone());
                    }
                }
            }
        }
        records[first] = canonical;
        for index in matches.into_iter().skip(1).rev() {
            records.remove(index);
        }
    }
}

/// Reuse the broker's already-persisted event_refs reconciliation, not prompt
/// text. The ordinary generated row keeps its ID and broker input provenance,
/// while exposing the native source observation that was previously hidden.
pub(super) fn bind_delivered_inputs(
    events: &mut Vec<AgentChatEvent>,
    records: &[ConversationNarrativeRecord],
) -> io::Result<bool> {
    let before = events.clone();
    for record in records {
        let generated = events.iter().position(|event| {
            event.metadata["generated"] == true
                && event.kind == AgentChatEventKind::Message
                && record.event_refs.contains(&event.id)
        });
        let Some(generated) = generated else {
            continue;
        };
        let candidates: Vec<usize> = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| {
                (index != generated
                    && record.event_refs.contains(&event.id)
                    && event.session_id == events[generated].session_id
                    && event.provider == events[generated].provider
                    && event.metadata["provider_log"] == true
                    && event.kind == AgentChatEventKind::Message
                    && event.role == Some(AgentChatRole::User)
                    && !matches!(
                        string(event, "input_origin"),
                        Some("provider_internal" | "context_injection")
                    )
                    && (string(event, "log_path").is_some()
                        || string(event, "provider_session_id").is_some()))
                .then_some(index)
            })
            .collect();
        let [native] = candidates.as_slice() else {
            continue;
        };
        let observation = events[*native].clone();
        enrich(&mut events[generated], &observation)?;
        events.remove(*native);
    }
    Ok(*events != before)
}
