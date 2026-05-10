import { defineStore } from "pinia";
import { ref } from "vue";
import { tauriApi, type RecentProject } from "@/services/tauri";

export const useProjectsStore = defineStore("projects", () => {
  const recent = ref<RecentProject[]>([]);
  const loading = ref(false);

  async function load() {
    loading.value = true;
    try {
      recent.value = await tauriApi.listRecentProjects();
    } finally {
      loading.value = false;
    }
  }

  async function createFromExample(example: string, targetDir: string) {
    const path = await tauriApi.seedExampleProject(targetDir, example);
    const displayName = `${example.replace(/-/g, " ")}`.replace(/\b\w/g, (c) => c.toUpperCase());
    const created = await tauriApi.addRecentProject(path, displayName);
    await load();
    return created;
  }

  async function openExisting(absPath: string, displayName: string) {
    const created = await tauriApi.addRecentProject(absPath, displayName);
    await load();
    return created;
  }

  async function remove(id: number) {
    await tauriApi.removeRecentProject(id);
    await load();
  }

  return { recent, loading, load, createFromExample, openExisting, remove };
});
