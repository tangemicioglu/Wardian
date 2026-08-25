import { useEffect, useState } from 'react';
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

  useEffect(() => {
    void invoke<BlueprintListResult | BlueprintRef[]>('workflow_list_blueprints')
      .then((result) => {
        setBlueprints(Array.isArray(result) ? result : result.blueprints);
        setBlueprintsTruncated(!Array.isArray(result) && result.truncated);
      })
      .catch(() => {
        setBlueprints([]);
        setBlueprintsTruncated(false);
      });
  }, []);

  return (
    <div className="blueprint-selector flex items-center gap-2" data-testid="blueprint-selector" data-tour-target="workflow-blueprint-selector">
      {blueprintsTruncated && (
        <span role="status" className="text-[10px] text-[var(--color-wardian-warning)]">
          Showing the first 500 workflows.
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
