import { describe, expect, it } from 'vitest';
import { assertLintPolicyUnchanged, changedLintPolicyFiles, compareMetrics, packageContract } from '../scripts/verify-budgets.mjs';

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

  describe('package.json contract comparison', () => {
    const base = JSON.stringify({
      name: 'wardian',
      scripts: { test: 'vitest run' },
      devDependencies: { eslint: '^9.0.0' },
    });

    it('allows a manifest change that only adds a script', () => {
      const head = JSON.stringify({
        name: 'wardian',
        scripts: { test: 'vitest run', 'site:media': 'node scripts/capture-site-media.mjs' },
        devDependencies: { eslint: '^9.0.0' },
      });
      expect(changedLintPolicyFiles(['package.json'], { base, head })).toEqual([]);
      expect(() => assertLintPolicyUnchanged(['package.json'], { base, head })).not.toThrow();
    });

    it('still rejects a dependency change', () => {
      const head = JSON.stringify({
        name: 'wardian',
        scripts: { test: 'vitest run' },
        devDependencies: { eslint: '^8.0.0' },
      });
      expect(changedLintPolicyFiles(['package.json'], { base, head })).toEqual(['package.json']);
    });

    it('still rejects an embedded eslint configuration change', () => {
      const head = JSON.stringify({
        name: 'wardian',
        scripts: { test: 'vitest run' },
        devDependencies: { eslint: '^9.0.0' },
        eslintConfig: { rules: { 'no-console': 'off' } },
      });
      expect(changedLintPolicyFiles(['package.json'], { base, head })).toEqual(['package.json']);
    });

    it('rejects when a manifest cannot be parsed, rather than allowing it', () => {
      expect(changedLintPolicyFiles(['package.json'], { base, head: '{ not json' })).toEqual(['package.json']);
    });

    it('rejects when manifest contents were not supplied', () => {
      expect(changedLintPolicyFiles(['package.json'])).toEqual(['package.json']);
    });

    it('reduces a manifest to its contract fields only', () => {
      expect(packageContract(base)).toBe(
        packageContract(JSON.stringify({ scripts: { anything: 'else' }, devDependencies: { eslint: '^9.0.0' } })),
      );
    });
  });
});
