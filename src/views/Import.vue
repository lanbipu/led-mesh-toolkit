<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";
import { open } from "@tauri-apps/plugin-dialog";

const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));

onMounted(async () => {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
});

async function loadMeasured() {
  if (!proj.absPath) return;
  try {
    const file = await open({
      title: "Select measured.yaml",
      filters: [{ name: "YAML", extensions: ["yaml", "yml"] }],
      defaultPath: `${proj.absPath}/measurements`,
    });
    if (!file) return;
    const mp = await tauriApi.loadMeasurementsYaml(String(file));
    const rel = String(file).startsWith(proj.absPath)
      ? String(file).slice(proj.absPath.length).replace(/^[\\/]+/, "")
      : String(file);
    recon.setMeasurementsPath(rel);
    ui.toast("success", `Loaded ${mp.points.length} measurements`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">Import (M0.2 demo)</h1>
    <p class="mt-2 text-sm text-muted-foreground">
      M1 will add CSV import (total station). M2 will add image import (visual BA). For now, load a
      hand-written measured.yaml from your project.
    </p>
    <div class="mt-6 flex flex-col gap-2">
      <button class="w-fit rounded bg-primary px-4 py-2 text-primary-foreground" @click="loadMeasured">
        Load measured.yaml
      </button>
      <p class="text-xs text-muted-foreground">Current: {{ recon.measurementsPath ?? "(none)" }}</p>
    </div>
  </div>
</template>
