import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { nextTick } from "vue";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    reconstructSurface: vi.fn(),
    exportObj: vi.fn(),
    listRuns: vi.fn(),
    loadProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useReconstructionStore } from "../reconstruction";
import { useCurrentProjectStore } from "../currentProject";

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

  it("switching active project wipes stale measurements + import report", async () => {
    const recon = useReconstructionStore();
    const proj = useCurrentProjectStore();

    // Manually push some state as if we had just imported from project A.
    recon.setMeasurementsPath("measurements/measured.yaml");
    recon.setImportReport({
      measurementsYamlPath: "measurements/measured.yaml",
      reportJsonPath: "measurements/import_report.json",
      measuredCount: 15,
      fabricatedCount: 0,
      outlierCount: 0,
      missingCount: 0,
      warnings: [],
    });
    expect(recon.importReport).not.toBeNull();
    expect(recon.measurementsPath).toBe("measurements/measured.yaml");

    // Simulate a project switch by mutating the currentProject id directly.
    // We bypass load() to keep the test focused on the watcher contract.
    (proj as any).id = 1;
    await nextTick();
    (proj as any).id = 2;
    await nextTick();

    expect(recon.importReport).toBeNull();
    expect(recon.measurementsPath).toBeNull();
    expect(recon.currentSurface).toBeNull();
  });
});
