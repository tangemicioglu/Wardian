import { describe, expect, it } from 'vitest';
import { nodeStatusesAt } from './replay';
import type { Blueprint } from '../builder/blueprintTypes';
import type { RunEvent } from './runTypes';

const events: RunEvent[] = [
  { seq: 0, ts: 't0', kind: 'run_started', blueprint_id: 'wf', schema: 2, trigger: {} },
  { seq: 1, ts: 't1', kind: 'node_started', node: 'a' },
  { seq: 2, ts: 't2', kind: 'node_completed', node: 'a', output: { ok: true } },
  { seq: 3, ts: 't3', kind: 'node_started', node: 'b' },
  { seq: 4, ts: 't4', kind: 'node_failed', node: 'b', error: 'boom' },
];

describe('nodeStatusesAt', () => {
  it('is all-pending before any node event (index 0 = after run_started)', () => {
    const m = nodeStatusesAt(events, 0);
    expect(m.a).toBeUndefined();
  });

  it('reflects running then completed for a as the index advances', () => {
    expect(nodeStatusesAt(events, 1).a).toBe('running');
    expect(nodeStatusesAt(events, 2).a).toBe('completed');
  });

  it('marks b failed at the last index', () => {
    const m = nodeStatusesAt(events, 4);
    expect(m.a).toBe('completed');
    expect(m.b).toBe('failed');
  });

  it('folds decision completion into the replayed node status', () => {
    const decisionEvents: RunEvent[] = [
      { seq: 0, ts: 't0', kind: 'node_started', node: 'choose' },
      {
        seq: 1,
        ts: 't1',
        kind: 'decision_completed',
        node: 'choose',
        output: { chosen: 'yes' },
        port: 'yes',
      },
    ];

    expect(nodeStatusesAt(decisionEvents, 1).choose).toBe('completed');
  });

  it('folds branch, decision, and approval control transitions', () => {
    const controlEvents: RunEvent[] = [
      { seq: 0, ts: 't0', kind: 'branch_taken', node: 'branch', port: 'on_true' },
      { seq: 1, ts: 't1', kind: 'decision_made', node: 'decision', port: 'yes' },
      { seq: 2, ts: 't2', kind: 'approval_granted', node: 'approved', actor: 'user' },
      { seq: 3, ts: 't3', kind: 'approval_rejected', node: 'rejected', actor: 'user' },
    ];

    expect(nodeStatusesAt(controlEvents, 3)).toEqual({
      branch: 'completed',
      decision: 'completed',
      approved: 'completed',
      rejected: 'failed',
    });
  });

  it('resets loop body statuses when replay reaches a new iteration', () => {
    const blueprint: Blueprint = {
      schema: 2,
      id: 'loop',
      name: 'Loop',
      nodes: [
        { id: 'repeat', type: 'loop' },
        { id: 'body', type: 'task', parent: 'repeat' },
      ],
      edges: [],
    };
    const loopEvents: RunEvent[] = [
      { seq: 0, ts: 't0', kind: 'loop_iteration', node: 'repeat', iteration: 0 },
      { seq: 1, ts: 't1', kind: 'node_completed', node: 'body', output: { ok: true } },
      { seq: 2, ts: 't2', kind: 'loop_iteration', node: 'repeat', iteration: 1 },
      { seq: 3, ts: 't3', kind: 'loop_completed', node: 'repeat' },
    ];

    expect(nodeStatusesAt(loopEvents, 2, blueprint)).toEqual({ repeat: 'running', body: 'pending' });
    expect(nodeStatusesAt(loopEvents, 3, blueprint)).toEqual({ repeat: 'completed', body: 'pending' });
  });
});
