# Update Availability Notice

- **Status:** Implemented
- **Date:** 2026-07-26

## Context

Wardian could discover an update only while Settings was open. This made a
newer release easy to miss and repeated the updater check when a person opened
Settings. A titlebar indicator would make an application-wide maintenance state
look like window chrome.

## Decision

The app owns one updater state for the desktop session. It checks eligible
official release builds in the background on startup. When a newer release is
available, Wardian shows one dismissible in-app notice with **Review update**
and **Later** actions.

**Review update** opens the existing Settings update controls. **Later** hides
the notice for the current session but leaves a small update marker on the
existing Settings button, so the update remains discoverable. Settings consumes
the shared updater state and does not make a second automatic check.

Wardian does not use an operating-system notification, request notification
permission, silently download, install, restart, or send any additional usage
data for this feature.

## Consequences

- People see an available update during normal use without searching Settings.
- The updater remains explicit and user-controlled: installation starts only
  after the person chooses it in Settings.
- The Settings marker persists for the current update after the transient notice
  is dismissed.
- Local, source-built, browser, prerelease, and otherwise ineligible runtimes
  continue to make no release-update request and show no notice.
