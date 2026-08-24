# Offline Prompt Delivery

## Decision

Ordinary prompts are submitted immediately to the shared delivery controller.
The caller does not wait for the agent status to become ready before queuing or
delivering the prompt.

When the target agent is offline or errored, an ordinary message starts the
existing headless provider transport. When the target is busy, the message is
placed in the mailbox for the next safe interactive turn. Provider slash
commands remain mailbox-only while offline because they require an interactive
provider command channel.

## Entry points

Desktop Chat and remote ordinary prompts use the same `QueueIfBusy` delivery
policy and preserve the input mode. The Chat composer is disabled only while a
submission is in flight; agent status does not prevent a prompt from being
submitted.

## Verification

The control-plane regression test proves that an offline ordinary prompt
reaches `headless_process` and reports `provider_applied`. The Chat regression
test proves that an offline agent's composer remains enabled and submits the
prompt.
