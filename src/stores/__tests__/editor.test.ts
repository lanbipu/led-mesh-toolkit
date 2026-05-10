import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useEditorStore } from "../editor";

describe("useEditorStore", () => {
  beforeEach(() => setActivePinia(createPinia()));

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
});
