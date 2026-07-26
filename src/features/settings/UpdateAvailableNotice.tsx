import type { AppUpdateInfo } from "./useAppUpdate";

interface UpdateAvailableNoticeProps {
  update: AppUpdateInfo;
  onReview: () => void;
  onDismiss: () => void;
}

export const UpdateAvailableNotice = ({ update, onReview, onDismiss }: UpdateAvailableNoticeProps) => (
  <section
    aria-live="polite"
    className="fixed right-4 top-12 z-40 flex max-w-md items-center gap-3 rounded border border-wardian-border bg-wardian-card px-3 py-2 text-sm text-primary shadow-lg"
    data-testid="update-available-notice"
    role="status"
  >
    <p className="min-w-0 flex-1">Wardian {update.version} is available.</p>
    <button
      className="shrink-0 rounded px-2 py-1 text-sm font-medium text-[var(--color-wardian-accent)] hover:bg-wardian-card-bg-muted"
      onClick={onReview}
      type="button"
    >
      Review update
    </button>
    <button
      aria-label="Dismiss update notice"
      className="shrink-0 rounded px-2 py-1 text-sm text-muted-neutral hover:bg-wardian-card-bg-muted hover:text-primary"
      onClick={onDismiss}
      type="button"
    >
      Later
    </button>
  </section>
);
