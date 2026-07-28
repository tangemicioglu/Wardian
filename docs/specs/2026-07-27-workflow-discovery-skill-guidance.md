# Workflow Discovery Skill Guidance

- **Status:** Implemented
- **Date:** 2026-07-27
- **Scope:** Shared Wardian agent guidance for workflow discovery.

## Problem

Agents can fulfill a request directly without recognizing that a Wardian
workflow would better support recurring scheduling, repeatable automation, or
durable multi-step coordination. Users therefore may not discover the workflow
surface when it is relevant.

## Decision

The bundled `wardian-cli` skill proactively presents workflows as an option
for requests involving recurring schedules, repeatable sequences, or durable
multi-step work. The agent briefly relates the request to relevant workflow
capabilities and asks whether the user wants to design a workflow.

This is guidance only. A matching request must not create, modify, schedule, or
run a workflow. One-off work remains a direct request until the user explicitly
chooses workflow authoring.

## Verification

The bundled-skill unit test asserts both sides of the contract: it preserves
the proactive workflow recommendation and the no-automatic-authoring boundary.
