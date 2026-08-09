import { useEffect, useRef, useState } from "react";
import type { FileRendererProps } from "./rendererRegistry";

export default function UnsupportedRenderer({
  snapshot,
  lifecycle,
  on_open_with,
  on_open_system,
  on_reveal,
}: FileRendererProps) {
  const { descriptor } = snapshot;
  const [openError, setOpenError] = useState<string | null>(null);
  const attemptedOpenRef = useRef<string | null>(null);
  const liveDocument = descriptor.mime_type === "text/html"
    || descriptor.mime_type === "image/svg+xml";
  const reason = descriptor.unavailable_reason
    ?? (liveDocument ? "live_renderer_not_activated" : "renderer_not_activated");

  useEffect(() => {
    if (
      !on_open_system
      || !lifecycle.visible
      || descriptor.unavailable_reason !== "unsupported_content"
    ) return;
    const attemptKey = `${snapshot.resource_id}:${snapshot.revision}`;
    if (attemptedOpenRef.current === attemptKey) return;
    attemptedOpenRef.current = attemptKey;
    void Promise.resolve(on_open_system(descriptor.canonical_path)).catch((error) => {
      setOpenError(error instanceof Error ? error.message : String(error));
    });
  }, [descriptor.canonical_path, descriptor.unavailable_reason, lifecycle.visible, on_open_system, snapshot.resource_id, snapshot.revision]);

  if (on_open_system && descriptor.unavailable_reason === "unsupported_content" && !openError) {
    return (
      <section className="files-resource-state" role="status" aria-label="Opening in system viewer">
        <h2>Opening in system viewer</h2>
        <p>{descriptor.display_name} is not supported by Wardian.</p>
      </section>
    );
  }

  return (
    <section className="files-resource-state" role={openError ? "alert" : "status"} aria-label="Preview unavailable">
      <h2>Preview unavailable</h2>
      <p>{openError ? `System viewer could not open this file: ${openError}` : reason}</p>
      <dl className="files-resource-metadata">
        <div><dt>Type</dt><dd>{descriptor.mime_type}</dd></div>
        <div><dt>Size</dt><dd>{descriptor.size_bytes.toLocaleString()} bytes</dd></div>
      </dl>
      <div className="files-resource-actions">
        <button type="button" onClick={() => void on_open_with(descriptor.canonical_path)}>
          Open With
        </button>
        <button type="button" onClick={() => void on_reveal(descriptor.canonical_path)}>
          Reveal
        </button>
      </div>
    </section>
  );
}
