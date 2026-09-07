# QA real-provider baseline plan

## Scope and revision

- System: Wardian native provider runtime and real-provider E2E harness.
- Baseline: `39d996c896de2e5a4883ff325984889c8513482e` on
  `test/provider-conformance-matrix`.
- Providers: Claude, Codex, OpenCode, Antigravity, and Pi. Gemini is inventoried
  separately as unmaintained; its real-provider cells are not silently counted
  as passes. Antigravity usage logging remains an intentional skip.
- Authority: QA owns baseline execution, assertion additions, evidence, and
  retest; Coder owns product fixes. Do not stop the host release app, attach to
  live agents, alter the staged main checkout, or merge/request GitHub review.

## Preconditions and isolation

1. Confirm the isolated worktree, revision, clean source status, and current
   provider CLI availability/version. Preserve pre-existing `.task` artifacts.
2. Use an explicitly created target directory under this worktree and an
   explicitly created app home under the OS temporary directory. Set
   `WARDIAN_E2E_NATIVE_HOME` and `CARGO_TARGET_DIR` for every native build/run;
   never rely on production `<wardian-home>` or the host release app.
3. Use `<absolute-workspace-path>` as the provider target directory. It must be
   a dedicated empty/test directory, not a live agent workspace. Codex receives
   the harness non-Git workspace when the existing headless test requires it.
4. Build the debug app and CLI from this worktree only:

   POSIX:

   ```sh
   mkdir -p .tmp/provider-conformance-target
   export CARGO_TARGET_DIR="$PWD/.tmp/provider-conformance-target"
   export WARDIAN_E2E_NATIVE_HOME="$(mktemp -d)"
   npm run tauri -- build --debug --no-bundle
   cargo build -p wardian-cli --bin wardian-cli
   ```

   PowerShell:

   ```powershell
   $targetRoot = (Join-Path (Get-Location) '.tmp/provider-conformance-target')
   New-Item -ItemType Directory -Force -Path $targetRoot | Out-Null
   $tempRoot = [System.IO.Path]::GetFullPath((Join-Path ([System.IO.Path]::GetTempPath()) ('wardian-provider-conformance-' + [guid]::NewGuid())))
   New-Item -ItemType Directory -Path $tempRoot | Out-Null
   $env:CARGO_TARGET_DIR = $targetRoot
   $env:WARDIAN_E2E_NATIVE_HOME = $tempRoot
   npm run tauri -- build --debug --no-bundle
   cargo build -p wardian-cli --bin wardian-cli
   ```

   If a harness command rebuilds internally, pass the same variables. Use
   `WARDIAN_NATIVE_SKIP_BUILD=1` only after verifying the isolated binary was
   built from the locked revision.

## Model and version discovery

Record command output before choosing models. Do not infer price from model
names or from Wardian defaults. For each provider, capture CLI version, live
catalog source, model ID, account/access result, and the basis for the
cheapest usable selection in `.task/provider-models.json` (or a provider
specific JSON file when an existing collector provides that format).

- Claude: `claude --version`; use the authenticated Claude model/catalog
  command exposed by `claude --help`, then verify the selected model with a
  minimal no-tool prompt. Prefer the lowest-cost model that is available to
  this account and can complete the required cases.
- Codex: `codex --version`; use the current interactive model picker/catalog
  or the supported model-list command exposed by `codex --help`; verify a
  selected model with a minimal prompt. Record effort separately from model.
- OpenCode: `opencode --version`; capture the live model list from the
  documented `opencode models` command (or the current equivalent shown by
  `opencode --help`), excluding local-only models unless they are the only
  usable account-backed option. Prefer an available `*-free` remote model
  only after the live catalog and a minimal prompt confirm usability.
- Antigravity: `agy --version`; inspect the current model/account selection
  command from `agy --help` and record the selected usable model. Usage/cost
  logging is an intentional skip, not an untested pass.
- Pi: `pi --version`; inspect the current model/catalog command from `pi
  --help`; record provider/model and any configured effort. A model routed to
  a local backend is usable only if the local backend is explicitly available
  and its cost basis is recorded.
- Gemini: `gemini --version` and `gemini --help` only for inventory. Record
  `unmaintained` with the repository reason; do not run or count Gemini as a
  maintained-provider pass.

## Existing real-provider harness execution

Run each harness slice with the same isolated home/target/workspace and retain
stdout, stderr, exit status, provider version/model/build, case name, and
artifact paths. Use the existing tests without weakening their skips:

