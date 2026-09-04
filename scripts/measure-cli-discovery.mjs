// Repeatable, offline discovery workloads. Optional token counts use tiktoken.
// Usage: node scripts/measure-cli-discovery.mjs <binary> <output.json>
import { spawnSync } from 'node:child_process';
import { readFileSync, mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
import { dirname, resolve, join } from 'node:path';
import { tmpdir } from 'node:os';
import assert from 'node:assert/strict';

const [binary, output] = process.argv.slice(2);
assert(binary && output, 'pass a CLI binary and report path');
const home = mkdtempSync(join(tmpdir(), 'wardian-discovery-'));
const env = { ...process.env, WARDIAN_HOME: home, NO_COLOR: '1' };
delete env.WARDIAN_SESSION_ID;
const run = (args) => {
  const result = spawnSync(resolve(binary), args, { env, encoding: 'utf8' });
  if (result.error) throw result.error;
  return result;
};
const schemaAvailable = run(['schema']).status === 0;
const workloads = {
  'root-help': ['--help'],
  'agent-list-help': ['agent', 'list', '--help'],
  'full-node-registry': ['automation', 'node-types', '--json'],
  'discover-task-node': schemaAvailable
    ? ['automation', 'node-types', 'task', '--json']
    : ['automation', 'node-types', '--json'],
};
const texts = {};
const commands = {};
for (const [name, args] of Object.entries(workloads)) {
  const samples = Array.from({ length: 3 }, () => run(args));
  for (const result of samples) {
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, samples[0].stdout, 'discovery must be deterministic');
  }
  texts[name] = samples[0].stdout.replaceAll('\r\n', '\n');
  commands[name] = args;
}
texts.skill = readFileSync('src-tauri/resources/library/skills/wardian-skills/wardian-cli/SKILL.md', 'utf8').replaceAll('\r\n', '\n');
const tokenResult = spawnSync('python', ['-c',
  'import sys,json,tiktoken; e=tiktoken.get_encoding("o200k_base"); print(json.dumps({k:len(e.encode(v,disallowed_special=())) for k,v in json.load(sys.stdin).items()}))',
], { input: JSON.stringify(texts), encoding: 'utf8' });
const tokens = tokenResult.status === 0 ? JSON.parse(tokenResult.stdout) : {};
const report = {
  metric: 'UTF-8 bytes; o200k_base tokens when tiktoken is installed',
  samples: 3,
  normalization: 'CRLF to LF; no live app, fresh isolated home, no managed session',
  workloads: Object.fromEntries(Object.entries(texts).map(([name, text]) => [name, {
    command: commands[name], bytes: Buffer.byteLength(text), tokens: tokens[name],
  }])),
};
mkdirSync(dirname(resolve(output)), { recursive: true });
writeFileSync(output, JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify(report, null, 2));
