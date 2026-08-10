import React, { useMemo } from "react";
import { Line } from "react-konva";

import type { GardenAgentUnit } from "./garden.types";
import type { TerrainCell } from "./terrain";
import type { TerrainPaint } from "./terrainPaint";
import type { GardenTheme } from "./useGardenTheme";
import { threadsFor } from "./attributionThreads";

interface AttributionLayerProps {
  cells: readonly TerrainCell[];
  paint: ReadonlyMap<string, TerrainPaint>;
  agentUnits: readonly GardenAgentUnit[];
  /** Selected agent, when the selection is a unit. */
  selectedAgentId?: string | null;
  /** Selected ground, when the selection is a cell. */
  selectedPath?: string | null;
  theme: GardenTheme;
}

/**
 * Agent-to-ground attribution, the one relation on the map that draws a line.
 *
 * The metric map's edge policy is that only *flow* relations should be drawn,
 * because structural and affiliation relations are already expressed as
 * geometry. "This agent wrote this path at this turn" is a flow, and it is the
 * first relation in the Garden that qualifies.
 *
 * Drawn only for the current selection. Threads for every agent at once would
 * be the hairball that districts and geometry-instead-of-edges exist to avoid.
 */
export const AttributionLayer: React.FC<AttributionLayerProps> = React.memo(
  ({ cells, paint, agentUnits, selectedAgentId, selectedPath, theme }) => {
    const positions = useMemo(
      () => new Map(agentUnits.map((unit) => [unit.ref.id, unit.position])),
      [agentUnits],
    );
    const threads = useMemo(
      () => threadsFor({ cells, paint, positions, selectedAgentId, selectedPath }),
      [cells, paint, positions, selectedAgentId, selectedPath],
    );

    if (threads.length === 0) return null;
    return (
      <>
        {threads.map((thread) => (
          <Line
            key={thread.key}
            points={[thread.from.x, thread.from.y, thread.to.x, thread.to.y]}
            stroke={theme.selection}
            strokeWidth={1}
            opacity={0.55}
            listening={false}
            perfectDrawEnabled={false}
          />
        ))}
      </>
    );
  },
);
AttributionLayer.displayName = "AttributionLayer";
