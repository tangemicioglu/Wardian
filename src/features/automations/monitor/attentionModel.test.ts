import { describe, expect, it } from 'vitest';
import { automationAttention } from './attentionModel';

describe('automationAttention', () => {
  it('keeps approval runs and unsuperseded failures in attention', () => {
    const result = automationAttention([
      { run_id: 'approval', blueprint_id: 'release', status: 'awaiting_approval', updated_at: '2026-08-31T12:00:00Z' },
      { run_id: 'failed', blueprint_id: 'audit', status: 'failed', updated_at: '2026-08-31T11:00:00Z' },
    ], []);

    expect([...result.runIds]).toEqual(['approval', 'failed']);
  });

  it('lets a newer completed or active run supersede an older failure', () => {
    const result = automationAttention([
      { run_id: 'failed', blueprint_id: 'audit', status: 'failed', updated_at: '2026-08-31T10:00:00Z' },
      { run_id: 'completed', blueprint_id: 'audit', status: 'completed', updated_at: '2026-08-31T11:00:00Z' },
      { run_id: 'older-failure', blueprint_id: 'release', status: 'failed', updated_at: '2026-08-31T10:00:00Z' },
      { run_id: 'running', blueprint_id: 'release', status: 'running', updated_at: '2026-08-31T12:00:00Z' },
    ], []);

    expect(result.runIds.size).toBe(0);
  });

  it('retains schedule launch failures until a newer schedule run appears', () => {
    const failedAt = Date.parse('2026-08-31T11:00:00Z');
    const result = automationAttention([
      { run_id: 'new-run', blueprint_id: 'audit', schedule_id: 'recovered', status: 'running', updated_at: '2026-08-31T12:00:00Z' },
    ], [
      { id: 'unrecovered', last_run_status: 'failed', last_run_epoch_ms: failedAt },
      { id: 'recovered', last_run_status: 'failed', last_run_epoch_ms: failedAt },
      { id: 'paused', last_run_status: 'completed', last_run_epoch_ms: failedAt },
    ]);

    expect([...result.scheduleIds]).toEqual(['unrecovered']);
  });
});
