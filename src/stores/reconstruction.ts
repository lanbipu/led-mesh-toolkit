import { defineStore } from "pinia";
import { computed, ref, watch } from "vue";
import {
  tauriApi,
  type ReconstructedSurface,
  type ReconstructionRun,
  type TotalStationImportResult,
} from "@/services/tauri";
import { useCurrentProjectStore } from "./currentProject";

export const useReconstructionStore = defineStore("reconstruction", () => {
  const measurementsPath = ref<string | null>(null);
  const currentSurface = ref<ReconstructedSurface | null>(null);
  const currentRunId = ref<number | null>(null);
  const status = ref<"idle" | "running" | "done" | "error">("idle");
  const recentRuns = ref<ReconstructionRun[]>([]);
  const importReport = ref<TotalStationImportResult | null>(null);

  /** Drop all per-project derived state. Called automatically when the
   *  active project switches; can also be invoked manually. */
  function resetForProjectSwitch() {
    measurementsPath.value = null;
    currentSurface.value = null;
    currentRunId.value = null;
    status.value = "idle";
    importReport.value = null;
    recentRuns.value = [];
  }

  // Reactively wipe state whenever the active project changes — prevents
  // stale measurements / surfaces / import reports from leaking into a
  // freshly opened project.
  watch(
    () => useCurrentProjectStore().id,
    (id, prev) => {
      if (id !== prev) resetForProjectSwitch();
    },
  );

  const canReconstruct = computed(() => measurementsPath.value !== null);

  function setMeasurementsPath(path: string) {
    measurementsPath.value = path;
  }

  function setImportReport(r: TotalStationImportResult | null) {
    importReport.value = r;
  }

  async function reconstruct(projectPath: string, screenId: string) {
    if (!measurementsPath.value) throw new Error("no measurements loaded");
    status.value = "running";
    try {
      const r = await tauriApi.reconstructSurface(projectPath, screenId, measurementsPath.value);
      currentRunId.value = r.run_id;
      currentSurface.value = r.surface;
      status.value = "done";
      return r;
    } catch (e) {
      status.value = "error";
      throw e;
    }
  }

  async function exportObj(target: string, dstAbsPath?: string) {
    if (!currentRunId.value) throw new Error("no run");
    return await tauriApi.exportObj(currentRunId.value, target, dstAbsPath);
  }

  async function loadRuns(projectPath: string, screenId?: string) {
    recentRuns.value = await tauriApi.listRuns(projectPath, screenId);
  }

  return {
    measurementsPath,
    currentSurface,
    currentRunId,
    status,
    recentRuns,
    importReport,
    canReconstruct,
    setMeasurementsPath,
    setImportReport,
    resetForProjectSwitch,
    reconstruct,
    exportObj,
    loadRuns,
  };
});
