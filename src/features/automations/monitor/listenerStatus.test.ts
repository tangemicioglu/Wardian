import { describe, expect, it } from 'vitest';
import type { AutomationListener, ListenerRuntime, ListenerTrigger } from '../../../types/automation';
import {
  lastFireLabel,
  listenerKindLabel,
  listenerProblemLabel,
  listenerSourceLabel,
  listenerStatusColor,
  listenerStatusLabel,
  survivesDowntime,
} from './listenerStatus';

const emptyRuntime: ListenerRuntime = {
  armed: true,
  fire_count: 0,
  recent_fire_epoch_ms: [],
  consecutive_failures: 0,
};

function listener(
  trigger: ListenerTrigger,
  runtime: Partial<ListenerRuntime> = {},
  enabled = true,
): AutomationListener {
  return {
    id: 'l1',
    blueprint_id: 'audit',
    name: 'Audit',
    enabled,
    trigger,
    input: {},
    bindings: {},
    runtime: { ...emptyRuntime, ...runtime },
  };
}

const fileTrigger: ListenerTrigger = {
  type: 'file_watch',
  path: '/work/repo',
  recursive: true,
  patterns: ['**/*.rs'],
  ignore: [],
  events: [],
  debounce_ms: 500,
};

const pollTrigger: ListenerTrigger = {
  type: 'web_poll',
  url: 'https://example.invalid/releases',
  interval_seconds: 600,
  method: 'get',
  headers: {},
  fingerprint: 'json_pointer',
  json_pointer: '/0/tag_name',
  regex: null,
  max_body_bytes: 1048576,
};

const webhookTrigger: ListenerTrigger = {
  type: 'webhook',
  path_segment: 'ci',
  auth: 'hmac_sha256',
  signature_header: null,
  max_body_bytes: 262144,
};

describe('listener status', () => {
  it('distinguishes an auto-disabled listener from one the user turned off', () => {
    const off = listener(fileTrigger, {}, false);
    const autoDisabled = listener(fileTrigger, { disabled_reason: 'rate ceiling tripped' });

    expect(listenerStatusLabel(off)).toBe('Off');
    expect(listenerStatusLabel(autoDisabled)).toBe('Auto-disabled');
    // The user's switch is untouched, which is what makes the two states
    // distinguishable at all.
    expect(autoDisabled.enabled).toBe(true);
    expect(listenerStatusColor(autoDisabled)).toBe('var(--color-wardian-error)');
    expect(listenerStatusColor(off)).toBe('var(--color-wardian-text-muted)');
  });

  it('reports an arming failure rather than showing a healthy listener', () => {
    const failed = listener(fileTrigger, { armed: false, arm_error: 'watch path is not accessible' });
    expect(listenerStatusLabel(failed)).toBe('Error');
    expect(listenerProblemLabel(failed)).toBe('watch path is not accessible');
  });

  it('shows an enabled but not-yet-armed listener as arming', () => {
    expect(listenerStatusLabel(listener(fileTrigger, { armed: false }))).toBe('Arming');
  });

  it('reads as listening once armed with nothing to report', () => {
    const healthy = listener(fileTrigger);
    expect(listenerStatusLabel(healthy)).toBe('Listening');
    expect(listenerProblemLabel(healthy)).toBeNull();
  });

  it('surfaces the most recent refusal so a silent listener is diagnosable', () => {
    const rejected = listener(webhookTrigger, {
      last_rejection: { reason: 'delivery credential did not match', at_epoch_ms: 1000 },
    });
    expect(listenerProblemLabel(rejected)).toBe('delivery credential did not match');
  });

  it('pairs a repeated poll failure with its streak count', () => {
    const flaky = listener(pollTrigger, {
      consecutive_failures: 4,
      last_rejection: { reason: 'request failed', at_epoch_ms: 1000 },
    });
    expect(listenerProblemLabel(flaky)).toBe('request failed (4 in a row)');
  });

  it('describes each trigger kind and what it watches', () => {
    expect(listenerKindLabel(fileTrigger)).toBe('File');
    expect(listenerSourceLabel(fileTrigger)).toBe('/work/repo (recursive) **/*.rs');

    expect(listenerKindLabel(webhookTrigger)).toBe('Webhook');
    expect(listenerSourceLabel(webhookTrigger)).toBe('/hooks/ci');

    expect(listenerKindLabel(pollTrigger)).toBe('Poll');
    expect(listenerSourceLabel(pollTrigger)).toContain('every 10m');
  });

  it('marks only the poll listener as surviving application downtime', () => {
    expect(survivesDowntime(pollTrigger)).toBe(true);
    expect(survivesDowntime(fileTrigger)).toBe(false);
    expect(survivesDowntime(webhookTrigger)).toBe(false);
  });

  it('says so plainly when a listener has never fired', () => {
    expect(lastFireLabel(listener(fileTrigger))).toBe('never fired');
    expect(lastFireLabel(listener(fileTrigger, { last_fire_epoch_ms: 1_700_000_000_000 }))).not.toBe(
      'never fired',
    );
  });
});
