import React from 'react';
import type {
  AutomationListener,
  FileChangeKind,
  ListenerFingerprintSource,
  ListenerOverlapPolicy,
  ListenerTrigger,
} from '../../types/automation';

const TRIGGER_TYPES = [
  { value: 'file_watch', label: 'File change' },
  { value: 'webhook', label: 'Inbound webhook' },
  { value: 'web_poll', label: 'Web poll' },
] as const;

const CHANGE_KINDS: { value: FileChangeKind; label: string }[] = [
  { value: 'created', label: 'Created' },
  { value: 'modified', label: 'Modified' },
  { value: 'removed', label: 'Removed' },
];

const FINGERPRINTS: { value: ListenerFingerprintSource; label: string; help: string }[] = [
  {
    value: 'etag_or_last_modified',
    label: 'ETag or Last-Modified',
    help: 'Cheapest, and correct for most servers.',
  },
  { value: 'body_hash', label: 'Whole response', help: 'Fires on any change to the body.' },
  {
    value: 'json_pointer',
    label: 'JSON field',
    help: 'Point at the field that changes, e.g. /0/tag_name for a releases feed.',
  },
  { value: 'regex', label: 'Pattern match', help: 'The first capture group is the watched value.' },
];

const OVERLAP_POLICIES: { value: ListenerOverlapPolicy; label: string; help: string }[] = [
  { value: 'skip', label: 'Skip', help: 'Drop the new event while a run is active.' },
  { value: 'coalesce', label: 'Coalesce', help: 'Keep only the latest pending event.' },
  { value: 'parallel', label: 'Parallel', help: 'Start a run for every event.' },
];

const inputClass =
  'w-full bg-[var(--color-wardian-input-bg)] border border-wardian-border rounded-lg px-3 py-1.5 text-[11px] text-[var(--color-wardian-text)] outline-none focus:border-[var(--color-wardian-accent)]/50 transition-colors';
const selectClass = `${inputClass} cursor-pointer`;
const labelClass = 'text-[11px] font-bold text-muted-neutral';
const helpClass = 'text-[10px] text-muted';

/** A blank trigger of each kind, used when the user switches type. */
export function defaultTrigger(type: ListenerTrigger['type']): ListenerTrigger {
  switch (type) {
    case 'file_watch':
      return {
        type: 'file_watch',
        path: '',
        recursive: true,
        patterns: [],
        ignore: [],
        events: [],
        debounce_ms: 500,
      };
    case 'webhook':
      return {
        type: 'webhook',
        path_segment: '',
        auth: 'hmac_sha256',
        signature_header: null,
        max_body_bytes: 262144,
      };
    case 'web_poll':
      return {
        type: 'web_poll',
        url: '',
        interval_seconds: 300,
        method: 'get',
        headers: {},
        fingerprint: 'etag_or_last_modified',
        json_pointer: null,
        regex: null,
        max_body_bytes: 1048576,
      };
  }
}

function parseList(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);
}

interface ListenerEditorProps {
  value: AutomationListener;
  onChange: (value: AutomationListener) => void;
  compact?: boolean;
}

