import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useEditorStore } from "../editor";

describe("useEditorStore", () => {
  beforeEach(() => { setActivePinia(createPinia()); });

  it("toggleCell pushes snapshot, undo restores", () => {
    const s = useEditorStore();
    s.initFromScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    expect(s.isAbsent(0, 0)).toBe(false);
    s.toggleCell(0, 0);
    expect(s.isAbsent(0, 0)).toBe(true);
    s.undo();
    expect(s.isAbsent(0, 0)).toBe(false);
    s.redo();
    expect(s.isAbsent(0, 0)).toBe(true);
  });

  it("setRef stores per role + undoable", () => {
    const s = useEditorStore();
    s.initFromScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    s.setMode("refs");
    s.setRef("origin", "MAIN_V001_R001");
    s.setRef("x_axis", "MAIN_V004_R001");
    expect(s.refs.origin).toBe("MAIN_V001_R001");
    expect(s.refs.x_axis).toBe("MAIN_V004_R001");
    s.undo();
    expect(s.refs.x_axis).toBeNull();
  });

  it("undoStack truncates at 50", () => {
    const s = useEditorStore();
    s.initFromScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    for (let i = 0; i < 60; i++) s.toggleCell(i % 4, 0);
    expect(s.undoDepth).toBeLessThanOrEqual(50);
  });

  it("commitToScreen persists baseline edits to bottom_completion", () => {
    const s = useEditorStore();
    s.initFromScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    s.setBaseline(3);
    const next = s.commitToScreen({
      cabinet_count: [4, 2],
      cabinet_size_mm: [500, 500],
      shape_prior: { type: "flat" },
      shape_mode: "rectangle",
      irregular_mask: [],
    });
    expect(next.bottom_completion?.lowest_measurable_row).toBe(3);
  });

  it("commitToScreen preserves existing bottom_completion fallback fields", () => {
    const s = useEditorStore();
    const screen = {
      cabinet_count: [4, 2] as [number, number],
      cabinet_size_mm: [500, 500] as [number, number],
      shape_prior: { type: "flat" as const },
      shape_mode: "rectangle" as const,
      irregular_mask: [],
      bottom_completion: {
        lowest_measurable_row: 1,
        fallback_method: "vertical_extension",
        assumed_height_mm: 250,
      },
    };
    s.initFromScreen(screen);
    s.setBaseline(5);
    const next = s.commitToScreen(screen);
    expect(next.bottom_completion).toEqual({
      lowest_measurable_row: 5,
      fallback_method: "vertical_extension",
      assumed_height_mm: 250,
    });
  });

  it("commitToScreen leaves bottom_completion untouched when baseline is null", () => {
    const s = useEditorStore();
    const screen = {
      cabinet_count: [4, 2] as [number, number],
      cabinet_size_mm: [500, 500] as [number, number],
      shape_prior: { type: "flat" as const },
      shape_mode: "rectangle" as const,
      irregular_mask: [],
    };
    s.initFromScreen(screen);
    const next = s.commitToScreen(screen);
    expect(next.bottom_completion).toBeUndefined();
  });
});
