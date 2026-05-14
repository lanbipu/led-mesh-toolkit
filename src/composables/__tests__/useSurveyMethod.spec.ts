import { describe, it, expect, beforeEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { ref } from "vue";

const storage: Record<string, string> = {};
vi.stubGlobal("localStorage", {
  getItem: (k: string) => storage[k] ?? null,
  setItem: (k: string, v: string) => {
    storage[k] = v;
  },
  removeItem: (k: string) => {
    delete storage[k];
  },
  clear: () => {
    for (const k of Object.keys(storage)) delete storage[k];
  },
});

const routeMock = { params: ref<{ id: string }>({ id: "5" }) };
vi.mock("vue-router", () => ({
  useRoute: () => ({
    get params() {
      return routeMock.params.value;
    },
  }),
}));

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    loadProjectYaml: vi.fn(),
    saveProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useSurveyMethod } from "../useSurveyMethod";
import { useCurrentProjectStore } from "@/stores/currentProject";

describe("useSurveyMethod", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    routeMock.params.value = { id: "5" };
  });

  it("returns null when no project loaded", () => {
    const { method } = useSurveyMethod();
    expect(method.value).toBeNull();
  });

  it("returns 'm1' when project.method == 'm1' and route matches", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project: { name: "X", unit: "mm", method: "m1" },
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    const proj = useCurrentProjectStore();
    await proj.load(5);
    const { method } = useSurveyMethod();
    expect(method.value).toBe("m1");
  });

  it("returns null when proj.id mismatches route (race during switch)", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 7, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project: { name: "X", unit: "mm", method: "m2" },
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    const proj = useCurrentProjectStore();
    await proj.load(7);
    routeMock.params.value = { id: "99" };
    const { method } = useSurveyMethod();
    expect(method.value).toBeNull();
  });

  it("returns null when method field is missing", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project: { name: "X", unit: "mm" },
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    const proj = useCurrentProjectStore();
    await proj.load(5);
    const { method } = useSurveyMethod();
    expect(method.value).toBeNull();
  });
});
