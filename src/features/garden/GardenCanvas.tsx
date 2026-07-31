import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Layer, Stage } from "react-konva";
import type Konva from "konva";
import { AgentUnit, AGENT_UNIT_NAME } from "./AgentUnit";
import { WorkflowUnit } from "./WorkflowUnit";
import { GardenContextMenu } from "./GardenContextMenu";
import { gardenDetailForScale, type GardenSkillGlyph } from "./skillGlyphs";
import { useGardenPulse } from "./useGardenPulse";
import type { GardenAgentUnit, GardenEntityRef, GardenWorkflowUnit } from "./garden.types";
import { unitKey } from "./garden.types";
import { isActiveAgentStatus, isActiveWorkflowStatus } from "./gardenStatus";
import { useGardenTheme } from "./useGardenTheme";

interface GardenCanvasProps {
  agentUnits: GardenAgentUnit[];
  workflowUnits: GardenWorkflowUnit[];
  selectedKey: string | null;
  /** Agents carrying the selected skill; empty unless a skill is selected. */
  highlightedAgentIds?: ReadonlySet<string>;
  onSelect: (ref: GardenEntityRef) => void;
  onOpenAgent: (id: string) => void;
  onOpenSkill?: (glyph: GardenSkillGlyph) => void;
  onMoveUnit: (key: string, x: number, y: number) => void;
  onResetLayout: () => void;
}

interface GardenMenuState {
  x: number;
  y: number;
  agentId: string | null;
}

const MIN_SCALE = 0.4;
const MAX_SCALE = 2.5;
const ZOOM_STEP = 1.05;
/** Margin around the content bounds when fitting the initial view, in world units. */
const FIT_PADDING = 80;

