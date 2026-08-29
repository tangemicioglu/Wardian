import { describe, expect, it } from 'vitest';
import { assertLintPolicyUnchanged, changedLintPolicyFiles, compareMetrics } from '../scripts/verify-budgets.mjs';

describe('base-relative debt budget comparison', () => {
  it('reports only metrics that increased over the base', () => {
    expect(compareMetrics(
      { file_lines: { 'a.rs': 12 }, ignored_rust_tests: 4, eslint_warnings: 1 },
      { file_lines: { 'a.rs': 10 }, ignored_rust_tests: 4, eslint_warnings: 2 },
    )).toEqual({
      over: [['a.rs lines', 12, 10]],
      under: [['eslint_warnings', 1, 2]],
    });
  });

  it('allows equal metrics', () => {
    expect(compareMetrics(
      { file_lines: { 'a.rs': 10 }, ignored_rust_tests: 4 },
      { file_lines: { 'a.rs': 10 }, ignored_rust_tests: 4 },
    )).toEqual({ over: [], under: [] });
  });

  it('rejects a head lint policy that could hide newly added warnings', () => {
    expect(changedLintPolicyFiles(['src/new-warning.ts', 'eslint.config.js'])).toEqual(['eslint.config.js']);
    expect(() => assertLintPolicyUnchanged(['src/new-warning.ts', 'eslint.config.js']))
      .toThrow('changes lint policy or dependencies');
  });
});
