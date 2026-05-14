import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    loadProjectYaml: vi.fn(),
    saveProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useCurrentProjectStore } from "../currentProject";

const sampleConfig = {
  project: { name: "X", unit: "mm" },
  screens: {
    MAIN: {
      cabinet_count: [8, 4] as [number, number],
      cabinet_size_mm: [500, 500] as [number, number],
      shape_prior: { type: "flat" } as const,
      shape_mode: "rectangle" as const,
      irregular_mask: [],
    },
  },
  coordinate_system: {
    origin_point: "MAIN_V001_R001",
    x_axis_point: "MAIN_V008_R001",
    xy_plane_point: "MAIN_V001_R004",
  },
  output: {
    target: "disguise",
    obj_filename: "{screen_id}.obj",
    weld_vertices_tolerance_mm: 1,
    triangulate: true,
  },
};

describe("useCurrentProjectStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("load by id resolves abs_path then loads yaml", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    const s = useCurrentProjectStore();
    await s.load(5);
    expect(s.absPath).toBe("/p");
    expect(s.config?.project.name).toBe("X");
    expect(s.dirty).toBe(false);
  });

  it("updateScreen sets dirty", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    const s = useCurrentProjectStore();
    await s.load(5);
    s.updateScreen("MAIN", { ...sampleConfig.screens.MAIN, cabinet_count: [10, 4] });
    expect(s.dirty).toBe(true);
  });

  it("save calls invoke + clears dirty", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    (tauriApi.saveProjectYaml as any).mockResolvedValueOnce(undefined);
    const s = useCurrentProjectStore();
    await s.load(5);
    s.updateScreen("MAIN", { ...sampleConfig.screens.MAIN, cabinet_count: [10, 4] });
    await s.save();
    expect(tauriApi.saveProjectYaml).toHaveBeenCalled();
    expect(s.dirty).toBe(false);
  });

  it("setMethod writes project.method and saves", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 9, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    (tauriApi.saveProjectYaml as any).mockResolvedValueOnce(undefined);
    const s = useCurrentProjectStore();
    await s.load(9);
    expect(s.config?.project.method).toBeUndefined();
    await s.setMethod("m1");
    expect(s.config?.project.method).toBe("m1");
    expect(tauriApi.saveProjectYaml).toHaveBeenCalled();
    expect(s.dirty).toBe(false);
  });

  it("setMethod is a no-op when no project loaded", async () => {
    const s = useCurrentProjectStore();
    await s.setMethod("m2");
    expect(tauriApi.saveProjectYaml).not.toHaveBeenCalled();
  });

  it("updateScreen targets named screen only — multi-screen projects stay isolated", async () => {
    const multiScreen = {
      ...sampleConfig,
      screens: {
        MAIN: sampleConfig.screens.MAIN,
        SECONDARY: {
          ...sampleConfig.screens.MAIN,
          cabinet_count: [6, 3] as [number, number],
          irregular_mask: [[1, 1]] as [number, number][],
        },
      },
    };
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 7, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(multiScreen);
    const s = useCurrentProjectStore();
    await s.load(7);
    s.updateScreen("SECONDARY", {
      ...multiScreen.screens.SECONDARY,
      irregular_mask: [
        [0, 0],
        [1, 1],
      ],
    });
    // SECONDARY took the edit
    expect(s.config?.screens.SECONDARY.irregular_mask).toEqual([
      [0, 0],
      [1, 1],
    ]);
    // MAIN must be untouched — guards against DesignToolbar saving to the wrong screen
    expect(s.config?.screens.MAIN.irregular_mask).toEqual([]);
    expect(s.config?.screens.MAIN.cabinet_count).toEqual([8, 4]);
  });
});
