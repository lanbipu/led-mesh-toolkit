import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    reconstructSurface: vi.fn(),
    exportObj: vi.fn(),
    listRuns: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useReconstructionStore } from "../reconstruction";

describe("useReconstructionStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("setMeasurementsPath enables reconstruction", () => {
    const s = useReconstructionStore();
    expect(s.canReconstruct).toBe(false);
    s.setMeasurementsPath("measurements/m.yaml");
    expect(s.canReconstruct).toBe(true);
  });

  it("reconstruct stores surface + runId", async () => {
    (tauriApi.reconstructSurface as any).mockResolvedValueOnce({
      run_id: 42,
      surface: {
        vertices: [],
        uv_coords: [],
        topology: { cols: 1, rows: 1 },
        screen_id: "MAIN",
        quality_metrics: {} as any,
      },
      report_json_path: "reports/r.json",
    });
    const s = useReconstructionStore();
    s.setMeasurementsPath("m.yaml");
    await s.reconstruct("/p", "MAIN");
    expect(s.currentRunId).toBe(42);
    expect(s.currentSurface).toBeTruthy();
  });
});
