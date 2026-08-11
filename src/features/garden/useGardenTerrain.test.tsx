import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { quantizeScale, useGardenTerrain } from "./useGardenTerrain";
import type { TerrainDistrict } from "./terrain";
import type { TerrainViewport } from "./terrainFrontier";

const invokeMock = vi.mocked(invoke);
const listenMock = vi.mocked(listen);

const DISTRICTS = new Map<string, TerrainDistrict>([
  ["workspace:d:/work/repo", { roots: ["d:/work/repo"], origin: { x: 0, y: 0 }, radius: 400 }],
]);

/** Wide enough that the root cell clears the expansion threshold at scale 1. */
const VIEWPORT: TerrainViewport = {
  world: { x: -600, y: -600, width: 1200, height: 1200 },
  scale: 1,
};

function directoryTree(...names: string[]) {
  return names.map((name) => ({
    name,
    path: `D:\\work\\repo\\${name}`,
    is_dir: false,
    extension: null,
  }));
}

beforeEach(() => {
  vi.clearAllMocks();
  listenMock.mockResolvedValue(() => {});
  invokeMock.mockResolvedValue(undefined);
});

describe("quantizeScale", () => {
  it("snaps to sqrt(2) steps so detail does not churn on a wheel tick", () => {
    expect(quantizeScale(1)).toBeCloseTo(1, 6);
    expect(quantizeScale(1.01)).toBeCloseTo(1, 6);
    expect(quantizeScale(1.4)).toBeCloseTo(Math.SQRT2, 6);
    expect(quantizeScale(2)).toBeCloseTo(2, 6);
  });

  it("falls back to 1 for a degenerate scale", () => {
    expect(quantizeScale(0)).toBe(1);
    expect(quantizeScale(Number.NaN)).toBe(1);
  });
});

