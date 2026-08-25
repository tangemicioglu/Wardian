export interface BlueprintRef {
  id: string;
  name: string;
  path: string;
}

export interface BlueprintListResult {
  blueprints: BlueprintRef[];
  truncated: boolean;
}
