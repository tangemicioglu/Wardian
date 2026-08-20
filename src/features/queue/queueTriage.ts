import type { QueueItem } from "../../types";

export function providerChoiceAcknowledgementUnresolved(item: QueueItem) {
  return Boolean(item.provider_choice_pending || (item.provider_choice_sent && !item.read));
}

export function providerChoiceRecorded(item: QueueItem) {
  return Boolean(item.provider_choice_pending || item.provider_choice_sent);
}

export function isClearableLegacyCompletion(item: QueueItem) {
  return !item.inbox_notification_id
    && !item.workflow_approval
    && (item.type === "agent_completed" || item.type === "workflow_completed");
}
