import { beforeEach, describe, expect, it, vi } from "vitest";

import { useLibraryStore } from "../../../store/useLibraryStore";
import { useBuilderStore } from "../../../store/useBuilderStore";
import { createCoreWorkbenchSurfaceRegistry } from "../coreSurfaceRegistry";
import { createWorkbenchNavigationService } from "../navigationService";
import { createWorkbenchStore } from "../useWorkbenchStore";
import { makeSingleGroupDocument, makeSurface } from "../workbenchTestUtils";
import {
  createLibrarySurfaceCloseAdapter,
  createAutomationsSurfaceCloseAdapter,
  type DirtySurfacePrompt,
} from "./dirtySurfaceGuards";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

describe("Library and Automations close preparation", () => {
  beforeEach(() => {
    useLibraryStore.setState({
      _editorDirty: false,
      _editorResources: {},
      _editorGenerationClock: 0,
    });
    useBuilderStore.getState().reset();
  });

  it("reports clean resources without prompting", () => {
    const prompt = vi.fn<DirtySurfacePrompt>();
    const library = createLibrarySurfaceCloseAdapter(prompt);
    const automations = createAutomationsSurfaceCloseAdapter(prompt);

    expect(library.observe(makeSurface("library-1", { surface_type: "library" })))
      .toMatchObject({ resource_id: "library:library-1", dirty: false });
    expect(automations.observe(makeSurface("automations-1", { surface_type: "automations" })))
      .toMatchObject({ resource_id: "automations:builder", dirty: false });
    expect(prompt).not.toHaveBeenCalled();
  });

  it("prepares Library and Automations choices without running save or discard effects", async () => {
    const librarySave = vi.fn().mockResolvedValue(true);
    const libraryDiscard = vi.fn().mockResolvedValue(true);
    useLibraryStore.setState({
      _editorDirty: true,
      _editorResources: {
        "library-1": {
          dirty: true,
          actions: { save: librarySave, discard: libraryDiscard },
          generation: 1,
          identity: "skills/library-1",
        },
      },
      _editorGenerationClock: 1,
    });
    const baseline = { schema: 2 as const, id: "wf", name: "Saved", nodes: [], edges: [] };
    useBuilderStore.setState({ blueprint: baseline, baseline, dirty: false });
    useBuilderStore.getState().setBlueprint({ ...baseline, name: "Draft" });
    const automationSave = vi.spyOn(useBuilderStore.getState(), "save");
    const automationDiscard = vi.spyOn(useBuilderStore.getState(), "discard");
    const library = createLibrarySurfaceCloseAdapter(() => "discard");
    const automations = createAutomationsSurfaceCloseAdapter(() => "cancel");
    const snapshot = makeSingleGroupDocument([
      makeSurface("library-1", { surface_type: "library" }),
      makeSurface("automations-1", { surface_type: "automations" }),
    ]);
    const context = {
      snapshot,
      transaction_version: 4,
      closing_surface_ids: ["library-1", "automations-1"],
    } as const;
    const libraryResource = {
      resource_id: "library:library-1",
      resource_generation: library.observe(snapshot.surfaces["library-1"])!.resource_generation,
      presentation_ids: ["library-1"],
    };
    const automationResource = {
      resource_id: "automations:builder",
      resource_generation: automations.observe(snapshot.surfaces["automations-1"])!.resource_generation,
      presentation_ids: ["automations-1"],
    };

    const libraryPreparation = await library.prepare({ context, resource: libraryResource });
    const automationPreparation = await automations.prepare({ context, resource: automationResource });

    expect(libraryPreparation?.choice).toBe("discard");
    expect(automationPreparation?.choice).toBe("cancel");
    expect(librarySave).not.toHaveBeenCalled();
    expect(libraryDiscard).not.toHaveBeenCalled();
    expect(automationSave).not.toHaveBeenCalled();
    expect(automationDiscard).not.toHaveBeenCalled();

    await libraryPreparation?.discard?.();
    expect(libraryDiscard).toHaveBeenCalledOnce();
  });

  it("binds deferred Library and Automations effects to the observed identity and generation", async () => {
    const librarySave = vi.fn().mockResolvedValue(true);
    useLibraryStore.getState().registerEditorCloseActions("library-1", {
      save: librarySave,
      discard: vi.fn(),
    });
    useLibraryStore.getState().markEditorSurfaceDirty(
      "library-1",
      true,
      "skills/alpha",
    );
    const library = createLibrarySurfaceCloseAdapter(() => "save");
    const librarySurface = makeSurface("library-1", { surface_type: "library" });
    const libraryDocument = makeSingleGroupDocument([librarySurface]);
    const libraryObservation = library.observe(librarySurface)!;
    const libraryPreparation = await library.prepare({
      context: {
        snapshot: libraryDocument,
        transaction_version: 1,
        closing_surface_ids: [librarySurface.surface_id],
      },
      resource: {
        resource_id: libraryObservation.resource_id,
        resource_generation: libraryObservation.resource_generation,
        presentation_ids: [librarySurface.surface_id],
      },
    });

    useLibraryStore.getState().markEditorSurfaceDirty(
      "library-1",
      true,
      "skills/beta",
    );
    await expect(libraryPreparation?.save?.()).resolves.toBe(false);
    expect(librarySave).not.toHaveBeenCalled();

    const automation = { schema: 2 as const, id: "wf", name: "Automation", nodes: [], edges: [] };
    useBuilderStore.getState().initialize(automation);
    useBuilderStore.getState().setBlueprint({ ...automation, name: "Draft" });
    const automationDiscard = vi.fn().mockReturnValue(true);
    useBuilderStore.setState({ discard: automationDiscard });
    const automations = createAutomationsSurfaceCloseAdapter(() => "discard");
    const automationsSurface = makeSurface("automations-1", { surface_type: "automations" });
    const automationsDocument = makeSingleGroupDocument([automationsSurface]);
    const automationsObservation = automations.observe(automationsSurface)!;
    const automationsPreparation = await automations.prepare({
      context: {
        snapshot: automationsDocument,
        transaction_version: 2,
        closing_surface_ids: [automationsSurface.surface_id],
      },
      resource: {
        resource_id: automationsObservation.resource_id,
        resource_generation: automationsObservation.resource_generation,
        presentation_ids: [automationsSurface.surface_id],
      },
    });

    useBuilderStore.getState().setBlueprint({ ...automation, name: "Newer draft" });
    await automationsPreparation?.discard?.();
    expect(automationDiscard).not.toHaveBeenCalled();
  });

  it("coalesces concurrent choice collection for one resource", async () => {
    let releaseChoice: ((choice: "cancel") => void) | undefined;
    const prompt = vi.fn<DirtySurfacePrompt>(() => new Promise((resolve) => {
      releaseChoice = resolve as (choice: "cancel") => void;
    }));
    useLibraryStore.setState({
      _editorDirty: true,
      _editorResources: {
        "library-1": {
          dirty: true,
          actions: { save: vi.fn(), discard: vi.fn() },
          generation: 1,
          identity: "skills/library-1",
        },
      },
      _editorGenerationClock: 1,
    });
    const adapter = createLibrarySurfaceCloseAdapter(prompt);
    const snapshot = makeSingleGroupDocument([
      makeSurface("library-1", { surface_type: "library" }),
    ]);
    const request = {
      context: {
        snapshot,
        transaction_version: 1,
        closing_surface_ids: ["library-1"],
      },
      resource: {
        resource_id: "library:library-1",
        resource_generation: adapter.observe(snapshot.surfaces["library-1"])!.resource_generation,
        presentation_ids: ["library-1"],
      },
    } as const;

    const first = adapter.prepare(request);
    const second = adapter.prepare(request);
    await vi.waitFor(() => expect(prompt).toHaveBeenCalledOnce());
    releaseChoice?.("cancel");

    await expect(first).resolves.toMatchObject({ choice: "cancel" });
    await expect(second).resolves.toMatchObject({ choice: "cancel" });
  });

  it("changes resource generation when Library or Automations resource state changes", () => {
    const library = createLibrarySurfaceCloseAdapter(() => "cancel");
    const automations = createAutomationsSurfaceCloseAdapter(() => "cancel");
    const librarySurface = makeSurface("library-1", { surface_type: "library" });
    const automationsSurface = makeSurface("automations-1", { surface_type: "automations" });
    const firstLibraryGeneration = library.observe(librarySurface)!.resource_generation;
    const firstAutomationGeneration = automations.observe(automationsSurface)!.resource_generation;

    useLibraryStore.getState().markEditorSurfaceDirty("library-1", true);
    useBuilderStore.getState().setBlueprint({
      schema: 2,
      id: "wf",
      name: "Draft",
      nodes: [],
      edges: [],
    });

    expect(library.observe(librarySurface)!.resource_generation)
      .not.toBe(firstLibraryGeneration);
    expect(automations.observe(automationsSurface)!.resource_generation)
      .not.toBe(firstAutomationGeneration);
  });

  it("keeps a failed Library save dirty and leaves layout intact", async () => {
    const save = vi.fn().mockResolvedValue(false);
    useLibraryStore.setState({
      _editorDirty: true,
      _editorResources: {
        "library-1": {
          dirty: true,
          actions: { save, discard: vi.fn() },
          generation: 1,
          identity: "skills/library-1",
        },
      },
      _editorGenerationClock: 1,
    });
    const surface = makeSurface("library-1", { surface_type: "library", state: {} });
    const registry = createCoreWorkbenchSurfaceRegistry({
      dirty_surface_prompt: () => "save",
    });
    const store = createWorkbenchStore({
      initial_document: makeSingleGroupDocument([surface]),
    });
    const before = store.getState().document;
    const navigation = createWorkbenchNavigationService({ registry, store });

    await expect(navigation.close("library-1")).resolves.toBe("cancel");

    expect(save).toHaveBeenCalledOnce();
    expect(store.getState().document).toBe(before);
    expect(useLibraryStore.getState().isEditorSurfaceDirty("library-1")).toBe(true);
  });

  it("cancels before effects when Automations switches resources through an edit-revision ABA", async () => {
    const first = { schema: 2 as const, id: "first", name: "First", nodes: [], edges: [] };
    const second = { schema: 2 as const, id: "second", name: "Second", nodes: [], edges: [] };
    useBuilderStore.getState().initialize(first);
    useBuilderStore.setState({ path: "/automations/first.md" });
    useBuilderStore.getState().setBlueprint({ ...first, name: "First draft" });
    expect(useBuilderStore.getState().editRevision).toBe(1);

    let releaseChoice: ((choice: "save") => void) | undefined;
    const prompt = vi.fn<DirtySurfacePrompt>(() => new Promise((resolve) => {
      releaseChoice = resolve as (choice: "save") => void;
    }));
    const registry = createCoreWorkbenchSurfaceRegistry({ dirty_surface_prompt: prompt });
    const surface = makeSurface("automations-1", { surface_type: "automations", state: {} });
    const store = createWorkbenchStore({
      initial_document: makeSingleGroupDocument([surface]),
    });
    const before = store.getState().document;
    const navigation = createWorkbenchNavigationService({ registry, store });

    const closing = navigation.close(surface.surface_id);
    await vi.waitFor(() => expect(prompt).toHaveBeenCalledOnce());

    useBuilderStore.getState().reset();
    useBuilderStore.getState().initialize(second);
    useBuilderStore.setState({ path: "/automations/second.md" });
    useBuilderStore.getState().setBlueprint({ ...second, name: "Second draft" });
    expect(useBuilderStore.getState().editRevision).toBe(1);
    const save = vi.fn().mockResolvedValue(true);
    const discard = vi.fn().mockReturnValue(true);
    useBuilderStore.setState({ save, discard });
    releaseChoice?.("save");

    await expect(closing).resolves.toBe("cancel");
    expect(save).not.toHaveBeenCalled();
    expect(discard).not.toHaveBeenCalled();
    expect(store.getState().document).toBe(before);
  });

  it.each(["close_group", "reset_workbench"] as const)(
    "does not partially discard Library when Automations cancels %s",
    async (action) => {
      const libraryDiscard = vi.fn().mockResolvedValue(true);
      useLibraryStore.setState({
        _editorDirty: true,
        _editorResources: {
          "library-1": {
            dirty: true,
            actions: { save: vi.fn(), discard: libraryDiscard },
            generation: 1,
            identity: "skills/library-1",
          },
        },
        _editorGenerationClock: 1,
      });
      const baseline = { schema: 2 as const, id: "wf", name: "Saved", nodes: [], edges: [] };
      useBuilderStore.setState({ blueprint: baseline, baseline, dirty: false });
      useBuilderStore.getState().setBlueprint({ ...baseline, name: "Draft" });
      const prompt = vi.fn<DirtySurfacePrompt>(({ surface_type }) => (
        surface_type === "library" ? "discard" : "cancel"
      ));
      const registry = createCoreWorkbenchSurfaceRegistry({ dirty_surface_prompt: prompt });
      const store = createWorkbenchStore({
        initial_document: makeSingleGroupDocument([
          makeSurface("library-1", { surface_type: "library", state: {} }),
          makeSurface("automations-1", { surface_type: "automations", state: {} }),
        ]),
      });
      const before = store.getState().document;
      const navigation = createWorkbenchNavigationService({ registry, store });

      const result = action === "close_group"
        ? navigation.close_group("group-1")
        : navigation.reset_workbench();
      await expect(result).resolves.toBe("cancel");

      expect(prompt).toHaveBeenCalledTimes(2);
      expect(libraryDiscard).not.toHaveBeenCalled();
      expect(store.getState().document).toBe(before);
      expect(useBuilderStore.getState().dirty).toBe(true);
    },
  );
});
