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
});
