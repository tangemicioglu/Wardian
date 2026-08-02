# Exclusive Graph Object Selection

**Status:** Implemented
**Date:** 2026-08-01

## Context and Problem Statement

Graph nodes and manual communication edges were tracked by separate state
owners. A node remained selected through `selectedAgentIds` while an edge was
selected through `selectedEdgeId`, allowing both highlights to remain active
and making the Delete target ambiguous.

## Decision

Treat node and edge selection as mutually exclusive at the Graph view
boundary for direct graph interactions. A node click clears the local edge
selection. An edge click clears the shared agent selection.

The shared agent selection remains the source of truth for node selection so
the graph continues to integrate with the roster and other agent surfaces.

## Consequences

- **Positive**: Only one graph object can be highlighted and acted on at a time.
- **Positive**: Delete has an unambiguous selected manual edge target.
- **Negative**: Selecting an edge clears any multi-agent selection in the shared roster state.
