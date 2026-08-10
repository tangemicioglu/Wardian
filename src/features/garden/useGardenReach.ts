import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type { AgentConfig, AgentReachEntry, AgentReachResponse } from "../../types";
import { agentWorkspaceRoots } from "./gardenProjection";
import { ROOT_KEY_SEPARATOR } from "./useGardenTerrain";

const EMPTY_REACH: readonly AgentReachEntry[] = [];

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
  const [reach, setReach] = useState<readonly AgentReachEntry[]>(EMPTY_REACH);

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
      setReach(cached);
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
        cache.current.set(rosterKey, entries);
        if (currentKey.current === rosterKey) setReach(entries);
      })
      .catch(() => {
        // A roster whose reach cannot be read lays out as if nobody coordinated
        // anything, which is the arrangement that existed before this input. A
        // failure here must not be able to blank the map.
        //
        // Deliberately not cached: a failed read answered nothing, and holding
        // it would make one transient error permanent for the session.
        if (currentKey.current === rosterKey) setReach(EMPTY_REACH);
      })
      .finally(() => {
        inFlight.current.delete(rosterKey);
      });
  }, [enabled, rosterKey, roots.length]);

  return enabled ? reach : EMPTY_REACH;
}
