import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { GardenPosition } from "../features/garden/garden.types";
import {
  createScene,
  excludeFromDistrict,
  markVisited,
  pinEntity,
  reviveScene,
  scenesConverged,
  unpinEntity,
  type GardenScene,
} from "../features/garden/gardenScene";

interface GardenStoreState {
  scene: GardenScene;
  /** True when stored geometry predates the current metric version. */
  needsRederive: boolean;
  /**
   * Pin an entity at an absolute point, stored relative to its district origin
   * so the placement survives the district moving.
   */
  pin: (
    entityKey: string,
    districtId: string,
    absolute: GardenPosition,
    districtOrigin: GardenPosition,
  ) => void;
  unpin: (entityKey: string) => void;
  exclude: (entityKey: string, districtId: string) => void;
  visit: (entityKey: string) => void;
  /** Adopt the scene a layout pass returned (district cells, warm-start seeds). */
  adoptScene: (scene: GardenScene) => void;
  reset: () => void;
}

/**
 * Garden scene store.
 *
 * The malleable-garden spec calls for scenes to live as inspectable files under
 * the active Wardian home. They still persist through browser storage here; the
 * scene is a plain serializable object with a versioned schema and a tolerant
 * reviver precisely so that becomes a storage swap rather than a rewrite.
 *
 * Layout output is written back through `adoptScene` because the drift penalty
 * needs last-known positions to warm-start from. Without them every reload would
 * re-derive from scratch and the map would rearrange itself in front of the user.
 */
export const useGardenStore = create<GardenStoreState>()(
  persist(
    (set) => ({
      scene: createScene(),
      needsRederive: false,
      pin: (entityKey, districtId, absolute, districtOrigin) =>
        set((state) => ({
          scene: pinEntity(state.scene, entityKey, districtId, absolute, districtOrigin),
        })),
      unpin: (entityKey) => set((state) => ({ scene: unpinEntity(state.scene, entityKey) })),
      exclude: (entityKey, districtId) =>
        set((state) => ({ scene: excludeFromDistrict(state.scene, entityKey, districtId) })),
      visit: (entityKey) => set((state) => ({ scene: markVisited(state.scene, entityKey) })),
      // A layout pass always returns a fresh scene object even when nothing
      // moved. Committing it unconditionally would publish a new `scene`
      // reference on every pass, re-rendering every subscriber and re-writing
      // storage for sub-pixel changes. Keeping the existing reference when the
      // two scenes are materially the same makes the write-back idempotent,
      // which is what stops a relayout from provoking another one.
      adoptScene: (scene) =>
        set((state) => (scenesConverged(state.scene, scene) ? state : { scene })),
      reset: () => set({ scene: createScene(), needsRederive: false }),
    }),
    {
      name: "wardian-garden",
      // Only the scene is persisted; `needsRederive` is derived on load.
      partialize: (state) => ({ scene: state.scene }),
      version: 2,
      // v1 stored absolute positions keyed by unitKey plus a boolean pin map.
      // Those coordinates came from the phyllotaxis seeding and carry no meaning
      // under the metric layout, so they are dropped rather than migrated into
      // pins the user never actually placed.
      migrate: () => ({ scene: createScene() }),
      merge: (persisted, current) => {
        const revived = reviveScene((persisted as { scene?: unknown } | undefined)?.scene);
        return { ...current, scene: revived.scene, needsRederive: revived.needsRederive };
      },
    },
  ),
);
