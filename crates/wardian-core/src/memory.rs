//! Provider-neutral, agent-owned memory persisted independently from conversation archives.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;
use uuid::Uuid;

const SCHEMA_VERSION: i64 = 1;
pub const DEFAULT_STALE_DAYS: i64 = 30;
pub const MEMORY_BUDGET_POLICY_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Stable,
    Current,
}

impl MemoryKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Current => "current",
        }
    }

    fn parse(value: &str) -> Result<Self, MemoryError> {
        match value {
            "stable" => Ok(Self::Stable),
            "current" => Ok(Self::Current),
            other => Err(MemoryError::Corrupt(format!("unknown memory kind {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Superseded,
    Removed,
}

impl MemoryStatus {
    fn parse(value: &str) -> Result<Self, MemoryError> {
        match value {
            "active" => Ok(Self::Active),
            "superseded" => Ok(Self::Superseded),
            "removed" => Ok(Self::Removed),
            other => Err(MemoryError::Corrupt(format!(
                "unknown memory status {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySource {
    pub source_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub revision_id: String,
    pub memory_id: String,
    pub revision: u32,
    pub agent_id: String,
    pub workspace: Option<String>,
    pub kind: MemoryKind,
    pub text: String,
    pub evidence_excerpt: String,
    pub evidence_hash: String,
    pub status: MemoryStatus,
    pub supersedes_revision_id: Option<String>,
    pub replaced_by_revision_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_verified_at: String,
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub sources: Vec<MemorySource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveMemoryRequest {
    pub agent_id: String,
    #[serde(default)]
    pub workspace: Option<String>,
    pub kind: MemoryKind,
    pub text: String,
    pub evidence_excerpt: String,
    #[serde(default)]
    pub sources: Vec<MemorySource>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMemoryRequest {
    pub memory_id: String,
    pub text: String,
    pub evidence_excerpt: String,
    #[serde(default)]
    pub sources: Vec<MemorySource>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallEntry {
    #[serde(flatten)]
    pub record: MemoryRecord,
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallResult {
    pub agent_id: String,
    pub workspace: Option<String>,
    pub stable: Vec<RecallEntry>,
    pub current: Vec<RecallEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBriefKind {
    Fresh,
    ResumeDelta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledMemoryBrief {
    pub kind: MemoryBriefKind,
    pub context_text: String,
    pub fingerprint: String,
    pub revision_ids: Vec<String>,
    pub omitted_count: usize,
    pub is_empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub event_id: String,
    pub agent_id: String,
    pub memory_id: Option<String>,
    pub revision_id: Option<String>,
    pub action: String,
    pub payload: serde_json::Value,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MemoryMutation {
    Save {
        kind: MemoryKind,
        text: String,
        evidence_excerpt: String,
        #[serde(default)]
        sources: Vec<MemorySource>,
    },
    Update {
        memory_id: String,
        text: String,
        evidence_excerpt: String,
        #[serde(default)]
        sources: Vec<MemorySource>,
    },
    Remove {
        memory_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCommitBatch {
    pub agent_id: String,
    #[serde(default)]
    pub workspace: Option<String>,
    pub idempotency_key: String,
    #[serde(default)]
    pub operations: Vec<MemoryMutation>,
    #[serde(default)]
    pub cursor: Option<MemoryCursorUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCursorUpdate {
    pub cursor_key: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCommitResult {
    pub idempotency_key: String,
    pub memory_ids: Vec<String>,
    pub replayed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("memory validation failed: {0}")]
    Validation(String),
    #[error("memory record not found: {0}")]
    NotFound(String),
    #[error("memory database is unavailable")]
    HomeUnavailable,
    #[error("memory database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("memory database is corrupt: {0}")]
    Corrupt(String),
    #[error("memory serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("memory filesystem error: {0}")]
    Io(#[from] std::io::Error),
}

pub struct MemoryStore {
    path: PathBuf,
}

struct StoredInjection {
    revision_ids: Vec<String>,
}

impl MemoryStore {
    pub fn from_default_home() -> Result<Self, MemoryError> {
        let path = crate::paths::memory_db_path().ok_or(MemoryError::HomeUnavailable)?;
        Self::open(path)
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self, MemoryError> {
        let store = Self { path: path.into() };
        if let Some(parent) = store.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = store.connection()?;
        migrate(&connection)?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connection(&self) -> Result<Connection, MemoryError> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(StdDuration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }

    pub fn save(&self, request: SaveMemoryRequest) -> Result<MemoryRecord, MemoryError> {
        let agent_id = required("agent_id", &request.agent_id)?;
        let text = normalize_text(&request.text)?;
        let evidence = normalize_evidence(&request.evidence_excerpt)?;
        let workspace = normalize_workspace(request.workspace.as_deref());
        let idempotency_key = normalize_optional(request.idempotency_key.as_deref());
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(existing) = idempotent_record(&transaction, idempotency_key.as_deref())? {
            if existing.agent_id != agent_id
                || existing.workspace != workspace
                || existing.kind != request.kind
                || existing.text != text
                || existing.evidence_excerpt != evidence
            {
                return Err(MemoryError::Validation(
                    "idempotency key was already used for a different save request".into(),
                ));
            }
            transaction.commit()?;
            return self.attach_sources(existing);
        }
        let memory_id = Uuid::new_v4().to_string();
        let revision_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let evidence_hash = hash_text(&evidence);
        transaction.execute(
            "INSERT INTO memory_records
             (revision_id, memory_id, revision, agent_id, workspace, kind, text,
              evidence_excerpt, evidence_hash, status, created_at, updated_at,
              last_verified_at, idempotency_key)
             VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?9, ?9, ?10)",
            params![
                revision_id,
                memory_id,
                agent_id,
                workspace,
                request.kind.as_str(),
                text,
                evidence,
                evidence_hash,
                now,
                idempotency_key
            ],
        )?;
        insert_sources(&transaction, &revision_id, &request.sources)?;
        insert_event(
            &transaction,
            &agent_id,
            Some(&memory_id),
            Some(&revision_id),
            "saved",
            Some(&serde_json::json!({
                "text": text,
                "evidence_excerpt": evidence,
                "evidence_hash": evidence_hash,
                "kind": request.kind,
                "workspace": workspace,
                "sources": request.sources
            })),
        )?;
        transaction.commit()?;
        self.get_revision(&revision_id)
    }

    pub fn update(&self, request: UpdateMemoryRequest) -> Result<MemoryRecord, MemoryError> {
        let memory_id = required("memory_id", &request.memory_id)?;
        let text = normalize_text(&request.text)?;
        let evidence = normalize_evidence(&request.evidence_excerpt)?;
        let idempotency_key = normalize_optional(request.idempotency_key.as_deref());
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(existing) = idempotent_record(&transaction, idempotency_key.as_deref())? {
            if existing.memory_id != memory_id
                || existing.text != text
                || existing.evidence_excerpt != evidence
            {
                return Err(MemoryError::Validation(
                    "idempotency key was already used for a different update request".into(),
                ));
            }
            transaction.commit()?;
            return self.attach_sources(existing);
        }
        let previous = query_active(&transaction, &memory_id)?
            .ok_or_else(|| MemoryError::NotFound(memory_id.clone()))?;
        let revision_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let evidence_hash = hash_text(&evidence);
        transaction.execute(
            "UPDATE memory_records SET status='superseded', replaced_by_revision_id=?1, updated_at=?2
             WHERE revision_id=?3",
            params![revision_id, now, previous.revision_id],
        )?;
        transaction.execute(
            "INSERT INTO memory_records
             (revision_id, memory_id, revision, agent_id, workspace, kind, text,
              evidence_excerpt, evidence_hash, status, supersedes_revision_id,
              created_at, updated_at, last_verified_at, idempotency_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?11, ?11, ?11, ?12)",
            params![
                revision_id,
                memory_id,
                previous.revision + 1,
                previous.agent_id,
                previous.workspace,
                previous.kind.as_str(),
                text,
                evidence,
                evidence_hash,
                previous.revision_id,
                now,
                idempotency_key
            ],
        )?;
        insert_sources(&transaction, &revision_id, &request.sources)?;
        insert_event(
            &transaction,
            &previous.agent_id,
            Some(&memory_id),
            Some(&revision_id),
            "updated",
            Some(&serde_json::json!({
                "text": text,
                "evidence_excerpt": evidence,
                "evidence_hash": evidence_hash,
                "kind": previous.kind,
                "workspace": previous.workspace,
                "sources": request.sources
            })),
        )?;
        transaction.commit()?;
        self.get_revision(&revision_id)
    }

    pub fn remove(&self, memory_id: &str) -> Result<MemoryRecord, MemoryError> {
        let memory_id = required("memory_id", memory_id)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut record = query_active(&transaction, &memory_id)?
            .ok_or_else(|| MemoryError::NotFound(memory_id.clone()))?;
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "UPDATE memory_records SET status='removed', updated_at=?1 WHERE revision_id=?2",
            params![now, record.revision_id],
        )?;
        insert_event(
            &transaction,
            &record.agent_id,
            Some(&memory_id),
            Some(&record.revision_id),
            "removed",
            None,
        )?;
        transaction.commit()?;
        record.status = MemoryStatus::Removed;
        record.updated_at = now;
        self.attach_sources(record)
    }

    pub fn get(&self, memory_id: &str) -> Result<MemoryRecord, MemoryError> {
        let connection = self.connection()?;
        let record = connection
            .query_row(
                "SELECT * FROM memory_records WHERE memory_id=?1 ORDER BY revision DESC LIMIT 1",
                [memory_id],
                row_to_record,
            )
            .optional()?
            .ok_or_else(|| MemoryError::NotFound(memory_id.to_string()))?;
        self.attach_sources(record)
    }

    pub fn history(&self, memory_id: &str) -> Result<Vec<MemoryRecord>, MemoryError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT * FROM memory_records WHERE memory_id=?1 ORDER BY revision ASC")?;
        let records = statement
            .query_map([memory_id], row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        records
            .into_iter()
            .map(|record| self.attach_sources(record))
            .collect()
    }

    pub fn list_active(
        &self,
        agent_id: &str,
        workspace: Option<&str>,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        let agent_id = required("agent_id", agent_id)?;
        let workspace = normalize_workspace(workspace);
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT * FROM memory_records
             WHERE agent_id=?1 AND status='active' AND (workspace IS NULL OR workspace=?2)
             ORDER BY CASE kind WHEN 'stable' THEN 0 ELSE 1 END,
               CASE WHEN workspace IS NULL THEN 0 ELSE 1 END,
               last_verified_at DESC, memory_id ASC",
        )?;
        let records = statement
            .query_map(params![agent_id, workspace], row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        records
            .into_iter()
            .map(|record| self.attach_sources(record))
            .collect()
    }

    pub fn recall(
        &self,
        agent_id: &str,
        workspace: Option<&str>,
    ) -> Result<RecallResult, MemoryError> {
        let workspace = normalize_workspace(workspace);
        let stale_before = Utc::now() - Duration::days(DEFAULT_STALE_DAYS);
        let mut stable = Vec::new();
        let mut current = Vec::new();
        for record in self.list_active(agent_id, workspace.as_deref())? {
            if hash_text(&record.evidence_excerpt) != record.evidence_hash {
                continue;
            }
            let stale = record.kind == MemoryKind::Current
                && DateTime::parse_from_rfc3339(&record.last_verified_at)
                    .map(|value| value.with_timezone(&Utc) < stale_before)
                    .unwrap_or(true);
            let entry = RecallEntry { record, stale };
            match entry.record.kind {
                MemoryKind::Stable => stable.push(entry),
                MemoryKind::Current => current.push(entry),
            }
        }
        Ok(RecallResult {
            agent_id: agent_id.to_string(),
            workspace,
            stable,
            current,
        })
    }

    /// Compile a deterministic startup brief. A resumed provider process gets
    /// only revisions that changed since its latest recorded fingerprint.
    pub fn compile_brief(
        &self,
        agent_id: &str,
        workspace: Option<&str>,
        provider: &str,
        provider_process_key: &str,
        resumed: bool,
        max_chars: usize,
    ) -> Result<CompiledMemoryBrief, MemoryError> {
        let recall = self.recall(agent_id, workspace)?;
        let all_active = recall
            .stable
            .iter()
            .chain(recall.current.iter())
            .map(|entry| entry.record.clone())
            .collect::<Vec<_>>();
        let revision_ids = all_active
            .iter()
            .map(|record| record.revision_id.clone())
            .collect::<Vec<_>>();
        let fingerprint = memory_fingerprint(&all_active);
        let previous = if resumed {
            self.latest_injection(agent_id, provider, provider_process_key)?
        } else {
            None
        };
        let (kind, changed, removed) = if let Some(previous) = previous {
            let previous_records = self.records_by_revision_ids(&previous.revision_ids)?;
            let previous_by_memory = previous_records
                .iter()
                .map(|record| (record.memory_id.as_str(), record))
                .collect::<std::collections::HashMap<_, _>>();
            let active_ids = all_active
                .iter()
                .map(|record| record.memory_id.as_str())
                .collect::<std::collections::HashSet<_>>();
            let removed = previous_records
                .iter()
                .filter(|record| !active_ids.contains(record.memory_id.as_str()))
                .map(|record| record.memory_id.clone())
                .collect::<Vec<_>>();
            let mut changed = all_active
                .iter()
                .filter(|record| {
                    previous_by_memory
                        .get(record.memory_id.as_str())
                        .is_none_or(|prior| prior.revision_id != record.revision_id)
                })
                .cloned()
                .collect::<Vec<_>>();
            // Every provider process must receive an authoritative checkpoint.
            // A prior process can exit before it submits a turn, so an audit
            // record alone does not prove that its model consumed the brief.
            // Re-send the active set when there is no content delta; changed
            // resumes still receive only changed and removed revisions.
            if changed.is_empty() && removed.is_empty() {
                changed = all_active.clone();
            }
            (MemoryBriefKind::ResumeDelta, changed, removed)
        } else {
            (MemoryBriefKind::Fresh, all_active, Vec::new())
        };
        let (context_text, omitted_count) = render_brief(kind, &changed, &removed, max_chars);
        Ok(CompiledMemoryBrief {
            kind,
            is_empty: context_text.is_empty(),
            context_text,
            fingerprint,
            revision_ids,
            omitted_count,
        })
    }

    pub fn record_injection(
        &self,
        agent_id: &str,
        workspace: Option<&str>,
        provider: &str,
        provider_process_key: &str,
        brief: &CompiledMemoryBrief,
    ) -> Result<Option<String>, MemoryError> {
        if brief.is_empty {
            return Ok(None);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let injection_id = Uuid::new_v4().to_string();
        let inserted = transaction.execute(
            "INSERT OR IGNORE INTO memory_injections
             (injection_id, agent_id, workspace, provider, provider_process_key, fingerprint,
              revision_ids_json, context_text, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                injection_id,
                agent_id,
                normalize_workspace(workspace),
                provider,
                provider_process_key,
                brief.fingerprint,
                serde_json::to_string(&brief.revision_ids)?,
                brief.context_text,
                Utc::now().to_rfc3339()
            ],
        )?;
        if inserted == 0 {
            transaction.commit()?;
            return Ok(None);
        }
        insert_event(
            &transaction,
            agent_id,
            None,
            None,
            "loaded",
            Some(&serde_json::json!({
                "injection_id": injection_id,
                "kind": brief.kind,
                "fingerprint": brief.fingerprint,
                "revision_ids": brief.revision_ids,
                "omitted_count": brief.omitted_count,
                "budget_policy_version": MEMORY_BUDGET_POLICY_VERSION,
                "injected_context": brief.context_text,
                "provider": provider,
                "provider_process_key": provider_process_key
            })),
        )?;
        transaction.commit()?;
        Ok(Some(injection_id))
    }

    pub fn list_events(&self, agent_id: &str) -> Result<Vec<MemoryEvent>, MemoryError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT event_id, agent_id, memory_id, revision_id, action, payload_json, occurred_at
             FROM (
               SELECT event_id, agent_id, memory_id, revision_id, action, payload_json, occurred_at
               FROM memory_events WHERE agent_id=?1
               ORDER BY occurred_at DESC, event_id DESC LIMIT 100
             ) ORDER BY occurred_at ASC, event_id ASC",
        )?;
        let events = statement
            .query_map([agent_id], |row| {
                let payload: Option<String> = row.get(5)?;
                Ok(MemoryEvent {
                    event_id: row.get(0)?,
                    agent_id: row.get(1)?,
                    memory_id: row.get(2)?,
                    revision_id: row.get(3)?,
                    action: row.get(4)?,
                    payload: payload
                        .and_then(|value| serde_json::from_str(&value).ok())
                        .unwrap_or(serde_json::Value::Null),
                    occurred_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(events)
    }

    /// Validate and commit a consolidator batch, its cursor, and its durable
    /// idempotency receipt in one transaction.
    pub fn commit_batch(
        &self,
        batch: MemoryCommitBatch,
    ) -> Result<MemoryCommitResult, MemoryError> {
        let agent_id = required("agent_id", &batch.agent_id)?;
        let key = required("idempotency_key", &batch.idempotency_key)?;
        let workspace = normalize_workspace(batch.workspace.as_deref());
        let request_hash = hash_text(&serde_json::to_string(&batch)?);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some((stored_hash, stored_result)) = transaction
            .query_row(
                "SELECT request_hash, result_json FROM memory_commits WHERE idempotency_key=?1",
                [&key],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if stored_hash != request_hash {
                return Err(MemoryError::Validation(
                    "idempotency key was already used for a different memory batch".into(),
                ));
            }
            let mut result: MemoryCommitResult = serde_json::from_str(&stored_result)?;
            result.replayed = true;
            transaction.commit()?;
            return Ok(result);
        }

        let mut memory_ids = Vec::new();
        for mutation in &batch.operations {
            match mutation {
                MemoryMutation::Save {
                    kind,
                    text,
                    evidence_excerpt,
                    sources,
                } => {
                    let text = normalize_text(text)?;
                    let evidence = normalize_evidence(evidence_excerpt)?;
                    let memory_id = Uuid::new_v4().to_string();
                    let revision_id = Uuid::new_v4().to_string();
                    let now = Utc::now().to_rfc3339();
                    transaction.execute(
                        "INSERT INTO memory_records
                         (revision_id, memory_id, revision, agent_id, workspace, kind, text,
                          evidence_excerpt, evidence_hash, status, created_at, updated_at, last_verified_at)
                         VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, 'active', ?9, ?9, ?9)",
                        params![revision_id, memory_id, agent_id, workspace, kind.as_str(), text,
                            evidence, hash_text(&evidence), now],
                    )?;
                    insert_sources(&transaction, &revision_id, sources)?;
                    insert_event(
                        &transaction,
                        &agent_id,
                        Some(&memory_id),
                        Some(&revision_id),
                        "saved",
                        Some(&serde_json::json!({
                            "text": text,
                            "evidence_excerpt": evidence,
                            "evidence_hash": hash_text(&evidence),
                            "kind": kind,
                            "workspace": workspace,
                            "sources": sources
                        })),
                    )?;
                    memory_ids.push(memory_id);
                }
                MemoryMutation::Update {
                    memory_id,
                    text,
                    evidence_excerpt,
                    sources,
                } => {
                    let prior = query_active(&transaction, memory_id)?
                        .ok_or_else(|| MemoryError::NotFound(memory_id.clone()))?;
                    if prior.agent_id != agent_id {
                        return Err(MemoryError::Validation(format!(
                            "memory {memory_id} belongs to another agent"
                        )));
                    }
                    let text = normalize_text(text)?;
                    let evidence = normalize_evidence(evidence_excerpt)?;
                    let revision_id = Uuid::new_v4().to_string();
                    let now = Utc::now().to_rfc3339();
                    transaction.execute(
                        "UPDATE memory_records SET status='superseded', replaced_by_revision_id=?1, updated_at=?2 WHERE revision_id=?3",
                        params![revision_id, now, prior.revision_id],
                    )?;
                    transaction.execute(
                        "INSERT INTO memory_records
                         (revision_id, memory_id, revision, agent_id, workspace, kind, text,
                          evidence_excerpt, evidence_hash, status, supersedes_revision_id,
                          created_at, updated_at, last_verified_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?11, ?11, ?11)",
                        params![
                            revision_id,
                            memory_id,
                            prior.revision + 1,
                            agent_id,
                            prior.workspace,
                            prior.kind.as_str(),
                            text,
                            evidence,
                            hash_text(&evidence),
                            prior.revision_id,
                            now
                        ],
                    )?;
                    insert_sources(&transaction, &revision_id, sources)?;
                    insert_event(
                        &transaction,
                        &agent_id,
                        Some(memory_id),
                        Some(&revision_id),
                        "updated",
                        Some(&serde_json::json!({
                            "text": text,
                            "evidence_excerpt": evidence,
                            "evidence_hash": hash_text(&evidence),
                            "kind": prior.kind,
                            "workspace": prior.workspace,
                            "sources": sources
                        })),
                    )?;
                    memory_ids.push(memory_id.clone());
                }
                MemoryMutation::Remove { memory_id } => {
                    let prior = query_active(&transaction, memory_id)?
                        .ok_or_else(|| MemoryError::NotFound(memory_id.clone()))?;
                    if prior.agent_id != agent_id {
                        return Err(MemoryError::Validation(format!(
                            "memory {memory_id} belongs to another agent"
                        )));
                    }
                    transaction.execute(
                        "UPDATE memory_records SET status='removed', updated_at=?1 WHERE revision_id=?2",
                        params![Utc::now().to_rfc3339(), prior.revision_id],
                    )?;
                    insert_event(
                        &transaction,
                        &agent_id,
                        Some(memory_id),
                        Some(&prior.revision_id),
                        "removed",
                        None,
                    )?;
                    memory_ids.push(memory_id.clone());
                }
            }
        }
        if let Some(cursor) = &batch.cursor {
            let cursor_key = required("cursor_key", &cursor.cursor_key)?;
            transaction.execute(
                "INSERT INTO memory_consolidation_cursors
                 (cursor_key, agent_id, workspace, conversation_id, sequence, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(cursor_key) DO UPDATE SET conversation_id=excluded.conversation_id,
                   sequence=excluded.sequence, updated_at=excluded.updated_at",
                params![
                    cursor_key,
                    agent_id,
                    workspace,
                    cursor.conversation_id,
                    cursor.sequence,
                    Utc::now().to_rfc3339()
                ],
            )?;
        }
        let result = MemoryCommitResult {
            idempotency_key: key.clone(),
            memory_ids,
            replayed: false,
        };
        transaction.execute(
            "INSERT INTO memory_commits(idempotency_key, request_hash, result_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                key,
                request_hash,
                serde_json::to_string(&result)?,
                Utc::now().to_rfc3339()
            ],
        )?;
        transaction.commit()?;
        Ok(result)
    }

    fn get_revision(&self, revision_id: &str) -> Result<MemoryRecord, MemoryError> {
        let connection = self.connection()?;
        let record = connection
            .query_row(
                "SELECT * FROM memory_records WHERE revision_id=?1",
                [revision_id],
                row_to_record,
            )
            .optional()?
            .ok_or_else(|| MemoryError::NotFound(revision_id.to_string()))?;
        self.attach_sources(record)
    }

    fn attach_sources(&self, mut record: MemoryRecord) -> Result<MemoryRecord, MemoryError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT source_type, locator, source_hash, is_primary FROM memory_sources
             WHERE revision_id=?1 ORDER BY is_primary DESC, id ASC",
        )?;
        record.sources = statement
            .query_map([&record.revision_id], |row| {
                Ok(MemorySource {
                    source_type: row.get(0)?,
                    locator: row.get(1)?,
                    source_hash: row.get(2)?,
                    primary: row.get::<_, i64>(3)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(record)
    }

    fn latest_injection(
        &self,
        agent_id: &str,
        provider: &str,
        process_key: &str,
    ) -> Result<Option<StoredInjection>, MemoryError> {
        let connection = self.connection()?;
        let stored = connection
            .query_row(
                "SELECT revision_ids_json FROM memory_injections
                 WHERE agent_id=?1 AND provider=?2 AND provider_process_key=?3
                 ORDER BY created_at DESC LIMIT 1",
                params![agent_id, provider, process_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        stored
            .map(|value| {
                Ok(StoredInjection {
                    revision_ids: serde_json::from_str(&value)?,
                })
            })
            .transpose()
    }

    fn records_by_revision_ids(
        &self,
        revision_ids: &[String],
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        revision_ids
            .iter()
            .map(|revision_id| self.get_revision(revision_id))
            .collect()
    }
}

fn migrate(connection: &Connection) -> Result<(), MemoryError> {
    connection.execute_batch(&format!(r#"
        CREATE TABLE IF NOT EXISTS memory_schema (version INTEGER NOT NULL);
        INSERT INTO memory_schema(version) SELECT {SCHEMA_VERSION}
          WHERE NOT EXISTS (SELECT 1 FROM memory_schema);
        CREATE TABLE IF NOT EXISTS memory_records (
          revision_id TEXT PRIMARY KEY, memory_id TEXT NOT NULL, revision INTEGER NOT NULL,
          agent_id TEXT NOT NULL, workspace TEXT, kind TEXT NOT NULL, text TEXT NOT NULL,
          evidence_excerpt TEXT NOT NULL, evidence_hash TEXT NOT NULL, status TEXT NOT NULL,
          supersedes_revision_id TEXT, replaced_by_revision_id TEXT,
          created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_verified_at TEXT NOT NULL,
          idempotency_key TEXT UNIQUE,
          UNIQUE(memory_id, revision)
        );
        CREATE INDEX IF NOT EXISTS idx_memory_active ON memory_records(agent_id, workspace, status, kind);
        CREATE TABLE IF NOT EXISTS memory_sources (
          id INTEGER PRIMARY KEY AUTOINCREMENT, revision_id TEXT NOT NULL,
          source_type TEXT NOT NULL, locator TEXT, source_hash TEXT, is_primary INTEGER NOT NULL DEFAULT 0,
          FOREIGN KEY(revision_id) REFERENCES memory_records(revision_id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS memory_events (
          event_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, memory_id TEXT, revision_id TEXT,
          action TEXT NOT NULL, payload_json TEXT, occurred_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_memory_events_agent ON memory_events(agent_id, occurred_at);
        CREATE TABLE IF NOT EXISTS memory_injections (
          injection_id TEXT PRIMARY KEY, agent_id TEXT NOT NULL, workspace TEXT, provider TEXT NOT NULL,
          provider_process_key TEXT NOT NULL, fingerprint TEXT NOT NULL, revision_ids_json TEXT NOT NULL,
          context_text TEXT NOT NULL, created_at TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_injection_unique
          ON memory_injections(agent_id, provider, provider_process_key, fingerprint);
        CREATE INDEX IF NOT EXISTS idx_memory_injection_process ON memory_injections(agent_id, provider, provider_process_key, created_at);
        CREATE TABLE IF NOT EXISTS memory_consolidation_cursors (
          cursor_key TEXT PRIMARY KEY, agent_id TEXT NOT NULL, workspace TEXT,
          conversation_id TEXT, sequence INTEGER NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS memory_commits (
          idempotency_key TEXT PRIMARY KEY, request_hash TEXT NOT NULL,
          result_json TEXT NOT NULL, created_at TEXT NOT NULL
        );
    "#))?;
    Ok(())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    let kind: String = row.get("kind")?;
    let status: String = row.get("status")?;
    Ok(MemoryRecord {
        revision_id: row.get("revision_id")?,
        memory_id: row.get("memory_id")?,
        revision: row.get("revision")?,
        agent_id: row.get("agent_id")?,
        workspace: row.get("workspace")?,
        kind: MemoryKind::parse(&kind).map_err(to_sql_conversion)?,
        text: row.get("text")?,
        evidence_excerpt: row.get("evidence_excerpt")?,
        evidence_hash: row.get("evidence_hash")?,
        status: MemoryStatus::parse(&status).map_err(to_sql_conversion)?,
        supersedes_revision_id: row.get("supersedes_revision_id")?,
        replaced_by_revision_id: row.get("replaced_by_revision_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_verified_at: row.get("last_verified_at")?,
        idempotency_key: row.get("idempotency_key")?,
        sources: Vec::new(),
    })
}

fn to_sql_conversion(error: MemoryError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn query_active(
    transaction: &Transaction<'_>,
    memory_id: &str,
) -> Result<Option<MemoryRecord>, MemoryError> {
    Ok(transaction.query_row(
        "SELECT * FROM memory_records WHERE memory_id=?1 AND status='active' ORDER BY revision DESC LIMIT 1",
        [memory_id], row_to_record,
    ).optional()?)
}

fn idempotent_record(
    transaction: &Transaction<'_>,
    key: Option<&str>,
) -> Result<Option<MemoryRecord>, MemoryError> {
    let Some(key) = key else {
        return Ok(None);
    };
    Ok(transaction
        .query_row(
            "SELECT * FROM memory_records WHERE idempotency_key=?1",
            [key],
            row_to_record,
        )
        .optional()?)
}

fn insert_sources(
    transaction: &Transaction<'_>,
    revision_id: &str,
    sources: &[MemorySource],
) -> Result<(), MemoryError> {
    for source in sources {
        let source_type = required("source_type", &source.source_type)?;
        transaction.execute(
            "INSERT INTO memory_sources(revision_id, source_type, locator, source_hash, is_primary)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                revision_id,
                source_type,
                normalize_optional(source.locator.as_deref()),
                normalize_optional(source.source_hash.as_deref()),
                i64::from(source.primary)
            ],
        )?;
    }
    Ok(())
}

fn insert_event(
    transaction: &Transaction<'_>,
    agent_id: &str,
    memory_id: Option<&str>,
    revision_id: Option<&str>,
    action: &str,
    payload: Option<&serde_json::Value>,
) -> Result<(), MemoryError> {
    transaction.execute(
        "INSERT INTO memory_events(event_id, agent_id, memory_id, revision_id, action, payload_json, occurred_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![Uuid::new_v4().to_string(), agent_id, memory_id, revision_id, action,
            payload.map(serde_json::to_string).transpose()?, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

fn required(field: &str, value: &str) -> Result<String, MemoryError> {
    let value = value.trim();
    if value.is_empty() {
        Err(MemoryError::Validation(format!("{field} is required")))
    } else {
        Ok(value.to_string())
    }
}

fn normalize_text(value: &str) -> Result<String, MemoryError> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    required("text", &value)
}

fn normalize_evidence(value: &str) -> Result<String, MemoryError> {
    let value = value.trim();
    required("evidence_excerpt", value)
}

pub fn normalize_workspace(value: Option<&str>) -> Option<String> {
    normalize_optional(value).map(|value| {
        let normalized = value.replace('\\', "/").trim_end_matches('/').to_string();
        if cfg!(windows) {
            normalized.to_ascii_lowercase()
        } else {
            normalized
        }
    })
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub fn hash_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn memory_fingerprint(records: &[MemoryRecord]) -> String {
    let canonical = records
        .iter()
        .map(|record| {
            let stale = record.kind == MemoryKind::Current
                && DateTime::parse_from_rfc3339(&record.last_verified_at)
                    .map(|value| {
                        value.with_timezone(&Utc) < Utc::now() - Duration::days(DEFAULT_STALE_DAYS)
                    })
                    .unwrap_or(true);
            format!(
                "{}:{}:{:?}:{}:{}:{}:{}:{}",
                record.memory_id,
                record.revision,
                record.kind,
                record.workspace.as_deref().unwrap_or("agent-wide"),
                record.evidence_hash,
                record.text,
                stale,
                MEMORY_BUDGET_POLICY_VERSION
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    hash_text(&canonical)
}

fn render_brief(
    kind: MemoryBriefKind,
    records: &[MemoryRecord],
    removed: &[String],
    max_chars: usize,
) -> (String, usize) {
    if records.is_empty() && removed.is_empty() {
        return (String::new(), 0);
    }
    let mut output = match kind {
        MemoryBriefKind::Fresh => "# Wardian memory\n".to_string(),
        MemoryBriefKind::ResumeDelta => "# Wardian memory changes\n".to_string(),
    };
    let mut omitted = 0;
    for (heading, target_kind) in [
        ("Stable memory", MemoryKind::Stable),
        ("Current state", MemoryKind::Current),
    ] {
        let entries = records
            .iter()
            .filter(|record| record.kind == target_kind)
            .collect::<Vec<_>>();
        if entries.is_empty() {
            continue;
        }
        let heading = format!("\n## {heading}\n");
        if output.len() + heading.len() <= max_chars {
            output.push_str(&heading);
        }
        for record in entries {
            let scope = record.workspace.as_deref().unwrap_or("agent-wide");
            let stale = record.kind == MemoryKind::Current
                && DateTime::parse_from_rfc3339(&record.last_verified_at)
                    .map(|value| {
                        value.with_timezone(&Utc) < Utc::now() - Duration::days(DEFAULT_STALE_DAYS)
                    })
                    .unwrap_or(true);
            let stale_label = if stale { " [stale]" } else { "" };
            let short_id = &record.memory_id[..record.memory_id.len().min(8)];
            let line = format!(
                "- [{short_id}]{} {} (scope: {scope}; verified: {})\n",
                stale_label, record.text, record.last_verified_at
            );
            if output.len() + line.len() > max_chars {
                omitted += 1;
            } else {
                output.push_str(&line);
            }
        }
    }
    if !removed.is_empty() {
        let heading = "\n## Removed or superseded\n";
        if output.len() + heading.len() <= max_chars {
            output.push_str(heading);
        }
        for memory_id in removed {
            let short_id = &memory_id[..memory_id.len().min(8)];
            let line = format!("- [{short_id}] no longer applies\n");
            if output.len() + line.len() > max_chars {
                omitted += 1;
            } else {
                output.push_str(&line);
            }
        }
    }
    if omitted > 0 {
        let notice = format!("\n_{omitted} additional memories omitted by the startup budget._\n");
        if output.len() + notice.len() <= max_chars {
            output.push_str(&notice);
        }
    }
    (output.trim().to_string(), omitted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(agent: &str, workspace: Option<&str>, text: &str) -> SaveMemoryRequest {
        SaveMemoryRequest {
            agent_id: agent.into(),
            workspace: workspace.map(str::to_string),
            kind: MemoryKind::Stable,
            text: text.into(),
            evidence_excerpt: format!("Evidence for {text}"),
            sources: vec![],
            idempotency_key: None,
        }
    }

    #[test]
    fn lifecycle_preserves_revision_history_and_sources() {
        let temp = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(temp.path().join("memory.db")).unwrap();
        let mut create = request("agent-a", Some("C:\\Work"), "Prefer concise answers");
        create.sources.push(MemorySource {
            source_type: "conversation".into(),
            locator: Some("conv-1#4".into()),
            source_hash: None,
            primary: true,
        });
        let first = store.save(create).unwrap();
        let expected_workspace = if cfg!(windows) { "c:/work" } else { "C:/Work" };
        assert_eq!(first.workspace.as_deref(), Some(expected_workspace));
        assert_eq!(first.sources.len(), 1);
        let second = store
            .update(UpdateMemoryRequest {
                memory_id: first.memory_id.clone(),
                text: "Prefer concise technical answers".into(),
                evidence_excerpt: "User clarified the preference".into(),
                sources: vec![],
                idempotency_key: None,
            })
            .unwrap();
        assert_eq!(second.revision, 2);
        let history = store.history(&first.memory_id).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].status, MemoryStatus::Superseded);
        assert_eq!(
            store.remove(&first.memory_id).unwrap().status,
            MemoryStatus::Removed
        );
        assert!(store
            .list_active("agent-a", Some("C:/Work"))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn recall_isolates_agents_and_workspaces() {
        let temp = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(temp.path().join("memory.db")).unwrap();
        store
            .save(request("agent-a", None, "Agent preference"))
            .unwrap();
        store
            .save(request("agent-a", Some("one"), "Project one"))
            .unwrap();
        store
            .save(request("agent-a", Some("two"), "Project two"))
            .unwrap();
        store
            .save(request("agent-b", Some("one"), "Other agent"))
            .unwrap();
        let recall = store.recall("agent-a", Some("one")).unwrap();
        let texts = recall
            .stable
            .into_iter()
            .map(|entry| entry.record.text)
            .collect::<Vec<_>>();
        assert_eq!(texts.len(), 2);
        assert!(texts.contains(&"Agent preference".to_string()));
        assert!(texts.contains(&"Project one".to_string()));
    }

    #[test]
    fn idempotency_returns_the_original_revision() {
        let temp = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(temp.path().join("memory.db")).unwrap();
        let mut first = request("agent-a", None, "Remember this");
        first.idempotency_key = Some("run-1:cursor-3".into());
        let original = store.save(first.clone()).unwrap();
        let retry = store.save(first.clone()).unwrap();
        assert_eq!(retry.revision_id, original.revision_id);
        assert_eq!(store.history(&original.memory_id).unwrap().len(), 1);
        first.text = "Different retry payload".into();
        assert!(matches!(store.save(first), Err(MemoryError::Validation(_))));
    }

    #[test]
    fn fresh_brief_and_resume_delta_are_fingerprinted() {
        let temp = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(temp.path().join("memory.db")).unwrap();
        let first = store
            .save(request("agent-a", Some("one"), "Use the compact layout"))
            .unwrap();
        let fresh = store
            .compile_brief("agent-a", Some("one"), "codex", "session-1", false, 8_000)
            .unwrap();
        assert!(fresh.context_text.contains("Stable memory"));
        assert!(fresh.context_text.contains("Use the compact layout"));
        store
            .record_injection("agent-a", Some("one"), "codex", "session-1", &fresh)
            .unwrap();
        let unchanged = store
            .compile_brief("agent-a", Some("one"), "codex", "session-1", true, 8_000)
            .unwrap();
        assert_eq!(unchanged.kind, MemoryBriefKind::ResumeDelta);
        assert!(unchanged.context_text.contains("Use the compact layout"));

        store
            .update(UpdateMemoryRequest {
                memory_id: first.memory_id,
                text: "Use the extra compact layout".into(),
                evidence_excerpt: "Preference was refined".into(),
                sources: vec![],
                idempotency_key: None,
            })
            .unwrap();
        let delta = store
            .compile_brief("agent-a", Some("one"), "codex", "session-1", true, 8_000)
            .unwrap();
        assert_eq!(delta.kind, MemoryBriefKind::ResumeDelta);
        assert!(delta.context_text.contains("extra compact"));
    }

    #[test]
    fn consolidation_batch_is_atomic_and_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(temp.path().join("memory.db")).unwrap();
        let batch = MemoryCommitBatch {
            agent_id: "agent-a".into(),
            workspace: Some("one".into()),
            idempotency_key: "run-1:conv-1:8".into(),
            operations: vec![MemoryMutation::Save {
                kind: MemoryKind::Stable,
                text: "Use metric units".into(),
                evidence_excerpt: "The user explicitly selected metric units.".into(),
                sources: vec![MemorySource {
                    source_type: "conversation".into(),
                    locator: Some("conv-1#8".into()),
                    source_hash: None,
                    primary: true,
                }],
            }],
            cursor: Some(MemoryCursorUpdate {
                cursor_key: "agent-a:one".into(),
                conversation_id: Some("conv-1".into()),
                sequence: 8,
            }),
        };
        let first = store.commit_batch(batch.clone()).unwrap();
        let replay = store.commit_batch(batch).unwrap();
        assert!(!first.replayed);
        assert!(replay.replayed);
        assert_eq!(first.memory_ids, replay.memory_ids);
        assert_eq!(store.list_active("agent-a", Some("one")).unwrap().len(), 1);
    }

    #[test]
    fn recall_labels_stale_current_and_excludes_invalid_evidence() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.db");
        let store = MemoryStore::open(&path).unwrap();
        let mut current = request("agent-a", Some("one"), "Release is awaiting validation");
        current.kind = MemoryKind::Current;
        let stale = store.save(current).unwrap();
        let invalid = store
            .save(request("agent-a", Some("one"), "Corrupted evidence"))
            .unwrap();
        let connection = Connection::open(path).unwrap();
        connection
            .execute(
                "UPDATE memory_records SET last_verified_at=?1 WHERE revision_id=?2",
                params![
                    (Utc::now() - Duration::days(DEFAULT_STALE_DAYS + 1)).to_rfc3339(),
                    stale.revision_id
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE memory_records SET evidence_hash='invalid' WHERE revision_id=?1",
                [&invalid.revision_id],
            )
            .unwrap();

        let recall = store.recall("agent-a", Some("one")).unwrap();
        assert!(recall.stable.is_empty());
        assert_eq!(recall.current.len(), 1);
        assert!(recall.current[0].stale);
        assert_eq!(recall.current[0].record.memory_id, stale.memory_id);
    }

    #[test]
    fn bounded_brief_is_deterministic_and_reports_omissions() {
        let temp = tempfile::tempdir().unwrap();
        let store = MemoryStore::open(temp.path().join("memory.db")).unwrap();
        for index in 0..8 {
            store
                .save(request(
                    "agent-a",
                    Some("one"),
                    &format!("Preference {index}: {}", "x".repeat(90)),
                ))
                .unwrap();
        }

        let first = store
            .compile_brief("agent-a", Some("one"), "codex", "fresh-a", false, 360)
            .unwrap();
        let second = store
            .compile_brief("agent-a", Some("one"), "codex", "fresh-b", false, 360)
            .unwrap();
        assert_eq!(first.context_text, second.context_text);
        assert_eq!(first.fingerprint, second.fingerprint);
        assert!(first.omitted_count > 0);
        assert!(first.context_text.len() <= 360);
    }

    #[test]
    fn invalid_batch_rolls_back_operations_and_cursor() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.db");
        let store = MemoryStore::open(&path).unwrap();
        let batch = MemoryCommitBatch {
            agent_id: "agent-a".into(),
            workspace: Some("one".into()),
            idempotency_key: "bad-batch".into(),
            operations: vec![
                MemoryMutation::Save {
                    kind: MemoryKind::Stable,
                    text: "Valid first operation".into(),
                    evidence_excerpt: "Valid evidence".into(),
                    sources: vec![],
                },
                MemoryMutation::Update {
                    memory_id: "missing-memory".into(),
                    text: "Cannot update".into(),
                    evidence_excerpt: "No source record".into(),
                    sources: vec![],
                },
            ],
            cursor: Some(MemoryCursorUpdate {
                cursor_key: "agent-a:one".into(),
                conversation_id: Some("conv-1".into()),
                sequence: 3,
            }),
        };

        assert!(matches!(
            store.commit_batch(batch),
            Err(MemoryError::NotFound(_))
        ));
        assert!(store
            .list_active("agent-a", Some("one"))
            .unwrap()
            .is_empty());
        let connection = Connection::open(path).unwrap();
        let cursors: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM memory_consolidation_cursors",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cursors, 0);
    }
}