describe("useGardenTerrain", () => {
  it("draws nothing until the canvas reports a viewport", () => {
    const { result } = renderHook(() =>
      useGardenTerrain({ enabled: true, districts: DISTRICTS, viewport: null }),
    );
    expect(result.current.cells).toEqual([]);
    expect(invokeMock).not.toHaveBeenCalledWith("get_directory_tree", expect.anything());
  });

  it("draws the root as truncated ground before any listing arrives", () => {
    const { result } = renderHook(() =>
      useGardenTerrain({ enabled: true, districts: DISTRICTS, viewport: VIEWPORT }),
    );
    expect(result.current.cells).toHaveLength(1);
    expect(result.current.cells[0]).toMatchObject({ name: "repo", truncated: true });
  });

  it("lists a visible root and subdivides it", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "get_directory_tree" ? directoryTree("a.ts", "b.ts") : undefined,
    );

    const { result } = renderHook(() =>
      useGardenTerrain({ enabled: true, districts: DISTRICTS, viewport: VIEWPORT }),
    );

    await waitFor(() => expect(result.current.cells.length).toBe(3));
    expect(result.current.cells.filter((cell) => cell.depth === 1).map((cell) => cell.name)).toEqual(
      ["a.ts", "b.ts"],
    );
    // Backslashes from the OS are normalized into the map's one keyspace.
    expect(result.current.cells[1].path).toBe("d:/work/repo/a.ts");
  });

  it("does not list anything when the root is too small on screen", async () => {
    invokeMock.mockImplementation(async (command: string) =>
      command === "get_directory_tree" ? directoryTree("a.ts") : undefined,
    );

    renderHook(() =>
      useGardenTerrain({
        enabled: true,
        districts: DISTRICTS,
        viewport: { world: VIEWPORT.world, scale: 0.02 },
      }),
    );

    await new Promise((resolve) => setTimeout(resolve, 260));
    expect(invokeMock).not.toHaveBeenCalledWith("get_directory_tree", expect.anything());
  });

  it("watches every root and releases the watch on unmount", async () => {
    const { unmount } = renderHook(() =>
      useGardenTerrain({ enabled: true, districts: DISTRICTS, viewport: VIEWPORT }),
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("explorer_watch", { rootPath: "d:/work/repo" }),
    );
    unmount();
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("explorer_unwatch", { rootPath: "d:/work/repo" }),
    );
  });

  it("re-lists a root after explorer-changed rather than polling for it", async () => {
    type ExplorerChanged = (event: {
      payload: { root_path: string; changed_paths: string[] };
    }) => void;
    // Held in an object rather than a `let`: TypeScript narrows a closure-
    // assigned local to `never` after the null check below.
    const watcher: { emit: ExplorerChanged | null } = { emit: null };
    listenMock.mockImplementation(async (_event: string, handler: unknown) => {
      watcher.emit = handler as ExplorerChanged;
      return () => {};
    });
    invokeMock.mockImplementation(async (command: string) =>
      command === "get_directory_tree" ? directoryTree("a.ts") : undefined,
    );

    const { result } = renderHook(() =>
      useGardenTerrain({ enabled: true, districts: DISTRICTS, viewport: VIEWPORT }),
    );
    await waitFor(() => expect(result.current.cells.length).toBe(2));
    const listingCalls = invokeMock.mock.calls.filter(([command]) => command === "get_directory_tree");
    expect(listingCalls).toHaveLength(1);

    invokeMock.mockImplementation(async (command: string) =>
      command === "get_directory_tree" ? directoryTree("a.ts", "c.ts") : undefined,
    );
    await waitFor(() => expect(watcher.emit).not.toBeNull());
    watcher.emit?.({ payload: { root_path: "D:\\work\\repo", changed_paths: [] } });

    await waitFor(() => expect(result.current.cells.length).toBe(3));
  });

  it("refreshes only the folder a change touched, and never blanks the ground", async () => {
    // The bug this locks: a write anywhere in the repo used to evict every
    // listing under the root, collapsing the district to a bare square for a
    // debounce plus a round trip. An agent writing steadily kept it collapsed.
    type ExplorerChanged = (event: {
      payload: { root_path: string; changed_paths: string[] };
    }) => void;
    const watcher: { emit: ExplorerChanged | null } = { emit: null };
    listenMock.mockImplementation(async (_event: string, handler: unknown) => {
      watcher.emit = handler as ExplorerChanged;
      return () => {};
    });

    let srcChildren = ["a.ts"];
    invokeMock.mockImplementation(async (command: string, args?: unknown) => {
      if (command !== "get_directory_tree") return undefined;
      const path = (args as { path: string }).path;
      if (path === "d:/work/repo") {
        return [
          { name: "src", path: "D:\\work\\repo\\src", is_dir: true, extension: null },
        ];
      }
      return srcChildren.map((name) => ({
        name,
        path: `D:\\work\\repo\\src\\${name}`,
        is_dir: false,
        extension: null,
      }));
    });

    const lengths: number[] = [];
    const { result } = renderHook(() => {
      const terrain = useGardenTerrain({
        enabled: true,
        districts: DISTRICTS,
        viewport: VIEWPORT,
      });
      lengths.push(terrain.cells.length);
      return terrain;
    });

    await waitFor(() => expect(result.current.cells.length).toBe(3));
    const listed = () =>
      invokeMock.mock.calls
        .filter(([command]) => command === "get_directory_tree")
        .map(([, args]) => (args as { path: string }).path);
    expect(listed()).toEqual(["d:/work/repo", "d:/work/repo/src"]);
    const settled = lengths.length;

    srcChildren = ["a.ts", "b.ts"];
    await waitFor(() => expect(watcher.emit).not.toBeNull());
    await act(async () => {
      watcher.emit?.({
        payload: { root_path: "D:\\work\\repo", changed_paths: ["D:\\work\\repo\\src\\b.ts"] },
      });
    });

    await waitFor(() => expect(result.current.cells.length).toBe(4));
    // Only the listing that names `b.ts` was re-read; the root was left alone.
    expect(listed().slice(2)).toEqual(["d:/work/repo/src"]);
    // And no render between the event and the new listing lost ground — the old
    // children keep drawing until the replacement arrives.
    expect(Math.min(...lengths.slice(settled))).toBeGreaterThanOrEqual(3);
  });

  it("retries an unreadable directory once, not on every evaluation", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_directory_tree") throw new Error("EACCES");
      return undefined;
    });

    const { rerender } = renderHook(
      (props: { viewport: TerrainViewport }) =>
        useGardenTerrain({ enabled: true, districts: DISTRICTS, viewport: props.viewport }),
      { initialProps: { viewport: VIEWPORT } },
    );

    await waitFor(() =>
      expect(
        invokeMock.mock.calls.filter(([command]) => command === "get_directory_tree"),
      ).toHaveLength(1),
    );

    rerender({ viewport: { ...VIEWPORT, world: { ...VIEWPORT.world, x: -601 } } });
    await new Promise((resolve) => setTimeout(resolve, 260));
    expect(
      invokeMock.mock.calls.filter(([command]) => command === "get_directory_tree"),
    ).toHaveLength(1);
  });

  it("drops the listings of a root that has left the roster", async () => {
    // The frontier budget counts cached directories, and nothing used to remove
    // them. A district that was removed or re-districted kept its share forever,
    // so once the cache filled with departed roots a newly added root could
    // never expand past its own root cell.
    invokeMock.mockImplementation(async (command: string, args?: unknown) =>
      command === "get_directory_tree" &&
      String((args as { path?: string } | undefined)?.path ?? "").includes("repo")
        ? directoryTree("a.ts", "b.ts")
        : [],
    );

    const { result, rerender } = renderHook(
      ({ districts }) => useGardenTerrain({ enabled: true, districts, viewport: VIEWPORT }),
      { initialProps: { districts: DISTRICTS } },
    );
    await waitFor(() => expect(result.current.cells.length).toBe(3));

    const listedRepo = () =>
      invokeMock.mock.calls.filter(
        ([command, args]) =>
          command === "get_directory_tree" &&
          (args as unknown as { path?: string } | undefined)?.path === "d:/work/repo",
      ).length;
    expect(listedRepo()).toBe(1);

    const replaced = new Map<string, TerrainDistrict>([
      [
        "workspace:d:/work/other",
        { roots: ["d:/work/other"], origin: { x: 0, y: 0 }, radius: 400 },
      ],
    ]);
    rerender({ districts: replaced });
    await waitFor(() =>
      expect(result.current.cells.map((cell) => cell.path)).toEqual(["d:/work/other"]),
    );

    // Coming back must re-list, which is only true if the departed root's
    // listing was actually dropped. Asserting on `cells` alone would pass
    // either way — cells derive from the current districts, so a retained
    // listing is invisible there while still holding frontier budget.
    rerender({ districts: DISTRICTS });
    await waitFor(() => expect(listedRepo()).toBe(2));
  });

  it("discards a listing that lands after its root has left the roster", async () => {
    // Pruning runs when the root set changes, but a listing outlives the render
    // that asked for it. Committing it unconditionally reinstated the entry
    // after the prune, where it held frontier budget and — being treated as
    // cached — suppressed the refetch when that root came back.
    let release: (nodes: unknown) => void = () => {};
    invokeMock.mockImplementation(
      (command: string) =>
        command === "get_directory_tree"
          ? new Promise((resolve) => {
              release = resolve;
            })
          : Promise.resolve(undefined) as Promise<unknown>,
    );

    const { result, rerender } = renderHook(
      ({ districts }) => useGardenTerrain({ enabled: true, districts, viewport: VIEWPORT }),
      { initialProps: { districts: DISTRICTS } },
    );
    await waitFor(() => expect(invokeMock).toHaveBeenCalled());

    const replaced = new Map<string, TerrainDistrict>([
      [
        "workspace:d:/work/other",
        { roots: ["d:/work/other"], origin: { x: 0, y: 0 }, radius: 400 },
      ],
    ]);
    rerender({ districts: replaced });
    await act(async () => {
      release(directoryTree("a.ts", "b.ts"));
    });

    // Coming back must still re-list: the late result was dropped rather than
    // reinstated behind the prune.
    const listedRepo = () =>
      invokeMock.mock.calls.filter(
        ([command, args]) =>
          command === "get_directory_tree" &&
          (args as unknown as { path?: string } | undefined)?.path === "d:/work/repo",
      ).length;
    const before = listedRepo();
    rerender({ districts: DISTRICTS });
    await waitFor(() => expect(listedRepo()).toBe(before + 1));
    expect(result.current.cells.length).toBeGreaterThan(0);
  });

  it("re-reads a folder the filesystem changed while its listing was in flight", async () => {
    // Two guards that are each right alone and lose the event together:
    // `staleListings` cannot name a listing that has not arrived, and
    // `requestListings` skips a path already in flight. The read then commits a
    // snapshot older than the change, and nothing retries — the next attempt
    // would only come from another write to the same folder.
    type ExplorerChanged = (event: {
      payload: { root_path: string; changed_paths: string[] };
    }) => void;
    const watcher: { emit: ExplorerChanged | null } = { emit: null };
    listenMock.mockImplementation(async (_event: string, handler: unknown) => {
      watcher.emit = handler as ExplorerChanged;
      return () => {};
    });

    const releases: Array<(nodes: unknown) => void> = [];
    invokeMock.mockImplementation(
      (command: string) =>
        command === "get_directory_tree"
          ? new Promise((resolve) => {
              releases.push(resolve);
            })
          : (Promise.resolve(undefined) as Promise<unknown>),
    );

    const { result } = renderHook(() =>
      useGardenTerrain({ enabled: true, districts: DISTRICTS, viewport: VIEWPORT }),
    );

    const listedRepo = () =>
      invokeMock.mock.calls.filter(
        ([command, args]) =>
          command === "get_directory_tree" &&
          (args as unknown as { path?: string } | undefined)?.path === "d:/work/repo",
      ).length;
    await waitFor(() => expect(listedRepo()).toBe(1));
    await waitFor(() => expect(watcher.emit).not.toBeNull());

    // The change lands mid-read, naming a file inside the folder being listed.
    watcher.emit?.({
      payload: {
        root_path: "D:\\work\\repo",
        changed_paths: ["D:\\work\\repo\\a.ts"],
      },
    });

    await act(async () => {
      releases[0]?.(directoryTree("a.ts"));
    });

    await waitFor(() => expect(listedRepo()).toBe(2));
    await act(async () => {
      releases[1]?.(directoryTree("a.ts", "b.ts"));
    });
    await waitFor(() => expect(result.current.cells.length).toBe(3));
  });

  it("retries a root that failed before it left the roster and came back", async () => {
    // `failed` suppresses the frontier request and is cleared only by an
    // `explorer-changed` event naming that exact path — which cannot arrive once
    // the root leaves, because its watcher goes with it. The root returned as a
    // square that could never expand again.
    let failNext = true;
    invokeMock.mockImplementation(async (command: string) => {
      if (command !== "get_directory_tree") return undefined;
      if (failNext) throw new Error("unreadable");
      return directoryTree("a.ts", "b.ts");
    });

    const { result, rerender } = renderHook(
      ({ districts }) => useGardenTerrain({ enabled: true, districts, viewport: VIEWPORT }),
      { initialProps: { districts: DISTRICTS } },
    );

    const listedRepo = () =>
      invokeMock.mock.calls.filter(
        ([command, args]) =>
          command === "get_directory_tree" &&
          (args as unknown as { path?: string } | undefined)?.path === "d:/work/repo",
      ).length;
    await waitFor(() => expect(listedRepo()).toBe(1));

    const replaced = new Map<string, TerrainDistrict>([
      [
        "workspace:d:/work/other",
        { roots: ["d:/work/other"], origin: { x: 0, y: 0 }, radius: 400 },
      ],
    ]);
    rerender({ districts: replaced });
    await waitFor(() => expect(result.current.cells.map((cell) => cell.path)).toEqual([
      "d:/work/other",
    ]));

    failNext = false;
    rerender({ districts: DISTRICTS });
    await waitFor(() => expect(listedRepo()).toBe(2));
    await waitFor(() => expect(result.current.cells.length).toBe(3));
  });

  it("draws nothing and watches nothing while disabled", () => {
    const { result } = renderHook(() =>
      useGardenTerrain({ enabled: false, districts: DISTRICTS, viewport: VIEWPORT }),
    );
    expect(result.current.cells).toEqual([]);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
