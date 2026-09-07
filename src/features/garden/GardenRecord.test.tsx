import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { GardenRecord } from "./GardenRecord";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../files/useFileResource", () => ({ useFileResource: () => ({ snapshot: null, error: null, retry: vi.fn() }) }));
const actions = { onOpenAgent: vi.fn(), onOpenSkill: vi.fn(), onOpenPath: vi.fn() };
beforeEach(() => { vi.mocked(invoke).mockReset(); vi.clearAllMocks(); });

it("reads skills using the Library-relative path and exposes its canonical action", async () => {
  vi.mocked(invoke).mockResolvedValue("# Planning\nEvidence first.");
  render(<GardenRecord target={{ kind: "skill", id: "skills/dev/planning" }} {...actions} />);
  expect(await screen.findByText(/Evidence first/)).toBeVisible();
  expect(invoke).toHaveBeenCalledWith("read_library_item", { section: "skills", path: "dev/planning" });
  fireEvent.click(screen.getByRole("button", { name: "Open in Library" }));
  expect(actions.onOpenSkill).toHaveBeenCalledWith("skills/dev/planning");
});

it("renders memory scope, evidence and revisions from canonical commands", async () => {
  const memory = { text: "Prefer focused checks", workspace: "/work", kind: "stable", status: "active", revision: 2, revision_id: "rev2", last_verified_at: "2026-09-07", evidence_excerpt: "Run focused verification", sources: [{ source_type: "conversation", locator: "turn-2" }] };
  vi.mocked(invoke).mockImplementation(async (command) => command === "memory_history" ? [memory] : memory);
  render(<GardenRecord target={{ kind: "memory", id: "m1" }} {...actions} />);
  expect(await screen.findByText("Scope")).toBeVisible();
  expect(screen.getByText("/work")).toBeVisible();
  expect(screen.getAllByText("Run focused verification")).toHaveLength(2);
  expect(invoke).toHaveBeenCalledWith("memory_get", { memoryId: "m1" });
  expect(invoke).toHaveBeenCalledWith("memory_history", { memoryId: "m1" });
});

it("does not present an unavailable record as an empty successful read and retries", async () => {
  vi.mocked(invoke).mockRejectedValueOnce(new Error("Permission denied")).mockResolvedValue("Recovered content");
  render(<GardenRecord target={{ kind: "skill", id: "skills/plan" }} {...actions} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Permission denied");
  fireEvent.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("Recovered content")).toBeVisible();
});
