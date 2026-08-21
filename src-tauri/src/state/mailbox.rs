pub use wardian_core::control::{
    MailboxDeliveryPhase, MailboxMessageDraft, MailboxMessageRecord, MailboxMessageStatus,
};

const MAX_TERMINAL_RECORDS_PER_TARGET: usize = 64;

#[derive(Debug, Default)]
pub struct MailboxState {
    records: Vec<MailboxMessageRecord>,
    last_millis: i64,
    counter: u64,
}

impl MailboxState {
    pub fn enqueue(&mut self, draft: MailboxMessageDraft) -> MailboxMessageRecord {
        let created_at = now_rfc3339_millis();
        let id = self.next_message_id();
        let record = MailboxMessageRecord {
            id,
            interaction_id: draft.interaction_id,
            target_session_id: draft.target_session_id,
            body: draft.body,
            input_mode: draft.input_mode,
            queue_policy: draft.queue_policy,
            approval_action: draft.approval_action,
            origin: draft.origin,
            created_at,
            ready_after: None,
            status: MailboxMessageStatus::Pending,
            phase: MailboxDeliveryPhase::Queued,
        };
        self.records.push(record.clone());
        record
    }

    /// Restores queued records from the durable interaction store. Terminal
    /// records are not stored there, so this only rehydrates work that may
    /// still need delivery or recovery.
    pub fn hydrate(&mut self, records: Vec<MailboxMessageRecord>) {
        self.records = records;
        self.records.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.id.cmp(&right.id))
        });
        self.last_millis = 0;
        self.counter = 0;
        let record_ids = self
            .records
            .iter()
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        for id in record_ids {
            self.observe_message_id(&id);
        }
    }

    pub fn all(&self) -> Vec<MailboxMessageRecord> {
        self.records.clone()
    }

    pub fn list_for_target(&self, target_session_id: &str) -> Vec<MailboxMessageRecord> {
        self.records
            .iter()
            .filter(|record| record.target_session_id == target_session_id)
            .cloned()
            .collect()
    }

    pub fn set_ready_after(
        &mut self,
        id: &str,
        ready_after: Option<String>,
    ) -> Option<MailboxMessageRecord> {
        let record = self.records.iter_mut().find(|record| record.id == id)?;
        record.ready_after = ready_after;
        Some(record.clone())
    }

    pub fn next_pending_for_target(&self, target_session_id: &str) -> Option<MailboxMessageRecord> {
        self.records
            .iter()
            .find(|record| {
                record.target_session_id == target_session_id
                    && record.status == MailboxMessageStatus::Pending
            })
            .cloned()
    }

    pub fn take_next_pending_for_target(
        &mut self,
        target_session_id: &str,
    ) -> Option<MailboxMessageRecord> {
        let record = self.records.iter_mut().find(|record| {
            record.target_session_id == target_session_id
                && record.status == MailboxMessageStatus::Pending
        })?;
        record.status = MailboxMessageStatus::InFlight;
        record.phase = MailboxDeliveryPhase::Dispatching;
        Some(record.clone())
    }

    pub fn mark_delivered(&mut self, id: &str) -> Option<MailboxMessageRecord> {
        self.mark_terminal(id, MailboxMessageStatus::Delivered)
    }

    pub fn mark_failed(&mut self, id: &str) -> Option<MailboxMessageRecord> {
        self.mark_terminal(id, MailboxMessageStatus::Failed)
    }

    pub fn mark_pending(&mut self, id: &str) -> Option<MailboxMessageRecord> {
        let record = self.records.iter_mut().find(|record| record.id == id)?;
        record.status = MailboxMessageStatus::Pending;
        record.phase = MailboxDeliveryPhase::Queued;
        Some(record.clone())
    }

    pub fn remove(&mut self, id: &str) -> Option<MailboxMessageRecord> {
        let index = self.records.iter().position(|record| record.id == id)?;
        Some(self.records.remove(index))
    }

    pub fn remove_for_target(&mut self, target_session_id: &str) -> usize {
        let original_len = self.records.len();
        self.records
            .retain(|record| record.target_session_id != target_session_id);
        original_len - self.records.len()
    }

    fn mark_terminal(
        &mut self,
        id: &str,
        status: MailboxMessageStatus,
    ) -> Option<MailboxMessageRecord> {
        let updated = {
            let record = self.records.iter_mut().find(|record| record.id == id)?;
            record.status = status;
            record.phase = MailboxDeliveryPhase::Terminal;
            record.clone()
        };
        self.compact_terminal_records_for_target(&updated.target_session_id);
        Some(updated)
    }

    fn compact_terminal_records_for_target(&mut self, target_session_id: &str) {
        let terminal_count = self
            .records
            .iter()
            .filter(|record| {
                record.target_session_id == target_session_id && record.status.is_terminal()
            })
            .count();
        let mut remove_count = terminal_count.saturating_sub(MAX_TERMINAL_RECORDS_PER_TARGET);
        if remove_count == 0 {
            return;
        }

        self.records.retain(|record| {
            if remove_count > 0
                && record.target_session_id == target_session_id
                && record.status.is_terminal()
            {
                remove_count -= 1;
                false
            } else {
                true
            }
        });
    }

    fn next_message_id(&mut self) -> String {
        let now_millis = chrono::Utc::now().timestamp_millis();
        let millis = now_millis.max(self.last_millis);
        if millis == self.last_millis {
            self.counter = self.counter.saturating_add(1);
        } else {
            self.last_millis = millis;
            self.counter = 0;
        }

        format!("msg_{millis:013}_{:06}", self.counter)
    }

    fn observe_message_id(&mut self, id: &str) {
        let Some((millis, counter)) = id
            .strip_prefix("msg_")
            .and_then(|suffix| suffix.split_once('_'))
            .and_then(|(millis, counter)| {
                Some((millis.parse::<i64>().ok()?, counter.parse::<u64>().ok()?))
            })
        else {
            return;
        };
        if millis > self.last_millis {
            self.last_millis = millis;
            self.counter = counter;
        } else if millis == self.last_millis {
            self.counter = self.counter.max(counter);
        }
    }
}

