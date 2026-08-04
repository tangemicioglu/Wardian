import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { AgentConfig, ProviderModelCatalog } from "../../types";

const AUTO_REFRESH_MS = 5 * 60 * 1000;

export interface ModelSelection {
  model?: string;
  reasoning_effort?: string;
}

interface ProviderModelSelectorProps {
  provider: AgentConfig["provider"];
  selection: ModelSelection;
  onSelectionChange: (selection: ModelSelection) => void;
  idPrefix: string;
  compact?: boolean;
}

export function ProviderModelSelector({
  provider,
  selection,
  onSelectionChange,
  idPrefix,
  compact = false,
}: ProviderModelSelectorProps) {
  const [catalog, setCatalog] = useState<ProviderModelCatalog | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadCatalog = useCallback(async (forceRefresh: boolean) => {
    if (!provider?.trim()) {
      setCatalog(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const nextCatalog = await invoke<ProviderModelCatalog>("list_provider_model_catalog", {
        provider,
        forceRefresh,
      });
      if (!nextCatalog || !Array.isArray(nextCatalog.models)) {
        throw new Error("Provider returned an invalid model catalogue.");
      }
      setCatalog(nextCatalog);
      setError(nextCatalog.refresh_error);
    } catch (reason) {
      setCatalog(null);
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, [provider]);

  useEffect(() => {
    void loadCatalog(false);
    const timer = window.setInterval(() => {
      void loadCatalog(true);
    }, AUTO_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [loadCatalog]);

  const models = catalog?.models ?? [];
  const selectedModel = useMemo(() => {
    if (selection.model) return models.find((model) => model.id === selection.model) ?? null;
    return models.find((model) => model.is_default) ?? models[0] ?? null;
  }, [models, selection.model]);
  const effortOptions = selectedModel?.effort_options ?? [];
  const modelValue = selection.model ?? "";
  const modelIsCurrentButUndiscovered = Boolean(
    selection.model && !models.some((model) => model.id === selection.model),
  );
  const showEffort = effortOptions.length > 0;
  const modelId = `${idPrefix}-model`;
  const effortId = `${idPrefix}-effort`;

  const chooseModel = (nextModel: string) => {
    const nextResolvedModel = nextModel
      ? models.find((model) => model.id === nextModel) ?? null
      : models.find((model) => model.is_default) ?? models[0] ?? null;
    const nextEfforts = nextResolvedModel?.effort_options ?? [];
    const nextEffort = nextEfforts.includes(selection.reasoning_effort ?? "")
      ? selection.reasoning_effort
      : nextResolvedModel
        ? nextResolvedModel.default_effort ?? undefined
        : undefined;
    onSelectionChange({
      model: nextModel || undefined,
      reasoning_effort: nextEffort,
    });
  };

  return (
    <div className={`rounded border border-wardian-light bg-[var(--color-wardian-card-bg-muted)] ${compact ? "px-2 py-1.5" : "p-3"}`}>
      <div className={compact && showEffort ? "flex gap-2" : "grid gap-2"}>
        <div className={compact && showEffort ? "min-w-0 flex-1" : "min-w-0"}>
          <label className="mb-1 block text-[10px] font-bold text-muted-neutral" htmlFor={modelId}>Model</label>
          <select
            aria-label="Model"
            className="w-full rounded border border-wardian-light bg-[var(--color-wardian-input-bg)] px-2 py-1.5 text-xs text-primary outline-none transition-colors focus:border-[var(--color-wardian-accent)] disabled:cursor-not-allowed disabled:opacity-60"
            disabled={loading || models.length === 0}
            id={modelId}
            onChange={(event) => chooseModel(event.target.value)}
            value={modelValue}
          >
            <option value="">Provider default</option>
            {modelIsCurrentButUndiscovered ? (
              <option value={selection.model}>{selection.model} (saved)</option>
            ) : null}
            {models.map((model) => (
              <option key={model.id} value={model.id}>{model.display_name}</option>
            ))}
          </select>
        </div>
        {showEffort ? (
          <div className={compact ? "w-24 shrink-0" : "min-w-0"}>
            <label className="mb-1 block text-[10px] font-bold text-muted-neutral" htmlFor={effortId}>Effort</label>
            <select
              aria-label="Effort"
              className="w-full rounded border border-wardian-light bg-[var(--color-wardian-input-bg)] px-2 py-1.5 text-xs text-primary outline-none transition-colors focus:border-[var(--color-wardian-accent)]"
              id={effortId}
              onChange={(event) => onSelectionChange({
                model: selection.model,
                reasoning_effort: event.target.value || undefined,
              })}
              value={selection.reasoning_effort ?? ""}
            >
              <option value="">Provider default</option>
              {effortOptions.map((effort) => (
                <option key={effort} value={effort}>{effort}</option>
              ))}
            </select>
          </div>
        ) : null}
      </div>
      {error ? (
        <p className="mt-1.5 text-[10px] leading-4 text-wardian-warning" role="status">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function errorMessage(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (typeof reason === "string") return reason;
  return "Unable to load provider models.";
}
