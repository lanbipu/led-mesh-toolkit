import { describe, it, expect, beforeEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";

// happy-dom doesn't expose localStorage by default in this project
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

import { useUiStore } from "../ui";

describe("useUiStore — dismissedBanners", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    for (const k of Object.keys(storage)) delete storage[k];
  });

  it("isBannerDismissed returns false by default", () => {
    const ui = useUiStore();
    expect(ui.isBannerDismissed("any-key")).toBe(false);
  });

  it("dismissBanner records the key", () => {
    const ui = useUiStore();
    ui.dismissBanner("design-method-banner-7");
    expect(ui.isBannerDismissed("design-method-banner-7")).toBe(true);
    expect(ui.isBannerDismissed("other-key")).toBe(false);
  });

  it("dismissals are session-scoped (not localStorage)", () => {
    const ui = useUiStore();
    ui.dismissBanner("k");
    expect(localStorage.getItem("lmt.dismissedBanners")).toBeNull();
  });
});
