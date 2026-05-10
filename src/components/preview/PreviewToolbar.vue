<script setup lang="ts">
import { useReconstructionStore } from "@/stores/reconstruction";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";

const recon = useReconstructionStore();
const proj = useCurrentProjectStore();
const ui = useUiStore();

async function reconstructNow() {
  if (!proj.absPath) return;
  if (!recon.canReconstruct) {
    ui.toast("error", "Load measurements first (Import view)");
    return;
  }
  try {
    await recon.reconstruct(proj.absPath, "MAIN");
    ui.toast("success", "Reconstruction done");
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function exportNow(target: string) {
  try {
    const path = await recon.exportObj(target);
    ui.toast("success", `Wrote ${path}`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}
</script>

<template>
  <div class="flex items-center gap-2 border-b bg-card p-2">
    <button
      :disabled="!recon.canReconstruct || recon.status === 'running'"
      class="rounded bg-primary px-3 py-1 text-sm text-primary-foreground disabled:opacity-50"
      @click="reconstructNow"
    >
      {{ recon.status === "running" ? "Running…" : "Reconstruct" }}
    </button>
    <span class="ml-2 text-xs text-muted-foreground">Status: {{ recon.status }}</span>
    <div class="ml-auto flex gap-2">
      <button
        :disabled="!recon.currentRunId"
        class="rounded border px-3 py-1 text-sm disabled:opacity-50"
        @click="exportNow('disguise')"
      >
        Export Disguise
      </button>
      <button
        :disabled="!recon.currentRunId"
        class="rounded border px-3 py-1 text-sm disabled:opacity-50"
        @click="exportNow('unreal')"
      >
        Export Unreal
      </button>
      <button
        :disabled="!recon.currentRunId"
        class="rounded border px-3 py-1 text-sm disabled:opacity-50"
        @click="exportNow('neutral')"
      >
        Export Neutral
      </button>
    </div>
  </div>
</template>
