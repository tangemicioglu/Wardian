import React from "react";
import { Circle, Group, Rect, Text } from "react-konva";
import type { GardenWorkflowUnit } from "./garden.types";
import { isActiveWorkflowStatus, workflowStatusColor } from "./gardenStatus";
import { resolveCssVar } from "./resolveColor";
import { PULSE_HALO_NAME } from "./useGardenPulse";
import type { GardenTheme } from "./useGardenTheme";

interface WorkflowUnitProps {
  unit: GardenWorkflowUnit;
  selected: boolean;
  theme: GardenTheme;
  onSelect: (id: string) => void;
  onDragEnd: (x: number, y: number) => void;
}

const POD_WIDTH = 84;
const POD_HEIGHT = 34;
const MAX_PIPS = 6;

export const WorkflowUnit: React.FC<WorkflowUnitProps> = ({
  unit,
  selected,
  theme,
  onSelect,
  onDragEnd,
}) => {
  const fill = resolveCssVar(workflowStatusColor(unit.runStatus));
  const active = isActiveWorkflowStatus(unit.runStatus);
  const pips = Math.min(Math.max(unit.nodeCount, 0), MAX_PIPS);

  return (
    <Group
      x={unit.position.x}
      y={unit.position.y}
      draggable
      onMouseEnter={(event) => {
        event.target.getStage()?.container().style.setProperty("cursor", "pointer");
      }}
      onMouseLeave={(event) => {
        event.target.getStage()?.container().style.setProperty("cursor", "default");
      }}
      onClick={() => onSelect(unit.ref.id)}
      onTap={() => onSelect(unit.ref.id)}
      onDragStart={(event) => {
        event.target.getStage()?.container().style.setProperty("cursor", "grabbing");
      }}
      onDragEnd={(e) => {
        e.target.getStage()?.container().style.setProperty("cursor", "pointer");
        onDragEnd(e.target.x(), e.target.y());
      }}
    >
      {/* Named so the canvas' single pulse animation can find it; see
          `useGardenPulse` for why this is not animated through React. */}
      <Rect
        name={active ? PULSE_HALO_NAME : undefined}
        width={POD_WIDTH}
        height={POD_HEIGHT}
        cornerRadius={10}
        fill={fill}
        opacity={0.22}
        listening={false}
      />
      <Rect
        width={POD_WIDTH}
        height={POD_HEIGHT}
        cornerRadius={10}
        stroke={selected ? theme.selection : fill}
        strokeWidth={selected ? 2 : 1}
      />
      {Array.from({ length: pips }).map((_, i) => (
        <Circle key={i} x={10 + i * 11} y={POD_HEIGHT - 8} radius={3} fill={fill} />
      ))}
      <Text
        text={unit.label}
        fontSize={theme.labelSize}
        fontFamily={theme.font}
        fill={theme.label}
        y={-18}
        width={POD_WIDTH}
        align="center"
        shadowColor={theme.labelBackdrop}
        shadowBlur={4}
        shadowOpacity={1}
      />
    </Group>
  );
};
