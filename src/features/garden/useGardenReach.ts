import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import type { AgentReachEntry, AgentReachResponse } from "../../types";
import { ROOT_KEY_SEPARATOR } from "./useGardenTerrain";

const EMPTY_REACH: readonly AgentReachEntry[] = [];

/**
 * Workspace roots each agent has written under.
 *
 * Garden seats a district nearer the centre when its agents write into other
 * districts' territory, so this is a *layout* input — which makes when it is
 * fetched part of the design rather than an implementation detail.
 *
 * **Fetched once per root set, never subscribed.** Reach is history: it can only
 * grow, and a growth is a genuine structural event. But the map must not
 * rearrange itself while someone is looking at it, so this deliberately does not
 * listen to `explorer-changed` or `git-changed` the way the change paint does.
 * A cross-boundary write today moves a district on the next launch. Paint is
 * live; geometry is not.
 *
 * Roots are the districts' workspace roots, supplied by the caller because it is
 * the side that knows them. Sorted into a stable key so the effect does not
 * re-fire when the same roots arrive in a different order.
 */
export function useGardenReach(
  enabled: boolean,
  roots: readonly string[],
): readonly AgentReachEntry[] {
  const [reach, setReach] = useState<readonly AgentReachEntry[]>(EMPTY_REACH);
  const rootsRef = useRef(roots);
  rootsRef.current = roots;
  const rootKey = [...roots].sort().join(ROOT_KEY_SEPARATOR);
  // Root sets already answered, so `enabled` going false and true again — which
  // is what hiding and re-showing the surface does — cannot refetch.
  //
  // Without this the once-per-session rule was a claim the code did not keep:
  // the effect depends on `enabled`, so re-showing the Garden re-read the
  // archive, and a cross-boundary write made while it was hidden could re-seat
  // districts the moment it came back. Reach is history; a new look at the map
  // is not a reason to adopt more of it.
  const answered = useRef(new Set<string>());

  useEffect(() => {
    if (!enabled || rootKey.length === 0) return;
    if (answered.current.has(rootKey)) return;
    answered.current.add(rootKey);
    let active = true;
    void Promise.resolve(
      invoke<AgentReachResponse>("load_agent_reach", { roots: [...rootsRef.current] }),
    )
      .then((response) => {
        if (!active || !response) return;
        setReach(response.agents ?? EMPTY_REACH);
      })
      .catch(() => {
        // A roster whose reach cannot be read lays out as if nobody coordinated
        // anything, which is the arrangement that existed before this input. A
        // failure here must not be able to blank the map.
        //
        // The root set is released so a later pass may retry: a failed read
        // answered nothing, and holding it would make one transient error
        // permanent for the session.
        answered.current.delete(rootKey);
        if (active) setReach(EMPTY_REACH);
      });
    return () => {
      active = false;
    };
  }, [enabled, rootKey]);

  return enabled ? reach : EMPTY_REACH;
}