export const ListenerEditor: React.FC<ListenerEditorProps> = ({ value, onChange, compact }) => {
  const trigger = value.trigger;
  const overlapId = React.useId();

  const update = (patch: Partial<AutomationListener>) => onChange({ ...value, ...patch });
  const updateTrigger = (patch: Partial<ListenerTrigger>) =>
    update({ trigger: { ...trigger, ...patch } as ListenerTrigger });

  return (
    <div className={`space-y-3 ${compact ? '' : 'p-3'}`}>
      <div className="space-y-1">
        <label className={labelClass} htmlFor={`${overlapId}-name`}>
          Name
        </label>
        <input
          id={`${overlapId}-name`}
          className={inputClass}
          value={value.name}
          onChange={(event) => update({ name: event.target.value })}
          placeholder="Source audit"
        />
      </div>

      <div className="space-y-1">
        <label className={labelClass} htmlFor={`${overlapId}-type`}>
          Trigger
        </label>
        <select
          id={`${overlapId}-type`}
          className={selectClass}
          value={trigger.type}
          onChange={(event) =>
            update({ trigger: defaultTrigger(event.target.value as ListenerTrigger['type']) })
          }
        >
          {TRIGGER_TYPES.map((type) => (
            <option key={type.value} value={type.value}>
              {type.label}
            </option>
          ))}
        </select>
        {trigger.type !== 'web_poll' ? (
          <p className={helpClass}>
            Events that arrive while Wardian is closed are missed. Only a web poll detects a change
            that happened during downtime.
          </p>
        ) : null}
      </div>

      {trigger.type === 'file_watch' ? (
        <>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-path`}>
              Watch path
            </label>
            <input
              id={`${overlapId}-path`}
              className={inputClass}
              value={trigger.path}
              onChange={(event) => updateTrigger({ path: event.target.value })}
              placeholder="Absolute path to a folder or file"
            />
            <p className={helpClass}>
              A path inside the Wardian home is refused: automation runs write there, so the
              listener would trigger itself.
            </p>
          </div>
          <label className="flex cursor-pointer items-center gap-2 text-[11px] text-muted">
            <input
              type="checkbox"
              checked={trigger.recursive}
              onChange={(event) => updateTrigger({ recursive: event.target.checked })}
            />
            Watch subfolders
          </label>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-patterns`}>
              Match patterns
            </label>
            <input
              id={`${overlapId}-patterns`}
              className={inputClass}
              value={trigger.patterns.join(', ')}
              onChange={(event) => updateTrigger({ patterns: parseList(event.target.value) })}
              placeholder="**/*.rs, **/*.toml (blank matches everything)"
            />
          </div>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-ignore`}>
              Also ignore
            </label>
            <input
              id={`${overlapId}-ignore`}
              className={inputClass}
              value={trigger.ignore.join(', ')}
              onChange={(event) => updateTrigger({ ignore: parseList(event.target.value) })}
              placeholder="**/generated/**"
            />
            <p className={helpClass}>
              .git, node_modules, target, dist, and build are always ignored.
            </p>
          </div>
          <div className="space-y-1">
            <span className={labelClass}>React to</span>
            <div className="flex gap-3">
              {CHANGE_KINDS.map((kind) => (
                <label key={kind.value} className="flex cursor-pointer items-center gap-1.5 text-[11px] text-muted">
                  <input
                    type="checkbox"
                    checked={trigger.events.length === 0 || trigger.events.includes(kind.value)}
                    onChange={(event) => {
                      const selected = new Set(
                        trigger.events.length === 0
                          ? CHANGE_KINDS.map((entry) => entry.value)
                          : trigger.events,
                      );
                      if (event.target.checked) selected.add(kind.value);
                      else selected.delete(kind.value);
                      updateTrigger({ events: Array.from(selected) });
                    }}
                  />
                  {kind.label}
                </label>
              ))}
            </div>
          </div>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-debounce`}>
              Quiet period (ms)
            </label>
            <input
              id={`${overlapId}-debounce`}
              type="number"
              min={1}
              className={inputClass}
              value={trigger.debounce_ms}
              onChange={(event) =>
                updateTrigger({ debounce_ms: Number(event.target.value) || 500 })
              }
            />
            <p className={helpClass}>One save touches several files; this collapses them into one run.</p>
          </div>
        </>
      ) : null}

      {trigger.type === 'webhook' ? (
        <>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-segment`}>
              URL path
            </label>
            <input
              id={`${overlapId}-segment`}
              className={inputClass}
              value={trigger.path_segment}
              onChange={(event) => updateTrigger({ path_segment: event.target.value })}
              placeholder="github-releases"
            />
            <p className={helpClass}>
              Reachable at /hooks/&lt;path&gt; on loopback. Use a tunnel to receive deliveries from
              outside this machine.
            </p>
          </div>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-auth`}>
              Authentication
            </label>
            <select
              id={`${overlapId}-auth`}
              className={selectClass}
              value={trigger.auth}
              onChange={(event) =>
                updateTrigger({ auth: event.target.value as 'token' | 'hmac_sha256' })
              }
            >
              <option value="hmac_sha256">HMAC-SHA256 signature</option>
              <option value="token">Shared token</option>
            </select>
          </div>
          {trigger.auth === 'hmac_sha256' ? (
            <div className="space-y-1">
              <label className={labelClass} htmlFor={`${overlapId}-sigheader`}>
                Signature header
              </label>
              <input
                id={`${overlapId}-sigheader`}
                className={inputClass}
                value={trigger.signature_header ?? ''}
                onChange={(event) =>
                  updateTrigger({ signature_header: event.target.value || null })
                }
                placeholder="X-Hub-Signature-256"
              />
            </div>
          ) : null}
        </>
      ) : null}

      {trigger.type === 'web_poll' ? (
        <>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-url`}>
              URL
            </label>
            <input
              id={`${overlapId}-url`}
              className={inputClass}
              value={trigger.url}
              onChange={(event) => updateTrigger({ url: event.target.value })}
              placeholder="https://api.github.com/repos/owner/repo/releases"
            />
          </div>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-interval`}>
              Check every (seconds)
            </label>
            <input
              id={`${overlapId}-interval`}
              type="number"
              min={30}
              className={inputClass}
              value={trigger.interval_seconds}
              onChange={(event) =>
                updateTrigger({ interval_seconds: Number(event.target.value) || 300 })
              }
            />
            <p className={helpClass}>Minimum 30 seconds.</p>
          </div>
          <div className="space-y-1">
            <label className={labelClass} htmlFor={`${overlapId}-fingerprint`}>
              Fires when this changes
            </label>
            <select
              id={`${overlapId}-fingerprint`}
              className={selectClass}
              value={trigger.fingerprint}
              onChange={(event) =>
                updateTrigger({ fingerprint: event.target.value as ListenerFingerprintSource })
              }
            >
              {FINGERPRINTS.map((entry) => (
                <option key={entry.value} value={entry.value}>
                  {entry.label}
                </option>
              ))}
            </select>
            <p className={helpClass}>
              {FINGERPRINTS.find((entry) => entry.value === trigger.fingerprint)?.help}
            </p>
          </div>
          {trigger.fingerprint === 'json_pointer' ? (
            <div className="space-y-1">
              <label className={labelClass} htmlFor={`${overlapId}-pointer`}>
                JSON pointer
              </label>
              <input
                id={`${overlapId}-pointer`}
                className={inputClass}
                value={trigger.json_pointer ?? ''}
                onChange={(event) => updateTrigger({ json_pointer: event.target.value || null })}
                placeholder="/0/tag_name"
              />
            </div>
          ) : null}
          {trigger.fingerprint === 'regex' ? (
            <div className="space-y-1">
              <label className={labelClass} htmlFor={`${overlapId}-regex`}>
                Pattern
              </label>
              <input
                id={`${overlapId}-regex`}
                className={inputClass}
                value={trigger.regex ?? ''}
                onChange={(event) => updateTrigger({ regex: event.target.value || null })}
                placeholder="version\\s+([0-9.]+)"
              />
            </div>
          ) : null}
        </>
      ) : null}

      <div className="space-y-1">
        <label className={labelClass} htmlFor={`${overlapId}-overlap`}>
          While a run is active
        </label>
        <select
          id={`${overlapId}-overlap`}
          className={selectClass}
          value={value.overlap ?? (trigger.type === 'webhook' ? 'parallel' : 'skip')}
          onChange={(event) => update({ overlap: event.target.value as ListenerOverlapPolicy })}
        >
          {OVERLAP_POLICIES.map((policy) => (
            <option key={policy.value} value={policy.value}>
              {policy.label}
            </option>
          ))}
        </select>
        <p className={helpClass}>
          {
            OVERLAP_POLICIES.find(
              (policy) =>
                policy.value === (value.overlap ?? (trigger.type === 'webhook' ? 'parallel' : 'skip')),
            )?.help
          }
        </p>
      </div>
    </div>
  );
};
