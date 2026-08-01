import type { WorkbenchSurfaceV1 } from "../../types";

export type WorkbenchMruSwitcherProps = {
  surface_ids: readonly string[];
  selected_index: number;
  surfaces: Readonly<Record<string, WorkbenchSurfaceV1>>;
  title_for_surface: (surface: WorkbenchSurfaceV1) => string;
};

/** A non-modal preview that commits the selected tab when the primary modifier is released. */
export function WorkbenchMruSwitcher({
  surface_ids,
  selected_index,
  surfaces,
  title_for_surface: titleForSurface,
}: WorkbenchMruSwitcherProps) {
  const primaryModifier = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform)
    ? "⌘"
    : "Ctrl";
  const entries = surface_ids.flatMap((surfaceId) => {
    const surface = surfaces[surfaceId];
    return surface ? [surface] : [];
  });

  if (entries.length < 2) return null;

  return (
    <section
      role="status"
      aria-live="polite"
      aria-label="Recent tabs"
      className="wardian-workbench-mru-switcher"
    >
      <header>
        <span>Recent tabs in this pane</span>
        <span>Release <kbd>{primaryModifier}</kbd> to switch</span>
      </header>
      <div role="listbox" aria-label="Recent tabs">
        {entries.map((surface, index) => (
          <div
            key={surface.surface_id}
            role="option"
            aria-selected={index === selected_index}
            className="wardian-workbench-mru-switcher-option"
          >
            {titleForSurface(surface)}
          </div>
        ))}
      </div>
    </section>
  );
}
