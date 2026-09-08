import { useLayoutEffect, useRef, type CSSProperties, type ReactNode } from "react";
import type { GardenCamera, GardenEntityRef } from "./garden.types";
import { CELL_WIDTH, projectBounds, recordPlaneBounds, revealBetween, type GardenWorldBounds } from "./gardenSpatialZoom";
import { agentMonogram } from "./agentMonogram";

interface Props {
  target: GardenEntityRef;
  bounds: GardenWorldBounds;
  camera: GardenCamera;
  label: string;
  status?: string;
  focused?: boolean;
  /** A selected child must not intercept the second click before zoom has begun. */
  revealFromScale?: number;
  viewport: { width: number; height: number };
  receding?: boolean;
  children: ReactNode;
  onSelect: () => void;
  onEnter: () => void;
}

/** A cell is part of the world: the same camera transforms its shell and contents. */
export function GardenSpatialCell({ target, bounds, camera, label, status, focused = false, revealFromScale, viewport, receding = false, children, onSelect, onEnter }: Props) {
  const plane = target.kind === "agent" ? bounds : recordPlaneBounds(bounds, revealBetween(bounds.width * camera.scale, 180, 540));
  const screen = projectBounds(plane, camera);
  const shell = revealBetween(screen.width, 70, 150);
  const activation = revealFromScale ? revealBetween(camera.scale / revealFromScale, 1, 1.25) : 1;
  const regions = revealBetween(screen.width, 120, 300);
  const nearDetail = revealBetween(screen.width, 420, 720);
  const context = target.kind === "agent" || receding ? 1 - revealBetween(screen.width, 1400, 2400) : 1;
  const detail = nearDetail * context;
  const readable = screen.width >= 540 && context > .1 && activation > .5;
  const navigable = screen.width >= 70 && context > .1;
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => { if (focused && navigable) root.current?.focus({ preventScroll: true }); }, [focused, navigable]);
  const style = {
    left: screen.x, top: screen.y, width: CELL_WIDTH, height: CELL_WIDTH * plane.height / plane.width,
    transform: `scale(${screen.width / CELL_WIDTH})`, opacity: shell * activation,
    // Coarse agent previews are visual only; Konva owns placement dragging.
    pointerEvents: activation > .5 && (target.kind !== "agent" || screen.width >= 280) ? "auto" : "none",
    "--garden-status": status, "--garden-regions": regions, "--garden-detail": detail,
    "--garden-shell-radius": target.kind === "agent" ? "50%" : "28px",
  } as CSSProperties;
  return <div ref={root} tabIndex={-1} className={`garden-spatial-cell garden-composition garden-spatial-${target.kind}`}
    data-garden-cell={`${target.kind}:${target.id}`} data-garden-detail={detail.toFixed(3)}
    data-garden-world={JSON.stringify(bounds)} style={style} aria-label={`${label} composition`}
    onClick={(event) => { if (event.target === event.currentTarget) { event.stopPropagation(); onSelect(); } }}
    onDoubleClick={(event) => { if (event.target === event.currentTarget) { event.stopPropagation(); onEnter(); } }}>
    {target.kind === "agent" && <div className="garden-spatial-nucleus-preview" aria-hidden="true" style={{
      opacity: 1 - revealBetween(screen.width, 300, 600), top: `${50 - 8 * revealBetween(screen.width, 180, 420)}%`,
      fontSize: Math.min(22, screen.width * .24) * CELL_WIDTH / screen.width,
    }}>{agentMonogram(label)}</div>}
    <div className="garden-spatial-caption" style={{ opacity: (1 - nearDetail) * context * revealBetween(screen.width, 180, 300),
      fontSize: 14 * CELL_WIDTH / screen.width }} aria-hidden="true">{label}</div>
    <div className="garden-spatial-contents" inert={!readable} aria-hidden={!readable}
      tabIndex={target.kind !== "agent" && readable ? 0 : undefined}
      role={target.kind !== "agent" ? "region" : undefined}
      aria-label={target.kind !== "agent" ? `${label} reading area` : undefined}
      style={target.kind === "agent" ? undefined : {
        width: Math.min(CELL_WIDTH - 80, Math.max(180, (viewport.width - 64) * CELL_WIDTH / screen.width)),
        left: "50%", right: "auto", transform: "translateX(-50%)",
        maxHeight: Math.max(100, (viewport.height - 170) * CELL_WIDTH / screen.width - 80),
      }}>
      {children}
    </div>
  </div>;
}
