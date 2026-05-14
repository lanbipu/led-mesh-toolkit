import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import { createMemoryHistory, createRouter } from "vue-router";
import { createI18n } from "vue-i18n";
import en from "@/locales/en.json";

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

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    loadProjectYaml: vi.fn(),
    saveProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import LmtSidebar from "../LmtSidebar.vue";
import { useCurrentProjectStore } from "@/stores/currentProject";

async function mountSidebar(routePath: string, options?: {
  recent?: Array<{ id: number; abs_path: string; display_name: string; last_opened_at: string }>;
  loadedProject?: { id: number; method?: "m1" | "m2" };
}) {
  setActivePinia(createPinia());
  for (const k of Object.keys(storage)) delete storage[k];
  const recent = options?.recent ?? [];
  (tauriApi.listRecentProjects as any).mockResolvedValue(recent);

  if (options?.loadedProject) {
    const proj = useCurrentProjectStore();
    const project: any = { name: "X", unit: "mm" };
    if (options.loadedProject.method) project.method = options.loadedProject.method;
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project,
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    await proj.load(options.loadedProject.id);
  }

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: "/", name: "home", component: { template: "<div />" } },
      { path: "/projects/:id/design", name: "design", component: { template: "<div />" } },
      { path: "/projects/:id/method", name: "method", component: { template: "<div />" } },
      { path: "/projects/:id/import", name: "import", component: { template: "<div />" } },
      { path: "/projects/:id/charuco", name: "charuco", component: { template: "<div />" } },
      { path: "/projects/:id/photoplan", name: "photoplan", component: { template: "<div />" } },
      { path: "/projects/:id/preview", name: "preview", component: { template: "<div />" } },
      { path: "/projects/:id/instruct", name: "instruct", component: { template: "<div />" } },
      { path: "/projects/:id/runs", name: "runs", component: { template: "<div />" } },
    ],
  });
  await router.push(routePath);
  await router.isReady();

  const i18n = createI18n({ legacy: false, locale: "en", messages: { en } });

  const w = mount(LmtSidebar, { global: { plugins: [router, i18n] } });
  await flushPromises();
  return w;
}

describe("LmtSidebar — home state", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders Recent Projects group when recent list non-empty", async () => {
    const w = await mountSidebar("/", {
      recent: [
        { id: 1, abs_path: "/a", display_name: "alpha", last_opened_at: "2026-01-01" },
        { id: 2, abs_path: "/b", display_name: "beta", last_opened_at: "2026-02-01" },
      ],
    });
    expect(w.text()).toContain("Recent Projects");
    expect(w.text()).toContain("alpha");
    expect(w.text()).toContain("beta");
  });

  it("omits Recent Projects group when list empty", async () => {
    const w = await mountSidebar("/", { recent: [] });
    expect(w.text()).not.toContain("Recent Projects");
  });

  it("limits Recent Projects to 5", async () => {
    const recent = Array.from({ length: 8 }).map((_, i) => ({
      id: i + 1,
      abs_path: `/p${i}`,
      display_name: `proj-${i}`,
      last_opened_at: `2026-01-0${(i % 7) + 1}`,
    }));
    const w = await mountSidebar("/", { recent });
    expect(w.findAll("[data-recent-project]")).toHaveLength(5);
  });

  it("first recent project is pinned (diamond icon + primary color) and others are not", async () => {
    const w = await mountSidebar("/", {
      recent: [
        { id: 11, abs_path: "/a", display_name: "alpha", last_opened_at: "2026-03-01" },
        { id: 12, abs_path: "/b", display_name: "beta", last_opened_at: "2026-02-01" },
        { id: 13, abs_path: "/c", display_name: "gamma", last_opened_at: "2026-01-01" },
      ],
    });
    const items = w.findAll("[data-recent-project]");
    expect(items).toHaveLength(3);
    // The pinned (first) item shows the diamond lucide icon + primary color + bold name
    const firstHtml = items[0].html();
    expect(firstHtml).toContain("lucide-diamond");
    expect(firstHtml).toContain("text-primary");
    expect(firstHtml).toContain("font-bold");
    // Subsequent items render the plain folder icon, no pinned styling
    const secondHtml = items[1].html();
    expect(secondHtml).toContain("lucide-folder");
    expect(secondHtml).not.toContain("text-primary");
  });
});

describe("LmtSidebar — project-internal state", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("hides SURVEY group when method is unset", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5 },
    });
    expect(w.text()).not.toContain("Survey");
    expect(w.text()).not.toContain("Import");
  });

  it("shows Import under SURVEY when method=m1", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5, method: "m1" },
    });
    expect(w.text()).toContain("Survey");
    expect(w.text()).toContain("Import");
    expect(w.text()).not.toContain("ChArUco");
  });

  it("shows Charuco + Photoplan when method=m2", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5, method: "m2" },
    });
    expect(w.text()).toContain("ChArUco");
    expect(w.text()).toContain("Photo Plan");
    expect(w.text()).not.toContain("Import");
  });

  it("output group order is Preview / Instruct / Runs (no Export)", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5, method: "m1" },
    });
    expect(w.text()).not.toContain("Export");
    const labels = w
      .findAll("[data-output-item]")
      .map((n) => n.text());
    expect(labels).toEqual(["Preview", "Instruct", "Runs"]);
  });
});
