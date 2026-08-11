import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

import type {
  ChangeReviewBaseline,
  ChangeReviewReviewedPath,
  ChangeReviewWatermark,
} from "../../types";
import { useAppShellWorkbenchNavigation } from "../../layout/AppShell";
import { useSettingsStore } from "../../store/useSettingsStore";
import { fileResourceKey } from "../files/fileResourceKey";
import { openFileWithSettings } from "../files/fileOpenRouting";
import {
  baselineForFile,
  changeBaselineLabel,
  changeSurfaceState,
} from "../changes/changeSurface";
import type { TerrainChangeEntry } from "./useTerrainChanges";

export interface TerrainOpenOptions {
  entries: ReadonlyMap<string, TerrainChangeEntry>;
  baseline: ChangeReviewBaseline;
}

/**
 * Open a piece of ground.
 *
 * Two paths, one rule: a changed file opens with its comparison already open
 * against the baseline the ground is painted with, and an unchanged file opens
 * however the operator's file-open preferences say files open. Neither path
 * renders anything itself — the Garden becomes a spatial entry point to the
 * files surface rather than a second viewer, which is the same split the
 * change-review spec made when it moved diffs out of the sidebar.
 */
export function useTerrainOpen(options: TerrainOpenOptions): (path: string) => void {
  const { entries, baseline } = options;
  const navigation = useAppShellWorkbenchNavigation();
  const externalEditor = useSettingsStore((state) => state.externalEditor);
  const externalEditorCustomExecutable = useSettingsStore(
    (state) => state.externalEditorCustomExecutable,
  );
  const fileOpenActions = useSettingsStore((state) => state.fileOpenActions);

  return useCallback(
    (path: string) => {
      const changed = entries.get(path);

      if (!changed || changed.entry.binary) {
        // Binary content is listed but never rendered, so it takes the ordinary
        // open path rather than a comparison it cannot fill.
        void openFileWithSettings(path, {
          navigation,
          file_open_actions: fileOpenActions,
          external_editor: externalEditor,
          external_editor_custom_executable: externalEditorCustomExecutable,
        }).catch(() => undefined);
        return;
      }
      if (!navigation) return;

      const surfaceId = navigation.open({
        surface_type: "files",
        resource_key: fileResourceKey(path),
        state: changeSurfaceState(
          baselineForFile(
            changed.entry,
            changed.baselineRef,
            changed.root,
            changeBaselineLabel(baseline),
          ),
        ),
      });
      navigation.pin_transient(surfaceId);
      void advanceWatermarks(changed);
    },
    [entries, baseline, navigation, fileOpenActions, externalEditor, externalEditorCustomExecutable],
  );
}

/**
 * Record the review, for every agent that claimed the path.
 *
 * The change-review spec defines opening a diff as the act of reviewing, and a
 * watermark is keyed by agent and workspace. The map has no single agent, so
 * picking one would be the same arbitrary choice the map-level baseline exists
 * to avoid — but the paths an agent *claimed* are not arbitrary. Those are
 * exactly the agents whose work was just read.
 *
 * A path with no claimants advances nothing, which is correct: an `inferred`
 * write belongs to no agent, and inventing a claimant here would forge the
 * attribution the evidence discriminant exists to withhold.
 *
 * Written sequentially because `save_change_review_watermark` reads, mutates,
 * and rewrites one shared index; two concurrent writes would drop one.
 */
async function advanceWatermarks(changed: TerrainChangeEntry): Promise<void> {
  for (const agentId of changed.entry.agent_ids) {
    try {
      const existing = await invoke<ChangeReviewWatermark | null>(
        "load_change_review_watermark",
        { agent_id: agentId, workspace: changed.root },
      ).catch(() => null);

      const reviewed_paths: ChangeReviewReviewedPath[] = [...(existing?.reviewed_paths ?? [])];
      const alreadyReviewed = reviewed_paths.some(
        (reviewed) =>
          reviewed.path === changed.entry.path &&
          reviewed.change_kind === changed.entry.change_kind &&
          reviewed.insertions === changed.entry.insertions &&
          reviewed.deletions === changed.entry.deletions,
      );
      if (!alreadyReviewed) {
        reviewed_paths.push({
          path: changed.entry.path,
          change_kind: changed.entry.change_kind,
          insertions: changed.entry.insertions,
          deletions: changed.entry.deletions,
        });
      }

      await invoke("save_change_review_watermark", {
        watermark: {
          schema: 1,
          agent_id: agentId,
          workspace: changed.root,
          reviewed_turn_index:
            Math.max(0, ...changed.entry.turn_indices) || existing?.reviewed_turn_index || 0,
          reviewed_at: new Date().toISOString(),
          reviewed_head: existing?.reviewed_head ?? null,
          reviewed_paths,
        },
      });
    } catch {
      // Failing to record a review must never fail the open. The operator has
      // the diff; the pane will simply still list the path.
    }
  }
}
