import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { readVerificationPlan, selectPlan } from '../scripts/verify-ci.mjs';

const workflow = readFileSync('.github/workflows/ci.yml', 'utf8');

describe('CI verification contract', () => {
  it('derives the exact core CI commands in workflow order', () => {
    expect(readVerificationPlan(workflow).map(({ command }) => command)).toEqual([
      'npm run typecheck',
      'npm run lint',
      'npm run test',
      'npm run build',
      'npm run check:workbench-cutover',
      'npm run check:test-reachability',
      'npm run check:deadcode',
      'npm run check:budgets',
      'npm run check:page-fixtures',
      'cargo clippy --workspace -- -D warnings',
      'cargo fmt --all -- --check',
      'cargo test --workspace -- --test-threads=1',
      'cargo check --workspace',
      'npm run docs:check-llms',
      'npm run docs:build',
    ]);
  });

  it('supports a category subset without changing command literals', () => {
    expect(selectPlan(readVerificationPlan(workflow), 'backend').map(({ command }) => command)).toEqual([
      'cargo clippy --workspace -- -D warnings',
      'cargo fmt --all -- --check',
      'cargo test --workspace -- --test-threads=1',
      'cargo check --workspace',
    ]);
  });
});