export const GardenCanvas: React.FC<GardenCanvasProps> = ({
  agentUnits,
  workflowUnits,
  selectedKey,
  highlightedAgentIds,
  onSelect,
  onOpenAgent,
  onOpenSkill,
  onMoveUnit,
  onResetLayout,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const stageRef = useRef<Konva.Stage>(null);
  const layerRef = useRef<Konva.Layer>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [scale, setScale] = useState(1);
  const [menu, setMenu] = useState<GardenMenuState | null>(null);
  // The transform the last automatic fit applied, published on the container as
  // `data-garden-fit`. Canvas units have no DOM handles, so this is the only way
  // a test (or a bug report) can turn a world position into a screen point. It
  // is not updated once the user takes over the viewport.
  const [fit, setFit] = useState<string | null>(null);
  const fitRef = useRef<string | null>(null);
  const theme = useGardenTheme();

  // Progressive disclosure. Detail is a pure function of zoom and touches only
  // what is painted — the layout reserved the crown's footprint regardless, so
  // crossing a threshold cannot move a unit.
  const detail = useMemo(() => gardenDetailForScale(scale), [scale]);
  const selectedSkillRef = selectedKey?.startsWith("skill:")
    ? selectedKey.slice("skill:".length)
    : null;

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const update = () => setSize({ width: el.clientWidth, height: el.clientHeight });
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // Open the menu on right-click via a native listener on the container (the
  // contextmenu event bubbles up from Konva's canvas, but binding directly to
  // the DOM node is more reliable than React delegation through the canvas).
  // Resolve which agent, if any, sits under the cursor via Konva hit-testing.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const onContextMenu = (e: MouseEvent) => {
      e.preventDefault();
      let agentId: string | null = null;
      try {
        const stage = stageRef.current;
        const pointer = stage?.getPointerPosition();
        if (stage && pointer) {
          const hit = stage.getIntersection(pointer);
          const group = hit?.findAncestor(`.${AGENT_UNIT_NAME}`, true);
          if (group) agentId = group.id() || null;
        }
      } catch {
        agentId = null;
      }
      setMenu({ x: e.clientX, y: e.clientY, agentId });
    };
    el.addEventListener("contextmenu", onContextMenu);
    return () => el.removeEventListener("contextmenu", onContextMenu);
  }, []);

  // Fit the map into view until the user takes control of the viewport.
  //
  // The layout places units around each district's own origin, so coordinates
  // are freely negative and a district can sit hundreds of pixels off the
  // Stage's top-left. Opening onto a viewport that happens to show two of five
  // districts is not a map.
  //
  // Fitting only once is not enough: the container is measured by a
  // ResizeObserver, and its first measurement can precede the surrounding
  // layout settling. A single fit against that early size leaves the content
  // permanently off-centre. So it re-fits on every size and content change, and
  // stops for good the moment the user pans, zooms, or moves a unit — after
  // which the viewport is theirs and must never be yanked back.
  const userAdjustedRef = useRef(false);
  useEffect(() => {
    if (userAdjustedRef.current) return;
    const stage = stageRef.current;
    if (!stage || size.width === 0 || size.height === 0) return;
    const positions = [...agentUnits, ...workflowUnits].map((unit) => unit.position);
    if (positions.length === 0) return;

    const xs = positions.map((position) => position.x);
    const ys = positions.map((position) => position.y);
    const minX = Math.min(...xs) - FIT_PADDING;
    const maxX = Math.max(...xs) + FIT_PADDING;
    const minY = Math.min(...ys) - FIT_PADDING;
    const maxY = Math.max(...ys) + FIT_PADDING;

    // Only ever zoom *out* to fit. Magnifying a sparse map would start the user
    // inside a cluster with no sense of the whole.
    const next = Math.min(
      1,
      Math.max(MIN_SCALE, Math.min(size.width / (maxX - minX), size.height / (maxY - minY))),
    );
    const position = {
      x: size.width / 2 - ((minX + maxX) / 2) * next,
      y: size.height / 2 - ((minY + maxY) / 2) * next,
    };
    // This effect re-runs on every telemetry tick, because the unit arrays are
    // rebuilt to keep status live. Writing an unchanged transform back would
    // redraw the layer each time for nothing.
    const applied = `${position.x},${position.y},${next}`;
    if (applied === fitRef.current) return;
    fitRef.current = applied;
    setScale(next);
    stage.position(position);
    setFit(applied);
  }, [agentUnits, workflowUnits, size]);

  // One animation for every breathing halo. See `useGardenPulse` for why this
  // is not per unit.
  //
  // Keyed on *which* units are breathing rather than on the unit arrays: those
  // are rebuilt on every telemetry tick, and re-running the effect that often
  // would rescan the scene graph for no reason. Only a status crossing the
  // active boundary, or a unit appearing or leaving, changes what to animate.
  const pulsingKey = useMemo(
    () =>
      [
        ...agentUnits.filter((unit) => isActiveAgentStatus(unit.status)).map((u) => u.ref.id),
        ...workflowUnits
          .filter((unit) => isActiveWorkflowStatus(unit.runStatus))
          .map((u) => u.ref.id),
      ].join(","),
    [agentUnits, workflowUnits],
  );
  useGardenPulse(layerRef, pulsingKey);

  // Stable identities so `AgentUnit` can skip re-rendering agents that did not
  // change. A closure created per unit per render would defeat that.
  const handleSelectAgent = useCallback(
    (id: string) => onSelect({ kind: "agent", id }),
    [onSelect],
  );
  const handleSelectSkill = useCallback(
    (glyph: GardenSkillGlyph) => onSelect({ kind: "skill", id: glyph.entryRef }),
    [onSelect],
  );
  const handleOpenSkill = useCallback(
    (glyph: GardenSkillGlyph) => onOpenSkill?.(glyph),
    [onOpenSkill],
  );
  const handleMoveAgent = useCallback(
    (id: string, x: number, y: number) => {
      userAdjustedRef.current = true;
      onMoveUnit(unitKey({ kind: "agent", id }), x, y);
    },
    [onMoveUnit],
  );

  const handleWheel = (e: Konva.KonvaEventObject<WheelEvent>) => {
    e.evt.preventDefault();
    userAdjustedRef.current = true;
    setScale((prev) => {
      const next = e.evt.deltaY < 0 ? prev * ZOOM_STEP : prev / ZOOM_STEP;
      return Math.min(MAX_SCALE, Math.max(MIN_SCALE, next));
    });
  };

  return (
    <div
      ref={containerRef}
      className="flex-1 min-h-0 garden-canvas"
      data-garden-fit={fit ?? undefined}
      role="img"
      aria-label={`Garden canvas showing ${agentUnits.length} agents and ${workflowUnits.length} workflows. Select a unit to read its status.`}
    >
      <Stage
        ref={stageRef}
        width={size.width}
        height={size.height}
        draggable
        scaleX={scale}
        scaleY={scale}
        onWheel={handleWheel}
        onDragEnd={(event) => {
          // Only a pan of the Stage itself; a unit drag reports the unit as
          // target and is handled by onMoveUnit.
          if (event.target === event.currentTarget) userAdjustedRef.current = true;
        }}
      >
        <Layer ref={layerRef}>
          {workflowUnits.map((unit) => (
            <WorkflowUnit
              key={unitKey(unit.ref)}
              unit={unit}
              selected={selectedKey === unitKey(unit.ref)}
              theme={theme}
              onSelect={() => onSelect(unit.ref)}
              onDragMove={(x, y) => onMoveUnit(unitKey(unit.ref), x, y)}
            />
          ))}
          {agentUnits.map((unit) => (
            <AgentUnit
              key={unitKey(unit.ref)}
              unit={unit}
              selected={selectedKey === unitKey(unit.ref)}
              highlighted={highlightedAgentIds?.has(unit.ref.id) ?? false}
              detail={detail}
              theme={theme}
              selectedSkillRef={selectedSkillRef}
              onSelect={handleSelectAgent}
              onOpen={onOpenAgent}
              onSelectSkill={handleSelectSkill}
              onOpenSkill={handleOpenSkill}
              onDragMove={handleMoveAgent}
            />
          ))}
        </Layer>
      </Stage>
      {menu && (
        <GardenContextMenu
          x={menu.x}
          y={menu.y}
          agentId={menu.agentId}
          onOpenAgent={onOpenAgent}
          onResetLayout={onResetLayout}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
};