```sh
WARDIAN_E2E_REAL_DELIVERY=1 \
WARDIAN_E2E_DELIVERY_PROVIDERS=codex,claude,opencode,antigravity,pi \
WARDIAN_E2E_DELIVERY_CASES=all \
WARDIAN_E2E_REAL_FRESH_TRANSCRIPT=1 \
WARDIAN_E2E_REAL_WORKSPACE=<absolute-workspace-path> \
npm run test:e2e:native:fast -- e2e-native/tests/provider-delivery-real-native.test.mjs

WARDIAN_E2E_REAL_RENDERING=1 \
WARDIAN_E2E_RENDERING_PROVIDERS=codex,claude,opencode,antigravity,pi \
WARDIAN_E2E_REAL_WORKSPACE=<absolute-workspace-path> \
WARDIAN_E2E_RENDERING_EXPECT_RESPONSE_TEXT=<marker> \
npm run test:e2e:native:fast -- e2e-native/tests/real-provider-rendering-native.test.mjs

WARDIAN_E2E_REAL_HEADLESS_PROVIDERS=1 \
WARDIAN_E2E_HEADLESS_PROVIDERS=codex,claude,opencode,antigravity,pi \
WARDIAN_E2E_REAL_WORKSPACE=<absolute-workspace-path> \
npm run test:e2e:native:fast -- e2e-native/tests/provider-headless-automation-real-native.test.mjs
```

PowerShell uses one assignment per line before each command:

```powershell
$env:WARDIAN_E2E_REAL_DELIVERY = '1'
$env:WARDIAN_E2E_DELIVERY_PROVIDERS = 'codex,claude,opencode,antigravity,pi'
$env:WARDIAN_E2E_DELIVERY_CASES = 'all'
$env:WARDIAN_E2E_REAL_FRESH_TRANSCRIPT = '1'
$env:WARDIAN_E2E_REAL_WORKSPACE = '<absolute-workspace-path>'
npm run test:e2e:native:fast -- e2e-native/tests/provider-delivery-real-native.test.mjs
```

Repeat the equivalent setup for rendering and headless variables. If the fast
runner cannot prove it is using the isolated build, rebuild first and record
the binary hash. Run providers one at a time after a full-matrix attempt when
needed to minimize failures and provider-side cost.

## Assertion inventory and additions

Before editing tests, map every requested function to an actual assertion in
delivery, rendering, or headless evidence. Functions include launch/readiness;
short, multiline, trailing-newline, and long input; completion/status; session
ID and live chat-log link; transcript refresh; actual user provenance;
assistant deduplication; tools; archive replay; pause/resume; fresh/clear;
usage/cost; instructions/skills; approvals; headless automation; and native
delivery. A cell is `pass` only when its exact assertion ran. Use
`untested`, `blocked`, and `intentional_skip` as distinct statuses.

Add only missing conformance assertions required to observe the stated
contract, especially chat-log link updates, actual user provenance, archive
replay, and assistant deduplication. Keep real-provider prompts bounded and
non-mutating; do not add product behavior or provider-specific assumptions.
Keep Antigravity usage/cost as `intentional_skip` and Gemini as `untested` or
`blocked` with an explicit unmaintained reason.

## Evidence schema and stopping rules

Append machine-readable records to `.task/provider-conformance-results.jsonl`
with at least:

```json
{
  "provider": "codex",
  "function": "chat_log_link",
  "status": "pass|fail|blocked|untested|intentional_skip",
  "model": "<catalog-selected-id>",
  "provider_version": "<version>",
  "wardian_revision": "39d996c8...",
  "build": "<debug-build/hash>",
  "mode": "interactive|headless|rendering",
  "case": "<case-name>",
  "evidence": "<sanitized artifact or assertion reference>",
  "failure": null
}
```

Sanitize all evidence before publishing: no user names, home paths, tokens,
credentials, or live-agent identifiers. Preserve raw local artifacts only in
the explicitly isolated temporary evidence root. Stop and report a confirmed
provider/product defect as soon as it is reproduced and minimized, then
continue independent providers. Stop only when all five providers have a
baseline record for every applicable harness function, all failures are
classified, and any necessary assertions have been rerun. Do not publish a
readiness claim while required cells remain untested or blocked.

## Handoff

Run focused tests for any assertion changes, run the applicable local CI
subset, inspect `git status`, and request no GitHub review. Report new defects
to Architect early with expected/actual behavior and replay commands; Coder
owns fixes. Before final handoff, include baseline and retest records,
provider/model/version/build basis, external blockers, intentional skips,
untested cells, and exact evidence artifact locations.