fn now_rfc3339_millis() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardian_core::control::{ApprovalAction, MessageInputMode, MessageOrigin, QueuePolicy};

    fn message_for(target_session_id: &str, body: &str) -> MailboxMessageDraft {
        MailboxMessageDraft {
            interaction_id: format!("int_{target_session_id}_{body}"),
            target_session_id: target_session_id.to_string(),
            body: body.to_string(),
            input_mode: MessageInputMode::Message,
            queue_policy: QueuePolicy::QueueIfBusy,
            approval_action: None,
            origin: None,
        }
    }

    #[test]
    fn enqueueing_message_records_pending_mailbox_entry() {
        let mut mailbox = MailboxState::default();

        let record = mailbox.enqueue(message_for("agent-1", "hello"));

        assert_eq!(record.target_session_id, "agent-1");
        assert_eq!(record.body, "hello");
        assert_eq!(record.status, MailboxMessageStatus::Pending);
        assert_eq!(record.phase, MailboxDeliveryPhase::Queued);
        assert!(record.id.starts_with("msg_"));
        assert_eq!(mailbox.all().len(), 1);
    }

    #[test]
    fn queued_message_preserves_interaction_id() {
        let mut mailbox = MailboxState::default();

        let record = mailbox.enqueue(message_for("agent-1", "hello"));

        assert_eq!(record.interaction_id, "int_agent-1_hello");
    }

    #[test]
    fn message_ids_are_monotonic_with_stable_shape() {
        let mut mailbox = MailboxState::default();

        let first = mailbox.enqueue(message_for("agent-1", "one"));
        let second = mailbox.enqueue(message_for("agent-1", "two"));

        assert!(first.id.starts_with("msg_"));
        assert!(second.id.starts_with("msg_"));
        assert_ne!(first.id, second.id);
        assert!(
            first.id < second.id,
            "ids should sort in enqueue order: {} then {}",
            first.id,
            second.id
        );
    }

    #[test]
    fn listing_can_filter_by_target_session_id() {
        let mut mailbox = MailboxState::default();
        mailbox.enqueue(message_for("agent-1", "one"));
        mailbox.enqueue(message_for("agent-2", "two"));
        mailbox.enqueue(message_for("agent-1", "three"));

        let agent_one = mailbox.list_for_target("agent-1");

        assert_eq!(agent_one.len(), 2);
        assert!(agent_one
            .iter()
            .all(|record| record.target_session_id == "agent-1"));
        assert_eq!(agent_one[0].body, "one");
        assert_eq!(agent_one[1].body, "three");
    }

    #[test]
    fn enqueue_preserves_approval_action_and_origin_metadata() {
        let mut mailbox = MailboxState::default();
        let origin = MessageOrigin::WardianAgent {
            session_id: "source-agent".to_string(),
        };
        let approval_action = ApprovalAction::Select {
            option: "allow_once".to_string(),
        };

        let record = mailbox.enqueue(MailboxMessageDraft {
            interaction_id: "int_approval".to_string(),
            target_session_id: "agent-1".to_string(),
            body: "approve".to_string(),
            input_mode: MessageInputMode::ApprovalAction,
            queue_policy: QueuePolicy::MailboxOnly,
            approval_action: Some(approval_action.clone()),
            origin: Some(origin.clone()),
        });

        assert_eq!(record.input_mode, MessageInputMode::ApprovalAction);
        assert_eq!(record.queue_policy, QueuePolicy::MailboxOnly);
        assert_eq!(record.approval_action, Some(approval_action));
        assert_eq!(record.origin, Some(origin));
    }

    #[test]
    fn taking_next_pending_marks_only_first_target_message_in_flight() {
        let mut mailbox = MailboxState::default();
        let first = mailbox.enqueue(message_for("agent-1", "one"));
        let second = mailbox.enqueue(message_for("agent-1", "two"));
        mailbox.enqueue(message_for("agent-2", "other"));

        let taken = mailbox.take_next_pending_for_target("agent-1").unwrap();

        assert_eq!(taken.id, first.id);
        assert_eq!(taken.status, MailboxMessageStatus::InFlight);
        assert_eq!(taken.phase, MailboxDeliveryPhase::Dispatching);
        let agent_one = mailbox.list_for_target("agent-1");
        assert_eq!(agent_one[0].status, MailboxMessageStatus::InFlight);
        assert_eq!(agent_one[1].id, second.id);
        assert_eq!(agent_one[1].status, MailboxMessageStatus::Pending);
    }

    #[test]
    fn terminal_markers_preserve_records_and_update_phase() {
        let mut mailbox = MailboxState::default();
        let delivered = mailbox.enqueue(message_for("agent-1", "one"));
        let failed = mailbox.enqueue(message_for("agent-1", "two"));

        let delivered = mailbox.mark_delivered(&delivered.id).unwrap();
        let failed = mailbox.mark_failed(&failed.id).unwrap();

        assert_eq!(delivered.status, MailboxMessageStatus::Delivered);
        assert_eq!(delivered.phase, MailboxDeliveryPhase::Terminal);
        assert_eq!(failed.status, MailboxMessageStatus::Failed);
        assert_eq!(failed.phase, MailboxDeliveryPhase::Terminal);
        assert_eq!(mailbox.all().len(), 2);
    }

    #[test]
    fn terminal_compaction_keeps_only_recent_terminal_records_per_target() {
        let mut mailbox = MailboxState::default();
        let first = mailbox.enqueue(message_for("agent-1", "first"));
        mailbox.mark_delivered(&first.id).unwrap();

        for index in 0..70 {
            let record = mailbox.enqueue(message_for("agent-1", &format!("message-{index}")));
            mailbox.mark_delivered(&record.id).unwrap();
        }

        let records = mailbox.list_for_target("agent-1");
        assert_eq!(records.len(), 64);
        assert!(records.iter().all(|record| record.body != "first"));
    }

    #[test]
    fn mark_pending_requeues_in_flight_message_for_retry() {
        let mut mailbox = MailboxState::default();
        let record = mailbox.enqueue(message_for("agent-1", "one"));
        mailbox.take_next_pending_for_target("agent-1").unwrap();

        let requeued = mailbox.mark_pending(&record.id).unwrap();

        assert_eq!(requeued.status, MailboxMessageStatus::Pending);
        assert_eq!(requeued.phase, MailboxDeliveryPhase::Queued);
        let pending = mailbox.take_next_pending_for_target("agent-1").unwrap();
        assert_eq!(pending.id, record.id);
    }

    #[test]
    fn hydrate_preserves_pending_records_and_advances_the_id_allocator() {
        let mut mailbox = MailboxState::default();
        let record = MailboxMessageRecord {
            id: "msg_9999999999999_000003".to_string(),
            interaction_id: "int_restored".to_string(),
            target_session_id: "agent-1".to_string(),
            body: "restored work".to_string(),
            input_mode: MessageInputMode::Message,
            queue_policy: QueuePolicy::QueueIfBusy,
            approval_action: None,
            origin: None,
            created_at: "2026-08-01T00:00:00.000Z".to_string(),
            ready_after: None,
            status: MailboxMessageStatus::Pending,
            phase: MailboxDeliveryPhase::Queued,
        };

        mailbox.hydrate(vec![record.clone()]);
        let next = mailbox.enqueue(message_for("agent-1", "new work"));

        assert_eq!(mailbox.all()[0], record);
        assert_eq!(next.id, "msg_9999999999999_000004");
    }
}
