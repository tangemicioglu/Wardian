import { describe, expect, it } from "vitest";
import type { Blueprint } from "../workflows/builder/blueprintTypes";
import { libraryFolderOf, workflowContextOf } from "./workflowContext";

function blueprint(nodes: Blueprint["nodes"]): Blueprint {
  return { schema: 2, id: "wf", name: "WF", nodes, edges: [] };
}

describe("workflowContextOf", () => {
  it("reads a shell node's working directory, which is where the workflow acts", () => {
    // Shape taken from a real blueprint: a manual trigger, a shell command with
    // a cwd, and notifications. It binds no agent at all, so the path is the
    // only thing that can place it.
    const context = workflowContextOf(
      blueprint([
        { id: "trigger-1", type: "manual_trigger" },
        {
          id: "command-1",
          type: "shell",
          fields: { command: "python .\\alerts.py", cwd: "D:\\Trading\\trident" },
        },
        { id: "notify-1", type: "notify", fields: { message: "Trident server started." } },
      ]),
    );
    expect(context.workspacePaths).toEqual(["D:\\Trading\\trident"]);
    expect(context.agentIds).toEqual([]);
  });

  it("reads agent bindings from agent_ref fields", () => {
    const context = workflowContextOf(
      blueprint([
        { id: "t1", type: "task", fields: { agent: "agent-a", prompt: "do it" } },
        { id: "d1", type: "decision", fields: { agent: "agent-b" } },
      ]),
    );
    expect(context.agentIds).toEqual(["agent-a", "agent-b"]);
  });

  it("takes field kinds from the registry rather than guessing at names", () => {
    // `command` is text on a shell node and must not be read as a directory,
    // even though it contains something path-shaped.
    const context = workflowContextOf(
      blueprint([
        { id: "c1", type: "shell", fields: { command: "C:\\Windows\\System32\\cmd.exe" } },
      ]),
    );
    expect(context.workspacePaths).toEqual([]);
  });

  it("ignores an unknown node type instead of inventing evidence", () => {
    const context = workflowContextOf(
      blueprint([{ id: "x", type: "not-a-real-type", fields: { cwd: "D:\\Somewhere" } }]),
    );
    expect(context.workspacePaths).toEqual([]);
  });

  it("is stable under node reordering", () => {
    const nodes: Blueprint["nodes"] = [
      { id: "a", type: "shell", fields: { cwd: "D:\\B" } },
      { id: "b", type: "shell", fields: { cwd: "D:\\A" } },
    ];
    expect(workflowContextOf(blueprint(nodes)).workspacePaths).toEqual(
      workflowContextOf(blueprint([...nodes].reverse())).workspacePaths,
    );
  });
});

describe("libraryFolderOf", () => {
  it("takes the folder a blueprint sits in below the workflows section", () => {
    expect(libraryFolderOf("C:/Users/x/.wardian/library/workflows/trident/trident-alerts.md")).toBe(
      "trident",
    );
    expect(libraryFolderOf("C:\\Users\\x\\.wardian\\library\\workflows\\trident\\sync-ld.md")).toBe(
      "trident",
    );
  });

  it("reports no folder for a blueprint in the section root", () => {
    // The root is not a grouping; treating it as one would tie every loose
    // blueprint to every other.
    expect(libraryFolderOf("C:/x/library/workflows/autoreview.md")).toBeNull();
    expect(libraryFolderOf(undefined)).toBeNull();
    expect(libraryFolderOf("C:/x/somewhere/else.md")).toBeNull();
  });

  it("keeps nested folders distinct", () => {
    expect(libraryFolderOf("/w/library/workflows/trident/scans/orb15.md")).toBe("trident/scans");
  });
});
