import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { BlueprintListResult, BlueprintRef } from './workflowTypes';

interface BlueprintSelectorProps {
  selectedPath?: string | null;
  onOpen: (path: string) => void;
  onNew: () => void;
}

export function BlueprintSelector({ selectedPath, onOpen, onNew }: BlueprintSelectorProps) {
  const [blueprints, setBlueprints] = useState<BlueprintRef[]>([]);
  const [blueprintsTruncated, setBlueprintsTruncated] = useState(false);
  const [nextOffset, setNextOffset] = useState<number | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);

  useEffect(() => {
    void invoke<BlueprintListResult>('workflow_list_blueprints')
      .then((result) => {
        setBlueprints(result.blueprints);
        setBlueprintsTruncated(result.truncated);
        setNextOffset(result.next_offset ?? null);
      })
      .catch(() => {
        setBlueprints([]);
        setBlueprintsTruncated(false);
        setNextOffset(null);
      });
  }, []);

  const loadMore = useCallback(async () => {
    if (nextOffset === null || loadingMore) return;
    setLoadingMore(true);
    try {
      const result = await invoke<BlueprintListResult>('workflow_list_blueprints', { offset: nextOffset });
      setBlueprints((current) => {
        const byPath = new Map(current.map((blueprint) => [blueprint.path, blueprint]));
        for (const blueprint of result.blueprints) byPath.set(blueprint.path, blueprint);
        return [...byPath.values()];
      });
      setBlueprintsTruncated(result.truncated);
      setNextOffset(result.next_offset ?? null);
    } finally {
      setLoadingMore(false);
    }
  }, [loadingMore, nextOffset]);

  return (
    <div className="blueprint-selector flex items-center gap-2" data-testid="blueprint-selector" data-tour-target="workflow-blueprint-selector">
      {blueprintsTruncated && (
        <span role="status" className="inline-flex items-center gap-1 text-[10px] text-[var(--color-wardian-warning)]">
          <span>Showing the first 500 workflows; pages are capped at 500.</span>
          {nextOffset !== null && (
            <button type="button" className="underline disabled:opacity-50" onClick={() => void loadMore()} disabled={loadingMore}>
              {loadingMore ? 'Loading…' : 'Load next 500'}
            </button>
          )}
        </span>
      )}
      <select
        className="rounded border border-wardian-border bg-[var(--color-wardian-bg)] px-2 py-1 text-xs text-wardian-text"
        value={selectedPath ?? ''}
        onChange={(event) => {
          if (event.target.value) {
            onOpen(event.target.value);
          }
        }}
      >
        <option value="" disabled>
          Open blueprint...
        </option>
        {blueprints.map((blueprint) => (
          <option key={blueprint.path} value={blueprint.path}>
            {blueprint.name}
          </option>
        ))}
      </select>
      <button
        type="button"
        className="rounded border border-wardian-border px-2 py-1 text-xs text-wardian-text"
        onClick={onNew}
      >
        New
      </button>
    </div>
  );
}
