import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    addRecentProject: vi.fn(),
    removeRecentProject: vi.fn(),
    seedExampleProject: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useProjectsStore } from "../projects";

describe("useProjectsStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("load fetches and stores recent", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 1, abs_path: "/x", display_name: "X", last_opened_at: "2026" },
    ]);
    const s = useProjectsStore();
    await s.load();
    expect(s.recent).toHaveLength(1);
  });

  it("createFromExample seeds + adds + reloads", async () => {
    (tauriApi.seedExampleProject as any).mockResolvedValueOnce("/seeded/curved-flat");
    (tauriApi.addRecentProject as any).mockResolvedValueOnce({
      id: 7,
      abs_path: "/seeded/curved-flat",
      display_name: "Curved Flat",
      last_opened_at: "2026",
    });
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 7, abs_path: "/seeded/curved-flat", display_name: "Curved Flat", last_opened_at: "2026" },
    ]);
    const s = useProjectsStore();
    const created = await s.createFromExample("curved-flat", "/seeded");
    expect(created.id).toBe(7);
    expect(s.recent).toHaveLength(1);
  });
});
