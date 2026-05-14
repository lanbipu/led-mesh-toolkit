import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
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
import Method from "../Method.vue";
import { useCurrentProjectStore } from "@/stores/currentProject";

// Stub reka-ui dialog parts (Teleport is unreliable under happy-dom).
const passthrough = defineComponent({
  props: ["open"],
  setup(_, { slots }) {
    return () => h("div", { "data-stub": true }, slots.default?.());
  },
});
const stubs = {
  DialogRoot: passthrough,
  DialogPortal: passthrough,
  DialogOverlay: passthrough,
  DialogContent: passthrough,
  DialogTitle: defineComponent({ setup(_, { slots }) { return () => h("h2", slots.default?.()); } }),
  DialogDescription: defineComponent({ setup(_, { slots }) { return () => h("p", slots.default?.()); } }),
};

async function mountWith(method: "m1" | "m2" | null) {
  setActivePinia(createPinia());
  for (const k of Object.keys(storage)) delete storage[k];
  (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
    { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
  ]);
  const project: any = { name: "X", unit: "mm" };
  if (method) project.method = method;
  (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
    project,
    screens: {},
    coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
    output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
  });
  (tauriApi.saveProjectYaml as any).mockResolvedValue(undefined);
  const proj = useCurrentProjectStore();
  await proj.load(5);

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/projects/:id/method", name: "method", component: Method }],
  });
  await router.push("/projects/5/method");
  await router.isReady();

  const i18n = createI18n({ legacy: false, locale: "en", messages: { en } });

  return mount(Method, {
    global: { plugins: [router, i18n], stubs },
  });
}

describe("Method.vue", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders both method cards with bullets", async () => {
    const w = await mountWith(null);
    expect(w.text()).toContain("M1 · Total Station");
    expect(w.text()).toContain("M2 · Visual Back-Calc");
    expect(w.text()).toContain("CSV import");
    expect(w.text()).toContain("ArUco / Charuco markers");
  });

  it("shows AVAILABLE on both cards when method is unset", async () => {
    const w = await mountWith(null);
    const text = w.text();
    const availableCount = (text.match(/AVAILABLE/g) || []).length;
    expect(availableCount).toBeGreaterThanOrEqual(2);
    expect(text).not.toContain("CURRENT");
  });

  it("shows CURRENT on M1 card when method=m1", async () => {
    const w = await mountWith("m1");
    expect(w.text()).toContain("CURRENT");
  });

  it("clicking 'Use M1' on unset project calls setMethod", async () => {
    const w = await mountWith(null);
    const btns = w.findAll("button");
    const useM1 = btns.find((b) => b.text().includes("Use M1"));
    expect(useM1).toBeDefined();
    await useM1!.trigger("click");
    expect(tauriApi.saveProjectYaml).toHaveBeenCalled();
  });

  it("clicking 'Switch to M2' opens confirm dialog (no save yet)", async () => {
    const w = await mountWith("m1");
    const btns = w.findAll("button");
    const switchM2 = btns.find((b) => b.text().includes("Switch to M2"));
    await switchM2!.trigger("click");
    expect(w.text()).toContain("Switch method");
    expect(tauriApi.saveProjectYaml).not.toHaveBeenCalled();
  });
});
