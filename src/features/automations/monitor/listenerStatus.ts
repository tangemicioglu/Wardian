import type { AutomationListener, ListenerTrigger } from '../../../types/automation';
import { automationRunStatusColor } from '../run/statusLabels';

/** Short label for the kind of event a listener waits on. */
export function listenerKindLabel(trigger: ListenerTrigger): string {
  switch (trigger.type) {
    case 'file_watch':
      return 'File';
    case 'webhook':
      return 'Webhook';
    case 'web_poll':
      return 'Poll';
    default:
      return 'Listener';
  }
}

/** One-line description of what the listener is watching. */
export function listenerSourceLabel(trigger: ListenerTrigger): string {
  switch (trigger.type) {
    case 'file_watch': {
      const scope = trigger.recursive ? ' (recursive)' : '';
      const patterns = trigger.patterns.length > 0 ? ` ${trigger.patterns.join(', ')}` : '';
      return `${trigger.path}${scope}${patterns}`;
    }
    case 'webhook':
      return `/hooks/${trigger.path_segment}`;
    case 'web_poll': {
      const every =
        trigger.interval_seconds % 60 === 0
          ? `every ${trigger.interval_seconds / 60}m`
          : `every ${trigger.interval_seconds}s`;
      return `${trigger.url} ${every}`;
    }
    default:
      return '';
  }
}

/**
 * Operational status.
 *
 * `enabled` is the user's switch and `disabled_reason` is the app's, so a
 * listener the rate ceiling stopped reads as "Auto-disabled" rather than
 * silently as "Off" — the difference is the whole point of keeping the two
 * fields apart.
 */
export function listenerStatusLabel(listener: AutomationListener): string {
  if (listener.runtime.disabled_reason) return 'Auto-disabled';
  if (!listener.enabled) return 'Off';
  if (listener.runtime.arm_error) return 'Error';
  if (!listener.runtime.armed) return 'Arming';
  if (listener.runtime.last_run_status === 'running') return 'Running';
  if (listener.runtime.last_run_status === 'failed') return 'Failed';
  return 'Listening';
}

/** Status color (semantic theme var) matching {@link listenerStatusLabel}. */
export function listenerStatusColor(listener: AutomationListener): string {
  if (listener.runtime.disabled_reason) return 'var(--color-wardian-error)';
  if (!listener.enabled) return 'var(--color-wardian-text-muted)';
  if (listener.runtime.arm_error) return 'var(--color-wardian-error)';
  if (!listener.runtime.armed) return 'var(--color-wardian-warning)';
  if (listener.runtime.last_run_status) {
    return automationRunStatusColor(listener.runtime.last_run_status);
  }
  return 'var(--color-wardian-success)';
}

/** Last-fire wall-clock label, or a marker for a listener that never fired. */
export function lastFireLabel(listener: AutomationListener): string {
  if (!listener.runtime.last_fire_epoch_ms) return 'never fired';
  return new Date(listener.runtime.last_fire_epoch_ms).toLocaleString();
}

/**
 * The single most useful thing to say about a listener that is not working.
 *
 * A listener that quietly does nothing is indistinguishable from one that is
 * working and has nothing to report, so the reason a fire was refused ranks
 * above liveness here.
 */
export function listenerProblemLabel(listener: AutomationListener): string | null {
  if (listener.runtime.disabled_reason) return listener.runtime.disabled_reason;
  if (listener.runtime.arm_error) return listener.runtime.arm_error;
  if (listener.runtime.last_run_error) return listener.runtime.last_run_error;
  if (listener.runtime.consecutive_failures > 0 && listener.runtime.last_rejection) {
    return `${listener.runtime.last_rejection.reason} (${listener.runtime.consecutive_failures} in a row)`;
  }
  if (listener.runtime.last_rejection) return listener.runtime.last_rejection.reason;
  return null;
}

/**
 * Whether the app is downtime-blind for this listener.
 *
 * Poll fingerprints are durable, so a poll listener detects a change that
 * happened while Wardian was closed. `notify` has no journal and an unbound
 * port cannot receive, so the other two simply miss those events.
 */
export function survivesDowntime(trigger: ListenerTrigger): boolean {
  return trigger.type === 'web_poll';
}
