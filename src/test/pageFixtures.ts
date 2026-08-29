/**
 * Page-shaped fixtures for the bounded list commands.
 *
 * `get_directory_tree`, `automation_list_blueprints`, `automation_list_runs`,
 * `list_inbox_notifications` and `get_pair_activity` each return one struct
 * carrying its collection plus `truncated` and `next_offset`. None of them
 * returns a bare array.
 *
 * The frontend used to branch on `Array.isArray` at every call site to tolerate
 * both shapes. Nothing produced the array shape except fixtures like these, so
 * the branch existed to keep stale mocks working. It is gone; use these helpers
 * instead of hand-writing the wrapper, so a fixture cannot drift back into a
 * shape the backend cannot emit.
 *
 * `scripts/verify-page-fixtures.mjs` fails the build on a mock that returns a
 * bare array for one of these commands.
 */

function page<K extends string, T>(key: K, items: T[], next: number | null) {
  return { [key]: items, truncated: next !== null, next_offset: next } as
    & Record<K, T[]>
    & { truncated: boolean; next_offset: number | null };
}

/** `get_directory_tree` */
export const dirPage = <T>(nodes: T[], next: number | null = null) => page("nodes", nodes, next);

/** `automation_list_blueprints` */
export const blueprintPage = <T>(blueprints: T[], next: number | null = null) =>
  page("blueprints", blueprints, next);

/** `automation_list_runs` */
export const runPage = <T>(runs: T[], next: number | null = null) => page("runs", runs, next);

/** `list_inbox_notifications` */
export const notificationPage = <T>(notifications: T[], next: number | null = null) =>
  page("notifications", notifications, next);

/** `get_pair_activity` */
export const pairPage = <T>(pairs: T[], next: number | null = null) => page("pairs", pairs, next);
