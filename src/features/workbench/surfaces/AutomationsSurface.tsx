import {
  AutomationsView,
  type AutomationsViewProps,
} from "../../../views/AutomationsView";
import type { SurfaceVisibility } from "./coreSurfaceMetadata";

export interface AutomationsSurfaceProps extends AutomationsViewProps {
  surface_id: string;
  visibility?: SurfaceVisibility;
}

/** Typed workbench presentation wrapper; the builder store remains the resource owner. */
export function AutomationsSurface({ surface_id, visibility = "visible", ...viewProps }: AutomationsSurfaceProps) {
  const hidden = visibility === "hidden";

  return (
    <section
      aria-hidden={hidden}
      className="h-full min-h-0 min-w-0"
      data-surface-id={surface_id}
      data-surface-visibility={visibility}
      data-testid="automations-surface"
      style={hidden ? { display: "none" } : undefined}
    >
      <AutomationsView {...viewProps} />
    </section>
  );
}
