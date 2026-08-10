import React, { useMemo } from "react";
import { Group, Rect, Text } from "react-konva";
import type Konva from "konva";

import { MIN_GROUND_RADIUS, type TerrainCell, type TerrainDistrict } from "./terrain";
import type { TerrainPaint } from "./terrainPaint";
import type { GardenTheme } from "./useGardenTheme";

interface TerrainLayerProps {
  cells: readonly TerrainCell[];
  districts: ReadonlyMap<string, TerrainDistrict>;
  /** Live world-to-screen factor, for the label legibility gate only. */
  scale: number;
  theme: GardenTheme;
  /** Change tint per path, including ancestors. Empty when nothing is loaded. */
  paint?: ReadonlyMap<string, TerrainPaint>;
}

/**
 * Strongest a change tint ever paints.
 *
 * The ground is context and the units are content, so a repository with ten
 * thousand changed lines has to stay quieter than an agent's status halo. A
 * full-strength wash also destroys the churn comparison it exists to make:
 * everything above the knee reads the same.
 */
const MAX_CHANGE_ALPHA = 0.55;

/** Floor, so a one-line change is visible rather than technically painted. */
const MIN_CHANGE_ALPHA = 0.12;

/** How much of the tint recency controls, the rest coming from churn. */
const RECENCY_SHARE = 0.4;

function changeAlpha(paint: TerrainPaint): number {
  const intensity = paint.churn * (1 - RECENCY_SHARE) + paint.recency * RECENCY_SHARE;
  const scaled = MIN_CHANGE_ALPHA + intensity * (MAX_CHANGE_ALPHA - MIN_CHANGE_ALPHA);
  // Reviewed work is still work: dimmed rather than hidden, because the
  // change-review rule is that no state may remove a path git reports.
  return paint.reviewed ? scaled * 0.4 : scaled;
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
  ({ cells, districts, scale, theme, paint }) => {
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
                <TerrainCellShape
                  key={cell.path}
                  cell={cell}
                  scale={scale}
                  theme={theme}
                  paint={paint?.get(cell.path)}
                />
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
  paint?: TerrainPaint;
}> = ({ cell, scale, theme, paint }) => {
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
      {paint && (
        // A separate tint rect rather than a blended fill, so the ground keeps
        // reading as ground and the change reads as something laid over it.
        // `attributed` strokes solid and `inferred` dashed: the change-review
        // spec makes that discriminant load-bearing, and a visual difference is
        // read where a badge in a tooltip is not.
        <Rect
          x={rect.x}
          y={rect.y}
          width={rect.width}
          height={rect.height}
          fill={theme.change[paint.kind]}
          opacity={changeAlpha(paint)}
          stroke={theme.change[paint.kind]}
          strokeWidth={cell.depth === 0 ? 1.5 : 0.75}
          dash={paint.evidence === "inferred" ? [4, 3] : undefined}
          cornerRadius={cell.depth === 0 ? 6 : 1}
          perfectDrawEnabled={false}
          listening={false}
        />
      )}
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
