import { defineStore } from "pinia";
import { ref } from "vue";
import { tauriApi, type ProjectConfig, type ScreenConfig } from "@/services/tauri";

export const useCurrentProjectStore = defineStore("currentProject", () => {
  const id = ref<number | null>(null);
  const absPath = ref<string | null>(null);
  const config = ref<ProjectConfig | null>(null);
  const dirty = ref(false);
  const loading = ref(false);

  async function load(projectId: number) {
    loading.value = true;
    try {
      const recent = await tauriApi.listRecentProjects();
      const match = recent.find((p) => p.id === projectId);
      if (!match) throw new Error(`project ${projectId} not in recent`);
      id.value = projectId;
      absPath.value = match.abs_path;
      config.value = await tauriApi.loadProjectYaml(match.abs_path);
      dirty.value = false;
    } finally {
      loading.value = false;
    }
  }

  function updateScreen(screenId: string, screen: ScreenConfig) {
    if (!config.value) return;
    config.value = {
      ...config.value,
      screens: { ...config.value.screens, [screenId]: screen },
    };
    dirty.value = true;
  }

  function updateCoordinateSystem(cs: ProjectConfig["coordinate_system"]) {
    if (!config.value) return;
    config.value = { ...config.value, coordinate_system: cs };
    dirty.value = true;
  }

  function updateOutputTarget(target: string) {
    if (!config.value) return;
    config.value = {
      ...config.value,
      output: { ...config.value.output, target },
    };
    dirty.value = true;
  }

  async function save() {
    if (!absPath.value || !config.value) throw new Error("no project loaded");
    await tauriApi.saveProjectYaml(absPath.value, config.value);
    dirty.value = false;
  }

  return {
    id,
    absPath,
    config,
    dirty,
    loading,
    load,
    updateScreen,
    updateCoordinateSystem,
    updateOutputTarget,
    save,
  };
});
