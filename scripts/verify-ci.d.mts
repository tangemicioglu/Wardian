export interface VerificationStep {
  category: 'frontend' | 'backend' | 'docs';
  command: string;
  workflowLine: number;
}

export function readVerificationPlan(workflowText: string): VerificationStep[];
export function selectPlan(plan: VerificationStep[], only: string | null): VerificationStep[];
export function parseArgs(argv: string[]): { list: boolean; only: string | null };
export function main(argv?: string[]): number;
