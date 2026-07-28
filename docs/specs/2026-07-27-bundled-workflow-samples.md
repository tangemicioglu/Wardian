# Bundled Workflow Samples

- **Status:** Implemented
- **Date:** 2026-07-27
- **Scope:** Privacy-safe workflow templates bundled into every Wardian library.

## Problem

Workflow discovery alone does not help an operator understand how to turn a
recurring or multi-step request into a safe graph. Starting on an empty canvas
also makes it easy to omit validation, approval, or an explicit schedule
boundary.

## Decision

Bundle four original Wardian templates: code-change review, scheduled brief,
research brief, and incident triage. Seed them into
`<WARDIAN_HOME>/library/workflows/samples/` only when the destination does not
already exist, preserving any operator edits. The templates are neither run nor
scheduled automatically.

The templates use generic role references and input fields. Their patterns are
derived from sanitized local workflow structures and public Archon and
Anthropic cookbook material; no user-specific content or upstream workflow
text is shipped.

## Verification

Unit tests verify that a seed creates a missing sample without overwriting a
user edit, and that every bundled template parses and validates against the
current workflow registry.
