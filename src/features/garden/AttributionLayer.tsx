import React, { useMemo } from "react";
import { Circle, Line } from "react-konva";

import type { GardenAgentUnit } from "./garden.types";
import type { TerrainCell } from "./terrain";
import type { TerrainPaint } from "./terrainPaint";
import type { GardenTheme } from "./useGardenTheme";
import { threadsFor } from "./attributionThreads";

/** World-space radius of the dot marking where a thread lands. */
const THREAD_TERMINATOR_RADIUS = 3;

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
          <React.Fragment key={thread.key}>
            <Line
              points={[thread.from.x, thread.from.y, thread.to.x, thread.to.y]}
              stroke={theme.selection}
              strokeWidth={1}
              opacity={0.55}
              listening={false}
              perfectDrawEnabled={false}
            />
            {/* A thread lands on a cell's geometric centre, and a cell's only
                visual anchor is a label at its top-left — so a line into a large
                folder ends in blank space and reads as going nowhere. The
                terminator says the line arrived. */}
            <Circle
              x={thread.to.x}
              y={thread.to.y}
              radius={THREAD_TERMINATOR_RADIUS}
              fill={theme.selection}
              opacity={0.75}
              listening={false}
              perfectDrawEnabled={false}
            />
          </React.Fragment>
        ))}
      </>
    );
  },
);
AttributionLayer.displayName = "AttributionLayer";
