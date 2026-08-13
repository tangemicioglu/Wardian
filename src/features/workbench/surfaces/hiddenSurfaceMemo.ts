import type { SurfaceVisibility } from "./coreSurfaceMetadata";

type SurfaceWithVisibility = {
  visibility?: SurfaceVisibility;
};

/**
 * Keeps a mounted hidden surface on its last rendered snapshot.
 *
 * Parent-owned data is refreshed by React when the surface becomes visible.
 * Internal state remains live because memoization only skips parent renders.
 */
export function keepHiddenSurfaceSnapshot<T extends SurfaceWithVisibility>(
  previous: Readonly<T>,
  next: Readonly<T>,
): boolean {
  return previous.visibility === "hidden" && next.visibility === "hidden";
}
