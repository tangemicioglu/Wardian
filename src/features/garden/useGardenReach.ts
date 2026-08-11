import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type { AgentConfig, AgentReachEntry, AgentReachResponse } from "../../types";
import { agentWorkspaceRoots } from "./gardenProjection";
import { ROOT_KEY_SEPARATOR } from "./useGardenTerrain";

const EMPTY_REACH: readonly AgentReachEntry[] = [];

/** A settled read, tagged with the roster it answers. */
interface Answer {
  key: string;
  entries: readonly AgentReachEntry[];
}

/**
 * How many rosters' answers to keep.
 *
 * The key holds the whole agent-id and root roster, so a long session that adds
 * and removes agents accumulates one entry per distinct roster. Small, but
 * unbounded, and only the current roster and whatever is in flight can be
 * revisited soon. Oldest insertion is evicted first: `Map` preserves insertion
 * order, and re-answering a roster refreshes its position.
 */
const REACH_CACHE_LIMIT = 16;

/** Separates the two lists in the cache key, so neither can spell the other. */
const KEY_SECTION_SEPARATOR = `${ROOT_KEY_SEPARATOR}${ROOT_KEY_SEPARATOR}`;

/**
 * Workspace roots each agent has written under.
 *
 * Garden seats a district nearer the centre when its agents write into other
 * districts' territory, so this is a *layout* input — which makes when it is
 * fetched part of the design rather than an implementation detail.
 *
 * **Fetched once per roster, never subscribed.** Reach is history: it can only
 * grow, and a growth is a genuine structural event. But the map must not
 * rearrange itself while someone is looking at it, so this deliberately does not
 * listen to `explorer-changed` or `git-changed` the way the change paint does.
 * A cross-boundary write today moves a district on the next launch. Paint is
 * live; geometry is not.
 *
 * Takes the agents rather than their roots, and that is a fix rather than a
 * convenience: the response is keyed by `agent_id` while the request is keyed by
 * root, and the two do not move together. Adding an agent to a workspace that
 * already had one leaves the root set identical, so a root-keyed cache
 * suppressed the fetch and the new agent's history stayed missing until the next
 * launch. The roster is part of the question, so it is part of the key.
 */
export function useGardenReach(
  enabled: boolean,
  agents: readonly AgentConfig[],
): readonly AgentReachEntry[] {
  // The answer carries the roster it answers. Holding the entries alone let the
  // previous roster's reach through on the render where the roster changed —
  // state lags by a render even on a cache hit, because effects run after paint.
  // That is enough to seat a district on evidence from a roster that is gone,
  // and then move it again when the real answer lands.
  const [answer, setAnswer] = useState<Answer | null>(null);

  const roots = useMemo(() => agentWorkspaceRoots(agents), [agents]);
  const rosterKey = useMemo(
    () =>
      [
        agents.map((agent) => agent.session_id).sort().join(ROOT_KEY_SEPARATOR),
        roots.join(ROOT_KEY_SEPARATOR),
      ].join(KEY_SECTION_SEPARATOR),
    [agents, roots],
  );

  const rootsRef = useRef(roots);
  rootsRef.current = roots;

  // Answers, by roster. Holding the *value* rather than a "seen" flag is what
  // makes cancellation safe: a request outlives the render that started it, so
  // the render is the wrong thing to attach the result to.
  const cache = useRef(new Map<string, readonly AgentReachEntry[]>());
  // Requests started and not yet settled, so two passes cannot ask the same
  // question twice.
  const inFlight = useRef(new Set<string>());
  // The roster currently on screen. A resolving request applies its answer only
  // if this still matches, which is what lets a request survive a roster going
  // A -> B -> A: the late A response is applied by whoever is current, instead
  // of being discarded because *its own* effect was torn down.
  const currentKey = useRef(rosterKey);
  currentKey.current = rosterKey;

  useEffect(() => {
    if (!enabled || roots.length === 0) return;

    const cached = cache.current.get(rosterKey);
    if (cached) {
      setAnswer({ key: rosterKey, entries: cached });
      return;
    }
    // An outstanding request for this roster will apply its own answer through
    // `currentKey`, so starting a second one would only duplicate the read.
    if (inFlight.current.has(rosterKey)) return;

    inFlight.current.add(rosterKey);
    void Promise.resolve(
      invoke<AgentReachResponse>("load_agent_reach", { roots: [...rootsRef.current] }),
    )
      .then((response) => {
        if (!response) return;
        const entries = response.agents ?? EMPTY_REACH;
        remember(cache.current, rosterKey, entries);
        if (currentKey.current === rosterKey) setAnswer({ key: rosterKey, entries });
      })
      .catch(() => {
        // A roster whose reach cannot be read lays out as if nobody coordinated
        // anything, which is the arrangement that existed before this input. A
        // failure here must not be able to blank the map.
        //
        // Deliberately not cached: a failed read answered nothing, and holding
        // it would make one transient error permanent for the session.
        if (currentKey.current === rosterKey) setAnswer({ key: rosterKey, entries: EMPTY_REACH });
      })
      .finally(() => {
        inFlight.current.delete(rosterKey);
      });
  }, [enabled, rosterKey, roots.length]);

  if (!enabled) return EMPTY_REACH;
  // A roster with no answer of its own lays out with no reach rather than with
  // its predecessor's. The cache is consulted here so a return to a known roster
  // is immediate instead of waiting a render for the effect.
  if (answer?.key === rosterKey) return answer.entries;
  return cache.current.get(rosterKey) ?? EMPTY_REACH;
}

/** Insert, refreshing recency, and evict the oldest beyond the bound. */
function remember(
  cache: Map<string, readonly AgentReachEntry[]>,
  key: string,
  entries: readonly AgentReachEntry[],
): void {
  cache.delete(key);
  cache.set(key, entries);
  while (cache.size > REACH_CACHE_LIMIT) {
    const oldest = cache.keys().next();
    if (oldest.done) break;
    cache.delete(oldest.value);
  }
}
