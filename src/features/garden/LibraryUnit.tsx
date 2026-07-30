import React from "react";
import { Group, Line, Rect, Text } from "react-konva";
import type { GardenLibraryUnit } from "./garden.types";
import { resolveCssVar } from "./resolveColor";
import type { GardenTheme } from "./useGardenTheme";

interface LibraryUnitProps {
  unit: GardenLibraryUnit;
  selected: boolean;
  theme: GardenTheme;
  onSelect: (unit: GardenLibraryUnit) => void;
  onDragMove: (x: number, y: number) => void;
}

const CARD_WIDTH = 104;
const CARD_HEIGHT = 34;

/**
 * Library assets read as flat cards rather than the agents' glowing dots.
 *
 * The shape carries meaning: agents are live processes and pulse with status,
 * while a skill or prompt is inert material. Giving them the same visual
 * vocabulary would imply the map is showing two of the same thing.
 */
const KIND_COLOR: Record<string, string> = {
  skill: "var(--color-wardian-accent)",
  prompt: "var(--color-wardian-processing)",
  class: "var(--color-wardian-warning)",
};

export const LibraryUnit: React.FC<LibraryUnitProps> = ({
  unit,
  selected,
  theme,
  onSelect,
  onDragMove,
}) => {
  const accent = resolveCssVar(KIND_COLOR[unit.ref.kind] ?? "var(--color-wardian-accent)");
  // An undeployed asset is inert. Drawing it at full strength would overstate
  // its role, so it reads as an outline until something actually uses it.
  const deployed = unit.deploymentCount > 0;

  return (
    <Group
      x={unit.position.x}
      y={unit.position.y}
      draggable
      onClick={() => onSelect(unit)}
      onTap={() => onSelect(unit)}
      onDragMove={(event) => onDragMove(event.target.x(), event.target.y())}
    >
      <Rect
        width={CARD_WIDTH}
        height={CARD_HEIGHT}
        offsetX={CARD_WIDTH / 2}
        offsetY={CARD_HEIGHT / 2}
        cornerRadius={4}
        fill={accent}
        opacity={deployed ? 0.16 : 0.06}
        stroke={selected ? theme.selection : accent}
        strokeWidth={selected ? 2 : 1}
        // A copied deployment is a fork whose edits do not sync back. Dashing the
        // border makes that visible on the map instead of only in the Library.
        dash={unit.hasCopiedDeployment ? [4, 3] : undefined}
      />
      {deployed && (
        <Line
          points={[
            -CARD_WIDTH / 2 + 6,
            CARD_HEIGHT / 2 - 5,
            -CARD_WIDTH / 2 + 6 + Math.min(unit.deploymentCount, 6) * 7,
            CARD_HEIGHT / 2 - 5,
          ]}
          stroke={accent}
          strokeWidth={2}
          lineCap="round"
        />
      )}
      <Text
        text={unit.label}
        fontSize={theme.subLabelSize}
        fontFamily={theme.font}
        fill={theme.label}
        width={CARD_WIDTH - 10}
        offsetX={CARD_WIDTH / 2 - 5}
        offsetY={6}
        align="center"
        ellipsis
        wrap="none"
        shadowColor={theme.labelBackdrop}
        shadowBlur={4}
        shadowOpacity={1}
      />
    </Group>
  );
};
