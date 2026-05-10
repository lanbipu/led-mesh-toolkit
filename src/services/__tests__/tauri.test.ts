import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { tauriApi } from "../tauri";

describe("tauriApi", () => {
  beforeEach(() => { vi.clearAllMocks(); });

  it("listRecentProjects calls invoke with no args", async () => {
    (invoke as any).mockResolvedValueOnce([]);
    const r = await tauriApi.listRecentProjects();
    expect(invoke).toHaveBeenCalledWith("list_recent_projects");
    expect(r).toEqual([]);
  });

  it("seedExampleProject passes target_dir + example", async () => {
    (invoke as any).mockResolvedValueOnce("/tmp/x/curved-flat");
    await tauriApi.seedExampleProject("/tmp/x", "curved-flat");
    expect(invoke).toHaveBeenCalledWith("seed_example_project", {
      targetDir: "/tmp/x",
      example: "curved-flat",
    });
  });

  it("reconstructSurface passes the 3 args", async () => {
    (invoke as any).mockResolvedValueOnce({ run_id: 1, surface: {}, report_json_path: "" });
    await tauriApi.reconstructSurface("/p", "MAIN", "m.yaml");
    expect(invoke).toHaveBeenCalledWith("reconstruct_surface", {
      projectPath: "/p",
      screenId: "MAIN",
      measurementsPath: "m.yaml",
    });
  });
});
