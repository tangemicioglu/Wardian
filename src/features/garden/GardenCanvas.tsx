import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Layer, Stage } from "react-konva";
import type Konva from "konva";
import { AgentUnit, AGENT_UNIT_NAME } from "./AgentUnit";
import { WorkflowUnit } from "./WorkflowUnit";
import { GardenContextMenu } from "./GardenContextMenu";
import { gardenDetailForScale, type GardenSkillGlyph } from "./skillGlyphs";
import { useGardenPulse } from "./useGardenPulse";
import { MAX_SCALE, MIN_SCALE, fitTransform, zoomAt } from "./gardenViewport";
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

const ZOOM_STEP = 1.05;
/** Coarser than the wheel: a keypress is a deliberate step, not a nudge. */
const KEY_ZOOM_STEP = 1.25;
/** Screen pixels moved per arrow press, so panning feels the same at any zoom. */
const PAN_STEP = 80;

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
  const minScaleRef = useRef(MIN_SCALE);
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

  // Zoom about a fixed screen point, keeping the world point under it still.
  //
  // Scaling without this moves everything except the stage's own origin, so the
  // map slides across the viewport as it grows — which reads as panning, not
  // zooming, and is why the wheel felt like it was scrolling the canvas.
  const zoomAround = useCallback((screenPoint: { x: number; y: number }, factor: number) => {
    const stage = stageRef.current;
    if (!stage) return;
    userAdjustedRef.current = true;
    setScale((previous) => {
      const next = zoomAt(
        screenPoint,
        { scale: previous, position: { x: stage.x(), y: stage.y() } },
        factor,
        { min: minScaleRef.current, max: MAX_SCALE },
      );
      if (next.scale === previous) return previous;
      stage.position(next.position);
      stage.batchDraw();
      return next.scale;
    });
  }, []);

  /** Zoom about the middle of the viewport, for the keyboard and the buttons. */
  const zoomByStep = useCallback(
    (factor: number) => zoomAround({ x: size.width / 2, y: size.height / 2 }, factor),
    [zoomAround, size.width, size.height],
  );

  const panBy = useCallback((dx: number, dy: number) => {
    const stage = stageRef.current;
    if (!stage) return;
    userAdjustedRef.current = true;
    stage.position({ x: stage.x() - dx, y: stage.y() - dy });
    stage.batchDraw();
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
  const applyFit = useCallback(
    (force: boolean) => {
      const stage = stageRef.current;
      if (!stage) return;
      const transform = fitTransform(
        [...agentUnits, ...workflowUnits].map((unit) => unit.position),
        size,
      );
      if (!transform) return;

      // This runs on every telemetry tick, because the unit arrays are rebuilt
      // to keep status live. Writing an unchanged transform back would redraw
      // the layer each time for nothing.
      const applied = `${transform.position.x},${transform.position.y},${transform.scale}`;
      if (!force && applied === fitRef.current) return;
      fitRef.current = applied;
      // The wheel must not snap the user back the moment they touch it.
      minScaleRef.current = Math.min(MIN_SCALE, transform.scale);
      setScale(transform.scale);
      stage.position(transform.position);
      stage.batchDraw();
      setFit(applied);
    },
    [agentUnits, workflowUnits, size],
  );

  useEffect(() => {
    if (userAdjustedRef.current) return;
    applyFit(false);
  }, [applyFit]);

  /**
   * Fit on demand. Unlike the automatic fit this does not hand the viewport
   * back: the user asked for this framing, so it is theirs until they move
   * again, and a later telemetry tick must not re-frame around them.
   */
  const fitNow = useCallback(() => {
    applyFit(true);
    userAdjustedRef.current = true;
  }, [applyFit]);

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

  // The wheel always zooms, and never scrolls. A canvas that pans on wheel and
  // zooms on modifier-wheel forces the user to discover which they are doing by
  // trying it; one gesture with one meaning does not.
  const handleWheel = (e: Konva.KonvaEventObject<WheelEvent>) => {
    e.evt.preventDefault();
    const stage = stageRef.current;
    const pointer = stage?.getPointerPosition() ?? { x: size.width / 2, y: size.height / 2 };
    zoomAround(pointer, e.evt.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    // A pan step in screen pixels, so it feels the same at every zoom level.
    const step = e.shiftKey ? PAN_STEP * 4 : PAN_STEP;
    const actions: Record<string, () => void> = {
      "+": () => zoomByStep(KEY_ZOOM_STEP),
      "=": () => zoomByStep(KEY_ZOOM_STEP),
      "-": () => zoomByStep(1 / KEY_ZOOM_STEP),
      _: () => zoomByStep(1 / KEY_ZOOM_STEP),
      "0": fitNow,
      f: fitNow,
      ArrowLeft: () => panBy(-step, 0),
      ArrowRight: () => panBy(step, 0),
      ArrowUp: () => panBy(0, -step),
      ArrowDown: () => panBy(0, step),
    };
    const action = actions[e.key];
    if (!action) return;
    e.preventDefault();
    action();
  };

  return (
    <div
      ref={containerRef}
      className="relative flex-1 min-h-0 garden-canvas"
      data-garden-fit={fit ?? undefined}
      // Focusable so the canvas can own its navigation keys. Without a tabIndex
      // the map is reachable only by mouse, which is the one input the report
      // said was confusing.
      tabIndex={0}
      onKeyDown={handleKeyDown}
      role="img"
      aria-label={`Garden canvas showing ${agentUnits.length} agents and ${workflowUnits.length} workflows. Select a unit to read its status. Scroll to zoom, drag to pan, or use plus and minus to zoom, arrow keys to pan, and zero to fit.`}
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
      <div
        data-testid="garden-viewport-controls"
        className="absolute bottom-3 right-3 z-10 flex items-center gap-1 rounded-md border border-wardian-border bg-[var(--color-wardian-bg)]/90 px-1 py-1 text-[11px] shadow-sm backdrop-blur"
      >
        <button
          type="button"
          aria-label="Zoom out"
          title="Zoom out (-)"
          className="rounded px-1.5 py-0.5 text-muted hover:text-primary"
          onClick={() => zoomByStep(1 / KEY_ZOOM_STEP)}
        >
          &minus;
        </button>
        {/* Doubles as the readout that tells you how far out you are, which is
            the thing a blank-looking canvas never explains by itself. */}
        <span
          data-testid="garden-zoom-level"
          className="min-w-[3.5rem] text-center tabular-nums text-muted"
        >
          {Math.round(scale * 100)}%
        </span>
        <button
          type="button"
          aria-label="Zoom in"
          title="Zoom in (+)"
          className="rounded px-1.5 py-0.5 text-muted hover:text-primary"
          onClick={() => zoomByStep(KEY_ZOOM_STEP)}
        >
          +
        </button>
        <span className="mx-0.5 h-3 w-px bg-wardian-border" aria-hidden="true" />
        <button
          type="button"
          data-testid="garden-fit-view"
          title="Fit everything in view (0)"
          className="rounded px-1.5 py-0.5 text-muted hover:text-primary"
          onClick={fitNow}
        >
          Fit
        </button>
      </div>
      <div className="pointer-events-none absolute bottom-3 left-1/2 z-10 -translate-x-1/2 rounded-md border border-wardian-border bg-[var(--color-wardian-bg)]/80 px-2 py-1 text-[10px] text-muted-neutral shadow-sm backdrop-blur">
        Scroll to zoom · drag to pan · arrows to move · 0 to fit
      </div>
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
