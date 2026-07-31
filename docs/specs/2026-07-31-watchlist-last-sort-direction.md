# Watchlist Last Sort Direction

## Decision

When the **Last** watchlist column is selected for the first time, its initial
sort direction is descending. This places the most recently queried agent at
the top of the roster. Other sortable columns retain their ascending initial
direction.

Clicking the active **Last** header still cycles from descending to unsorted;
selecting it again starts a new descending sort.

## Rationale

The purpose of **Last** is recency monitoring. Showing the oldest interaction
first on the initial sort makes the column's most useful signal harder to find
and is inconsistent with the expected “most recent first” representation.

## Verification

`cycleSort` coverage proves both the initial descending preference and the
resulting most-recent-first agent order. Existing sort-cycle behavior for other
columns remains covered unchanged.
