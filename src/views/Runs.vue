<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";

const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));
const expanded = ref<number | null>(null);
const reportCache = ref<Record<number, unknown>>({});

async function load() {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
    if (proj.absPath) await recon.loadRuns(proj.absPath);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function toggle(runId: number) {
  expanded.value = expanded.value === runId ? null : runId;
  if (expanded.value !== null && reportCache.value[runId] === undefined) {
    try {
      reportCache.value[runId] = await tauriApi.getRunReport(runId);
    } catch (e) {
      reportCache.value[runId] = { error: `${e}` };
    }
  }
}

onMounted(load);
</script>

<template>
  <div class="p-6">
    <h1 class="text-2xl font-bold">Reconstruction Runs</h1>
    <table class="mt-4 w-full text-sm">
      <thead class="border-b text-left">
        <tr>
          <th class="py-2">Created</th>
          <th>Screen</th>
          <th>Method</th>
          <th>RMS (mm)</th>
          <th>Vertices</th>
          <th>Target</th>
          <th>OBJ</th>
        </tr>
      </thead>
      <tbody>
        <template v-for="r in recon.recentRuns" :key="r.id">
          <tr class="cursor-pointer border-b hover:bg-accent" @click="toggle(r.id)">
            <td class="py-1">{{ r.created_at }}</td>
            <td>{{ r.screen_id }}</td>
            <td>{{ r.method }}</td>
            <td>{{ r.estimated_rms_mm.toFixed(2) }}</td>
            <td>{{ r.vertex_count }}</td>
            <td>{{ r.target ?? "—" }}</td>
            <td class="truncate text-xs">{{ r.output_obj_path ?? "—" }}</td>
          </tr>
          <tr v-if="expanded === r.id">
            <td colspan="7" class="bg-muted p-3">
              <pre class="text-xs">{{ JSON.stringify(reportCache[r.id], null, 2) }}</pre>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
    <p v-if="recon.recentRuns.length === 0" class="mt-4 text-sm text-muted-foreground">
      No runs yet. Run a reconstruction from the Preview view.
    </p>
  </div>
</template>
