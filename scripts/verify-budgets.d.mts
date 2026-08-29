export interface Metrics {
  file_lines: Record<string, number>;
  clippy_allow_too_many_arguments?: number;
  clippy_allow_await_holding_lock?: number;
  cfg_test_functions?: number;
  ignored_rust_tests?: number;
  skipped_e2e_tests?: number;
  eslint_warnings?: number;
}

export function compareMetrics(current: Metrics, base: Metrics): {
  over: Array<[string, number, number]>;
  under: Array<[string, number, number]>;
};
export function main(argv?: string[]): number;
