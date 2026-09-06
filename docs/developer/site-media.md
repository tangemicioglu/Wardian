# Site Media Capture

The feature site at `wardian.org` is built around short looping clips of the
real Wardian UI. This page covers producing them.

The clips are recorded by a script in this repository rather than copied by
hand, so they can be regenerated on every release. Before this existed the site
drifted four releases behind, because refreshing it meant a person moving files
between two repositories.

## Running the capture

```sh
npm run site:media
```

The script starts its own Vite dev server on port 1431 with an isolated
`WARDIAN_HOME`, records each clip with Playwright, transcodes them with ffmpeg,
and writes everything to `docs/assets/site-media/`.

**ffmpeg must be on `PATH`.** The script checks for both `ffmpeg` and `ffprobe`
before it records anything and stops with an install pointer if either is
missing.

PowerShell (Windows):

```powershell
npm run site:media
```

Two environment variables override the defaults:

| Variable | Effect |
| --- | --- |
| `WARDIAN_SITE_MEDIA_PORT` | Port for the owned dev server. Default `1431`. |
| `WARDIAN_SITE_MEDIA_URL` | Capture an app already running at this URL instead of starting one. |

The stills capture (`npm run docs:screenshots`) defaults to port 1420, so the
two can run back to back without colliding.

## What it produces

```
docs/assets/site-media/
  manifest.json
  <clip-id>.mp4      H.264, yuv420p, +faststart, no audio
  <clip-id>.webm     VP9, no audio
  <clip-id>.png      poster frame
```

`manifest.json` is the integration seam with the site repository:

```json
{
  "generated_at": "2026-09-05T23:00:00.000Z",
  "app_version": "0.6.0",
  "clips": [
    {
      "id": "graph",
      "mp4": "graph.mp4",
      "webm": "graph.webm",
      "poster": "graph.png",
      "width": 1600,
      "height": 1000,
      "duration_ms": 8200,
      "bytes_mp4": 512345
    }
  ]
}
```

The site consumes the same filenames, flat, from its own `assets/media/`.

## Constraints the script enforces

These are checked in the run, not left to review:

- **Every clip is 6-12 seconds.** A clip outside that window fails the run and
  names itself, so the choreography gets adjusted rather than the budget.
- **Every mp4 is under 900 KB.** An oversized clip should be shortened, not
  shipped.
- **Every captured id appears in `EXPECTED_CLIPS`.** A clip added to the
  sequence but not to the manifest list would never be reported as skipped.
- **A run that fails partway names the clips it never reached.** Those files are
  still on disk from the previous run and still look current. This is the same
  staleness discipline the stills capture uses, and it exists because a stale
  screenshot once survived several releases unnoticed.

Required clips fail the run when they cannot be produced. Stretch clips are
reported and skipped, because an empty or broken clip is worse than an absent
section.

## Shared fixtures

Both captures drive the same seeded app through
[`scripts/lib/docs-app-mock.mjs`](https://github.com/wardian-app/Wardian/blob/main/scripts/lib/docs-app-mock.mjs),
which holds the in-browser Tauri IPC mock and every fixture. They live in one
place so the two captures cannot show different data for the same surface.

The fixtures are deterministic and public-safe: workspace paths are the
`<absolute-workspace-path>` placeholder and the agents are invented. Never point
a capture at a real workspace — the repository and the site are both public.

One difference between the two captures matters. The stills apply
`stabilizeVisuals()`, which zeroes animation and transition durations so the
images are byte-stable. The video capture deliberately does not, because it
would flatten every clip into a still image.

## Adding a clip

1. Add the id to `REQUIRED_CLIPS` (or `STRETCH_CLIPS`) in
   `scripts/capture-site-media.mjs`.
2. Add an entry to `CLIPS` with that id and a `run(page)` choreography. Aim for
   roughly 8 seconds of deliberate movement; the recorded app boot is trimmed
   automatically, so time only the actions.
3. If the surface needs seed data the shared mock does not have, pass it through
   the mock's options rather than editing fixtures other clips rely on:
   - `fixtures` shallow-overrides any exported fixture.
   - `commandResults` answers IPC commands the mock has no handler for, as plain
     serializable values.
4. Run `npm run site:media` and check the clip frame by frame for leaked paths
   before committing.

A clip whose surface the mock cannot drive should be left out and said so
plainly. Do not substitute an easier surface to fill the slot.

## Refresh workflow

[`.github/workflows/site-media.yml`](https://github.com/wardian-app/Wardian/blob/main/.github/workflows/site-media.yml)
runs the capture on every published release and on manual dispatch. It always
uploads the result as a `site-media` workflow artifact.

### The one manual setup step

Opening the pull request against the site repository needs a token, because
`GITHUB_TOKEN` cannot write to another repository.

1. Create a fine-grained personal access token scoped to
   `wardian-app/wardian.org` with **Contents: write** and **Pull requests:
   write**.
2. Add it to this repository as the secret `WARDIAN_SITE_PAT`.

Until that secret exists the workflow still captures the clips and uploads the
artifact, and logs a warning explaining what is missing. The bundle can be
downloaded and copied by hand in the meantime, so the pipeline is useful before
the token is set up.
