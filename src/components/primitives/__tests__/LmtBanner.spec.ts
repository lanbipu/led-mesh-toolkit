import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";

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

import LmtBanner from "../LmtBanner.vue";
import { useUiStore } from "@/stores/ui";

describe("LmtBanner", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    for (const k of Object.keys(storage)) delete storage[k];
  });

  it("renders title and action label", () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hello", actionLabel: "Go", dismissKey: "k1" },
    });
    expect(w.text()).toContain("Hello");
    expect(w.text()).toContain("Go");
  });

  it("emits action when action button clicked", async () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", actionLabel: "Go", dismissKey: "k2" },
    });
    await w.find("button[data-banner-action]").trigger("click");
    expect(w.emitted("action")).toBeTruthy();
  });

  it("dismiss button calls ui.dismissBanner with the key", async () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", dismissKey: "design-banner-3" },
    });
    const ui = useUiStore();
    await w.find("button[data-banner-dismiss]").trigger("click");
    expect(ui.isBannerDismissed("design-banner-3")).toBe(true);
  });

  it("renders nothing when already dismissed", () => {
    const ui = useUiStore();
    ui.dismissBanner("dismissed-key");
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", dismissKey: "dismissed-key" },
    });
    expect(w.find("[data-banner]").exists()).toBe(false);
  });

  it("info tone applies status-info classes", () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", dismissKey: "kt" },
    });
    const root = w.find("[data-banner]");
    expect(root.classes().join(" ")).toContain("status-info");
  });
});
