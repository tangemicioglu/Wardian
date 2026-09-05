# Listeners

A listener is an invoker that watches something and starts a durable automation
run when it changes. Like a schedule, a listener is separate from the automation
definition and is not a node in the blueprint.

Three kinds of listener share one configuration surface:

| Listener | Fires when | Use it for |
| --- | --- | --- |
| **File change** | a matching file under a watched path is created, modified, or removed | reacting to edits in a repository or an inbox folder |
| **Inbound webhook** | an authenticated HTTP request arrives at `/hooks/<path>` | a system you control pushing to you: your CI, your repository, a payment provider |
| **Web poll** | a watched URL's response changes | a system you do **not** control: a release feed, a status page, a remote file |

## Choosing between a webhook and a poll

These answer different questions, and picking the wrong one is the most common
way to end up with a listener that never fires.

An **inbound webhook** requires the other side to be configured to notify you.
Only a repository administrator can add a webhook to a repository, and the
endpoint has to be reachable from the internet. If you do not administer the
source, this is not available to you.

A **web poll** asks the question yourself. Watching for a new release of a
project you merely use is a poll, not a webhook. So is watching a remote file,
a status page, or an API field.

A poll is also the only listener that recovers from downtime — see
[While Wardian is closed](#while-wardian-is-closed).

## Creating a Listener

From the app, open the automation monitor and choose **New listener**. From the
CLI:

```bash
# Watch a source tree
wardian automation listener watch \
  --blueprint code-review \
  --name "Source audit" \
  --path <absolute-workspace-path> \
  --recursive \
  --pattern '**/*.rs' \
  --enable

# Watch for a new release of a project you do not administer
wardian automation listener poll \
  --blueprint release-notes \
  --name "Upstream releases" \
  --url https://api.github.com/repos/<owner>/<repo>/releases \
  --fingerprint json \
  --json-pointer /0/tag_name \
  --interval 900 \
  --enable

# Receive deliveries from a system you control
wardian automation listener hook \
  --blueprint deploy-check \
  --name "CI results" \
  --path ci \
  --auth hmac \
  --enable
```

PowerShell uses the same commands with backtick line continuations:

```powershell
wardian automation listener watch `
  --blueprint code-review `
  --name "Source audit" `
  --path <absolute-workspace-path> `
  --recursive `
  --pattern '**/*.rs' `
  --enable
```

A listener is created **disabled** unless you pass `--enable`, so a watch never
starts spending provider tokens before you have looked at it.

## What the Automation Receives

The event payload arrives as the run's trigger input, so any node can reference
it as `&#123;&#123;trigger.output.&lt;field&gt;&#125;&#125;`.

**File change**

| Field | Meaning |
| --- | --- |
| `paths` | changed paths relative to the watch root, capped at 200 |
| `path_count` | how many changed in total |
| `truncated` | whether `paths` was capped |
| `rescan` | the platform lost track of individual paths; something changed but not what |

**Inbound webhook**

| Field | Meaning |
| --- | --- |
| `delivery_id` | the sender's delivery id, used to make retries idempotent |
| `headers` | request headers, with every credential-bearing header removed |
| `body` | the request body parsed as JSON, or null |
| `body_text` | the raw body as text, or null if it is not UTF-8 |

**Web poll**

| Field | Meaning |
| --- | --- |
| `value` | the extracted value for a JSON-pointer or pattern fingerprint |
| `fingerprint` / `previous_fingerprint` | what changed, and what it was before |
| `body` / `body_truncated` | the response body, capped for the payload |

A webhook body and a poll response are **untrusted external input**. They reach
an agent prompt as data. Treat them the way you would treat any text a stranger
sent you, and be deliberate about wiring them into a `shell` or `script` node.

## Bursts, Overlap, and Runaway Protection

One editor save touches several files. A listener collapses a burst of events
into a single run once the watch has been quiet for the **quiet period**
(500 ms by default).

If a listener fires while one of its own runs is still going, the **overlap**
policy decides:

- **Skip** — drop the new event. The default for file and poll listeners,
  because a later burst supersedes an earlier one.
- **Coalesce** — keep only the newest pending event, at most one.
- **Parallel** — start a run for every event. The default for webhooks, whose
  deliveries are independent events carrying distinct payloads.

A listener that fires more than 20 times in a minute is **auto-disabled** with a
reason you can read in the monitor. This is the backstop for a watch that
triggers itself — an automation writing into the tree it watches — and for a
delivery flood. Re-enable it from the monitor or with
`wardian automation listener enable <id>` after fixing the cause.

Wardian refuses at creation time to watch a path that contains or sits inside
the Wardian home, because automation runs write there.

## While Wardian is closed

- **File and webhook listeners miss events that happen while the app is not
  running.** The operating system does not journal filesystem events for a
  process that is not listening, and an unbound port cannot receive a request.
- **A poll listener recovers.** Its fingerprint is stored on disk, so the next
  poll after startup still sees a value different from the last one it recorded
  and fires.

If it matters that an event is never missed, use a poll.

## Webhook Setup

The webhook server binds to loopback and starts automatically when at least one
webhook listener is enabled. There is no separate switch to flip.

Creating a webhook listener prints a **secret shown exactly once**. Configure
the sender with it. Wardian stores it outside the inspectable listener config
and cannot show it again; generate a new one to rotate.

- **HMAC** (default) — the sender signs the raw body with the secret and sends
  the hex digest in `X-Hub-Signature-256`, with or without a `sha256=` prefix.
  This is what GitHub and Stripe send.
- **Token** — the sender presents the secret in `X-Wardian-Token` or as
  `Authorization: Bearer <secret>`.

Wardian answers `202 Accepted` once the run is durably recorded, before it
finishes. A retried delivery carrying the same delivery id resolves to the run
that already exists rather than starting a second one.

To receive deliveries from outside this machine, expose the loopback port with a
tunnel you control (`cloudflared`, `ngrok`, `tailscale funnel`). Point the
sender at the tunnel's URL plus `/hooks/<path>`. Wardian does not bind to a
public interface itself.

Change the port with `<wardian-home>/settings/listener-gateway.json`.

## Diagnosing a Listener That Does Not Fire

Run `wardian automation listener show <id>`, or read the row in the monitor. In
order of likelihood:

1. **Status is "Off"** — it was never enabled. `--enable` or the toggle.
2. **Status is "Auto-disabled"** — the rate ceiling tripped. The reason says so.
3. **Status is "Error"** — arming failed. The watch path may be gone, or the
   webhook port may be in use. The error text is on the row.
4. **A rejection is shown** — the event arrived and was refused: a signature
   mismatch, an oversized body, or a poll response the fingerprint could not be
   read from.
5. **Nothing at all** — for a file listener, check that the pattern matches and
   that the path is not inside one of the always-ignored directories (`.git`,
   `node_modules`, `target`, `dist`, `build`).

A failure to launch writes a failed run you can open in the monitor, so a broken
blueprint does not make a listener look silently inert.

## Related References

- [Triggers](./triggers.md)
- [Scheduled Runs](./scheduled-runs.md)
- [Automation Engine Architecture](../developer/automation-engine.md)
