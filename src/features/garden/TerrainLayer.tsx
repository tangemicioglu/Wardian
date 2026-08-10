import React, { useMemo } from "react";
import { Group, Rect, Text } from "react-konva";
import type Konva from "konva";

import { MIN_GROUND_RADIUS, type TerrainCell, type TerrainDistrict } from "./terrain";
import type { GardenTheme } from "./useGardenTheme";

interface TerrainLayerProps {
  cells: readonly TerrainCell[];
  districts: ReadonlyMap<string, TerrainDistrict>;
  /** Live world-to-screen factor, for the label legibility gate only. */
  scale: number;
  theme: GardenTheme;
}

/** Smallest cell, in screen pixels, that gets a name written on it. */
const LABEL_MIN_WIDTH_PX = 46;
const LABEL_MIN_HEIGHT_PX = 13;

/**
 * Ground opacity by depth.
 *
 * The ground is context, not content: units and their status must stay the
 * brightest thing on the map. Deeper cells sit slightly stronger than their
 * parents so nesting reads without borders doing all the work.
 */
const DEPTH_OPACITY = [0.34, 0.42, 0.5, 0.56];

function opacityForDepth(depth: number): number {
  return DEPTH_OPACITY[Math.min(depth, DEPTH_OPACITY.length - 1)];
}

/**
 * The ground beneath the districts.
 *
 * Rendered as its own group *below* the units, and non-interactive in this
 * slice: `listening={false}` keeps two thousand rectangles out of Konva's
 * hit-testing graph entirely, which is what makes drawing them affordable.
 *
 * Cells never move in response to zoom. Zooming changes which levels are drawn
 * — see `terrainFrontier.ts` — and a cell drawn at two zoom levels occupies the
 * same world rect at both, because its rect is a function of its parent's rect
 * and its siblings and of nothing else.
 */
export const TerrainLayer: React.FC<TerrainLayerProps> = React.memo(
  ({ cells, districts, scale, theme }) => {
    // Grouped per district so each can be clipped to its own territory. The
    // ground is the bounding square of the district's disc, so without the clip
    // a populous district would paint into the gap its neighbours rely on.
    const byDistrict = useMemo(() => {
      const grouped = new Map<string, TerrainCell[]>();
      for (const cell of cells) {
        const existing = grouped.get(cell.districtId);
        if (existing) existing.push(cell);
        else grouped.set(cell.districtId, [cell]);
      }
      return grouped;
    }, [cells]);

    return (
      <>
        {[...byDistrict.entries()].map(([districtId, districtCells]) => {
          const district = districts.get(districtId);
          if (!district) return null;
          // The same radius `buildTerrain` measured the ground square against,
          // so the clip is exactly the territory and never a cell wider than it.
          const radius = Math.max(district.radius, MIN_GROUND_RADIUS);
          return (
            <Group
              key={districtId}
              listening={false}
              clipFunc={(context: Konva.Context) => {
                context.arc(district.origin.x, district.origin.y, radius, 0, Math.PI * 2, false);
              }}
            >
              {districtCells.map((cell) => (
                <TerrainCellShape key={cell.path} cell={cell} scale={scale} theme={theme} />
              ))}
            </Group>
          );
        })}
      </>
    );
  },
);
TerrainLayer.displayName = "TerrainLayer";

const TerrainCellShape: React.FC<{
  cell: TerrainCell;
  scale: number;
  theme: GardenTheme;
}> = ({ cell, scale, theme }) => {
  const { rect } = cell;
  const fill = cell.depth === 0 ? theme.ground : cell.isDir ? theme.groundDir : theme.groundFile;
  const showLabel =
    rect.width * scale >= LABEL_MIN_WIDTH_PX && rect.height * scale >= LABEL_MIN_HEIGHT_PX;

  return (
    <>
      <Rect
        x={rect.x}
        y={rect.y}
        width={rect.width}
        height={rect.height}
        fill={fill}
        opacity={opacityForDepth(cell.depth)}
        stroke={theme.groundBorder}
        strokeWidth={cell.depth === 0 ? 1.5 : 0.5}
        cornerRadius={cell.depth === 0 ? 6 : 1}
        // Konva's perfect-draw pass allocates an offscreen canvas per shape to
        // composite fill and stroke correctly at partial opacity. At two
        // thousand ground cells that is the dominant cost, and the artefact it
        // prevents is invisible on a 0.5px hairline.
        perfectDrawEnabled={false}
        listening={false}
      />
      {showLabel && (
        <Text
          x={rect.x + 4}
          y={rect.y + 3}
          width={Math.max(0, rect.width - 8)}
          text={cell.name}
          fontFamily={theme.font}
          fontSize={theme.subLabelSize}
          fill={theme.labelMuted}
          opacity={0.85}
          ellipsis
          wrap="none"
          listening={false}
          perfectDrawEnabled={false}
        />
      )}
    </>
  );
};
