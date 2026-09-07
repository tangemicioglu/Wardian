import { Arrow, Circle, Group, Rect, Text } from "react-konva";
import type { SituatedRoute } from "./canvasHierarchy";
import type { GardenEntityRef } from "./garden.types";
import type { GardenTheme } from "./useGardenTheme";

/** Derived attachments and execution paths; deliberately never draggable. */
export function AutomationRoutesLayer({ routes, theme, scale, selectedKey, onSelect, onOpen, mode = "all" }: {
  routes: readonly SituatedRoute[];
  theme: GardenTheme;
  scale: number;
  selectedKey: string | null;
  onSelect: (ref: GardenEntityRef) => void;
  onOpen: (ref: GardenEntityRef) => void;
  /** Keep route hits below agents and stage attention above their bodies. */
  mode?: "all" | "routes" | "markers";
}) {
  return <>{routes.map(({ input, points, anchor, presentation }) => {
    const ref: GardenEntityRef = { kind: "automation", id: input.id };
    const selected = selectedKey === `automation:${input.id}`;
    return <Group key={input.id} onClick={() => onSelect(ref)} onTap={() => onSelect(ref)} onDblClick={() => onOpen(ref)}>
      {mode !== "markers" && <><Arrow points={(points.length === 1 ? [points[0], anchor] : points).flatMap((point) => [point.x, point.y])}
        stroke={selected ? theme.selection : theme.labelMuted} fill={theme.labelMuted}
        strokeWidth={(selected ? 2 : 1) / scale} hitStrokeWidth={24 / scale}
        dash={presentation.paused ? [7 / scale, 4 / scale, 1 / scale, 4 / scale] : !presentation.live ? [5 / scale, 4 / scale] : undefined}
        pointerLength={5 / scale} pointerWidth={5 / scale} />
      <Circle x={anchor.x} y={anchor.y} radius={6 / scale} fill={theme.groundFile} stroke={selected ? theme.selection : theme.labelMuted} />
      <Text x={anchor.x + 10 / scale} y={anchor.y - 6 / scale} text={presentation.summary}
        fontFamily={theme.font} fontSize={12 / scale} fill={theme.label} /></>}
      {mode !== "routes" && presentation.markers.map((marker, index) => {
        const color = marker.attention === "failed" ? theme.change.deleted : marker.attention ? theme.change.modified : theme.labelMuted;
        const labelRow = presentation.markers.slice(0, index).filter((other) => other.position.x === marker.position.x && other.position.y === marker.position.y).length;
        return <Group key={marker.key} name="automation-stage-marker" id={marker.key} x={marker.position.x} y={marker.position.y}>
          {marker.temporary && <Rect name="temporary-provider" x={-10 / scale} y={-10 / scale} width={20 / scale} height={20 / scale}
            cornerRadius={5 / scale} fill={theme.groundFile} stroke={color} strokeWidth={1.5 / scale} dash={[3 / scale, 3 / scale]} />}
          {marker.attention && <>
            {/* Covers the route at the actual assignment, producing a local interruption. */}
            <Circle name="stage-attention" radius={24 / scale} stroke={color} strokeWidth={3 / scale} />
            <Text x={16 / scale} y={-28 / scale} text={marker.attention === "failed" ? "×" : "!"}
              fontSize={18 / scale} fontFamily={theme.font} fill={color} />
          </>}
          <Text x={28 / scale} y={(-24 + labelRow * 16) / scale} text={marker.label}
            fontFamily={theme.font} fontSize={12 / scale} fill={color}
            shadowColor={theme.labelBackdrop} shadowBlur={4 / scale} shadowOpacity={1} />
        </Group>;
      })}
    </Group>;
  })}</>;
}
