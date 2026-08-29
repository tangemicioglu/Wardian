import { describe, expect, it } from "vitest";
import type { Blueprint } from "../automations/builder/blueprintTypes";
import { libraryFolderOf, automationContextOf } from "./automationContext";

function blueprint(nodes: Blueprint["nodes"]): Blueprint {
  return { schema: 2, id: "wf", name: "WF", nodes, edges: [] };
}

describe("automationContextOf", () => {
  it("reads a shell node's working directory, which is where the automation acts", () => {
    // Shape taken from a real blueprint: a manual trigger, a shell command with
    // a cwd, and notifications. It binds no agent at all, so the path is the
    // only thing that can place it.
    const context = automationContextOf(
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
    const context = automationContextOf(
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
    const context = automationContextOf(
      blueprint([
        { id: "c1", type: "shell", fields: { command: "C:\\Windows\\System32\\cmd.exe" } },
      ]),
    );
    expect(context.workspacePaths).toEqual([]);
  });

  it("ignores an unknown node type instead of inventing evidence", () => {
    const context = automationContextOf(
      blueprint([{ id: "x", type: "not-a-real-type", fields: { cwd: "D:\\Somewhere" } }]),
    );
    expect(context.workspacePaths).toEqual([]);
  });

  it("is stable under node reordering", () => {
    const nodes: Blueprint["nodes"] = [
      { id: "a", type: "shell", fields: { cwd: "D:\\B" } },
      { id: "b", type: "shell", fields: { cwd: "D:\\A" } },
    ];
    expect(automationContextOf(blueprint(nodes)).workspacePaths).toEqual(
      automationContextOf(blueprint([...nodes].reverse())).workspacePaths,
    );
  });
});

describe("libraryFolderOf", () => {
  it("takes the folder a blueprint sits in below the automations section", () => {
    expect(libraryFolderOf("C:/Users/x/.wardian/library/automations/trident/trident-alerts.md")).toBe(
      "trident",
    );
    expect(libraryFolderOf("C:\\Users\\x\\.wardian\\library\\automations\\trident\\sync-ld.md")).toBe(
      "trident",
    );
  });

  it("reports no folder for a blueprint in the section root", () => {
    // The root is not a grouping; treating it as one would tie every loose
    // blueprint to every other.
    expect(libraryFolderOf("C:/x/library/automations/autoreview.md")).toBeNull();
    expect(libraryFolderOf(undefined)).toBeNull();
    expect(libraryFolderOf("C:/x/somewhere/else.md")).toBeNull();
  });

  it("keeps nested folders distinct", () => {
    expect(libraryFolderOf("/w/library/automations/trident/scans/orb15.md")).toBe("trident/scans");
  });
});

describe("an agent_ref is three different things", () => {
  // Shape taken from the real evolver blueprints: every task node binds
  // `role:evolver`, and nothing in the file names an agent.
  const evolver = blueprint([
    { id: "trigger-1", type: "manual_trigger" },
    { id: "agent-scan", type: "task", fields: { agent: "role:evolver", prompt: "Scan." } },
    { id: "agent-triage", type: "task", fields: { agent: "role:evolver", prompt: "Triage." } },
  ]);

  it("does not mistake an unfilled role for an agent id", () => {
    // This is what put the Evolver's automations in the commons: `role:evolver`
    // was read as an agent id, resolved against no agent, and left the automation
    // with no tie to anywhere. A role says what kind of agent the automation
    // needs; which agent actually runs it is decided elsewhere.
    const context = automationContextOf(evolver);
    expect(context.agentIds).toEqual([]);
    expect(context.roleNames).toEqual(["evolver"]);
  });

  it("keeps a class requirement apart from a binding too", () => {
    const context = automationContextOf(
      blueprint([
        { id: "a", type: "task", fields: { agent: "class:Coder", prompt: "Build." } },
        { id: "b", type: "task", fields: { agent: "agent-7", prompt: "Ship." } },
      ]),
    );
    expect(context.classNames).toEqual(["Coder"]);
    expect(context.agentIds).toEqual(["agent-7"]);
    expect(context.roleNames).toEqual([]);
  });

  it("ties an automation to nobody when its agent is ephemeral", () => {
    const context = automationContextOf(
      blueprint([{ id: "a", type: "task", fields: { agent: "ephemeral", prompt: "Once." } }]),
    );
    expect(context.agentIds).toEqual([]);
    expect(context.roleNames).toEqual([]);
    expect(context.classNames).toEqual([]);
  });

  it("de-duplicates and sorts, so node order cannot move the automation", () => {
    const context = automationContextOf(
      blueprint([
        { id: "a", type: "task", fields: { agent: "role:zeta", prompt: "p" } },
        { id: "b", type: "task", fields: { agent: "role:alpha", prompt: "p" } },
        { id: "c", type: "task", fields: { agent: "role:zeta", prompt: "p" } },
      ]),
    );
    expect(context.roleNames).toEqual(["alpha", "zeta"]);
  });

  it("ignores a role that is only a prefix", () => {
    const context = automationContextOf(
      blueprint([{ id: "a", type: "task", fields: { agent: "role:", prompt: "p" } }]),
    );
    expect(context.roleNames).toEqual([]);
    expect(context.agentIds).toEqual([]);
  });
});
