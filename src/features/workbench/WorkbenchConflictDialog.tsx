export type WorkbenchConflictDialogProps = {
  mode: "revision_conflict" | "future_schema";
  resolving?: boolean;
  on_use_disk: () => void | Promise<void>;
  on_replace_disk: () => void | Promise<void>;
  on_export_local: () => void;
};

/** Explicit, non-merging recovery choices for a frozen workbench draft. */
export function WorkbenchConflictDialog({
  mode,
  resolving = false,
  on_use_disk,
  on_replace_disk,
  on_export_local,
}: WorkbenchConflictDialogProps) {
  const futureSchema = mode === "future_schema";
  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="workbench-conflict-title"
      className="wardian-dialog-overlay fixed inset-0 z-50 flex items-center justify-center p-4"
    >
      <section
        className="wardian-dialog-panel wardian-dialog-panel--standard w-full p-5"
      >
        <h2 id="workbench-conflict-title" className="text-lg font-semibold">
          {futureSchema ? "Newer workbench version" : "Workbench changed on disk"}
        </h2>
        <p
          className="mt-2 text-sm"
          style={{ color: "var(--color-wardian-text-muted)" }}
        >
          {futureSchema
            ? "This workbench is read-only in this Wardian version. Export the local draft before upgrading."
            : "Saving is paused. Choose which complete document to keep; Wardian will not merge layouts automatically."}
        </p>
        <div className="mt-5 flex flex-wrap justify-end gap-2">
          {!futureSchema && (
            <>
              <button
                type="button"
              className="wardian-button wardian-button--secondary"
                disabled={resolving}
                onClick={() => void on_use_disk()}
              >
                Use Disk
              </button>
              <button
                type="button"
                className="wardian-button wardian-button--primary"
                disabled={resolving}
                onClick={() => void on_replace_disk()}
              >
                Replace Disk
              </button>
            </>
          )}
          <button
            type="button"
            className="wardian-button wardian-button--secondary"
            disabled={resolving}
            onClick={on_export_local}
          >
            Export Local JSON
          </button>
        </div>
      </section>
    </div>
  );
}
