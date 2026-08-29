import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const WORKFLOW_PATH = resolve('.github/workflows/ci.yml');
const CATEGORIES = new Set(['frontend', 'backend', 'docs']);

/**
 * Read the local-verification contract directly from the CI workflow.
 * Only single-line run steps are eligible; this keeps the command boundary
 * literal and avoids trying to interpret shell blocks as YAML or JavaScript.
 */
export function readVerificationPlan(workflowText) {
  const lines = workflowText.split(/\r?\n/);
  const plan = [];
  for (let index = 0; index < lines.length; index += 1) {
    const marker = lines[index].match(/^\s*#\s*local-verify:\s*(\S+)\s*$/);
    if (!marker) continue;

    const runLine = lines[index + 1]?.match(/^\s*run:\s*(\S.*)\s*$/);
    if (!runLine || runLine[1] === '|' || runLine[1] === '>-' || runLine[1] === '>') {
      throw new Error(`local-verify marker at workflow line ${index + 1} must precede a single-line run step`);
    }
    const category = marker[1];
    if (!CATEGORIES.has(category)) throw new Error(`unknown local-verify category: ${category}`);
    plan.push({ category, command: runLine[1].trim(), workflowLine: index + 2 });
    index += 1;
  }
  if (plan.length === 0) throw new Error('CI workflow contains no local-verify steps');
  return plan;
}

export function parseArgs(argv) {
  let only = null;
  let list = false;
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === '--list') {
      list = true;
    } else if (argument === '--only') {
      only = argv[++index];
    } else if (argument.startsWith('--only=')) {
      only = argument.slice('--only='.length);
    } else {
      throw new Error(`unknown argument: ${argument}`);
    }
  }
  if (only && !CATEGORIES.has(only)) throw new Error(`--only must be one of: ${[...CATEGORIES].join(', ')}`);
  return { list, only };
}

export function selectPlan(plan, only) {
  return only ? plan.filter((step) => step.category === only) : plan;
}

function execute(command) {
  const result = spawnSync(command, { cwd: process.cwd(), shell: true, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    console.error(`FAILED: ${command}`);
    process.exitCode = result.status ?? 1;
    return false;
  }
  return true;
}

export function main(argv = process.argv.slice(2)) {
  const { list, only } = parseArgs(argv);
  const plan = selectPlan(readVerificationPlan(readFileSync(WORKFLOW_PATH, 'utf8')), only);
  if (plan.length === 0) throw new Error(`no local-verify steps selected for ${only}`);
  if (list) {
    for (const step of plan) console.log(`${step.category}\t${step.command}`);
    return 0;
  }
  for (const step of plan) {
    console.log(`\n>>> ${step.command}`);
    if (!execute(step.command)) return process.exitCode;
  }
  return 0;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
