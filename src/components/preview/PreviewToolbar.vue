<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtStatusBadge from "@/components/primitives/LmtStatusBadge.vue";
import Button from "@/components/ui/Button.vue";
import type { LmtTone } from "@/components/primitives/types";

const { t } = useI18n();
const recon = useReconstructionStore();
const proj = useCurrentProjectStore();
const ui = useUiStore();

const statusTone = computed<LmtTone>(() => {
  switch (recon.status) {
    case "running":
      return "progress";
    case "done":
      return "healthy";
    case "error":
      return "critical";
    default:
      return "unknown";
  }
});

const statusIcon = computed(() => {
  switch (recon.status) {
    case "running":
      return "loader-2";
    case "done":
      return "check-circle-2";
    case "error":
      return "alert-triangle";
    default:
      return "circle";
  }
});

function lmtErrMsg(e: unknown): string {
  if (e && typeof e === "object") {
    const err = e as Record<string, unknown>;
    if (typeof err.message === "string") return `[${err.kind ?? "error"}] ${err.message}`;
  }
  return String(e);
}

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
    ui.toast("error", lmtErrMsg(e));
  }
}

const isExporting = ref(false);

async function exportNow(target: string) {
  if (!proj.absPath || !recon.currentRunId || isExporting.value) return;
  isExporting.value = true;
  const snapshotId = proj.id;
  const snapshotAbsPath = proj.absPath;
  try {
    // Pull the active screen id from project config (M1.1 single-screen → first key);
    // matches what reconstruct/export stored, so the default filename lines up.
    const screen = Object.keys(proj.config?.screens ?? {})[0] ?? "MAIN";
    const defaultPath = `${snapshotAbsPath}/output/${screen}_${target}_run${recon.currentRunId}.obj`;
    const dst = await saveDialog({
      title: t("preview.exportPickPath"),
      defaultPath,
      filters: [{ name: "OBJ", extensions: ["obj"] }],
    });
    if (!dst) return;
    if (proj.id !== snapshotId) {
      ui.toast("info", "project changed during file pick — export cancelled");
      return;
    }
    const path = await recon.exportObj(target, String(dst));
    if (proj.id !== snapshotId) return;
    ui.toast("success", `Wrote ${path}`);
  } catch (e) {
    ui.toast("error", lmtErrMsg(e));
  } finally {
    isExporting.value = false;
  }
}

const exportTargets: { id: string; label: string; icon: string }[] = [
  { id: "disguise", label: t("preview.exportDisguise"), icon: "monitor-cog" },
  { id: "unreal", label: t("preview.exportUnreal"), icon: "gamepad-2" },
  { id: "neutral", label: t("preview.exportNeutral"), icon: "package" },
];
</script>

<template>
  <div class="flex flex-wrap items-center gap-3 border-b bg-background px-6 py-2.5">
    <Button
      variant="default"
      size="sm"
      :disabled="!recon.canReconstruct || recon.status === 'running'"
      @click="reconstructNow"
    >
      <LmtIcon
        :name="recon.status === 'running' ? 'loader-2' : 'play'"
        :size="13"
        :class="{ 'animate-spin': recon.status === 'running' }"
      />
      {{ recon.status === "running" ? t("preview.reconstructing") : t("preview.reconstruct") }}
    </Button>

    <div class="flex items-center gap-2">
      <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("preview.status") }}
      </p>
      <LmtStatusBadge :tone="statusTone" :label="recon.status" :icon="statusIcon" size="sm" />
    </div>

    <div class="ml-auto flex items-center gap-2">
      <p class="hidden text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground md:block">
        EXPORT OBJ
      </p>
      <Button
        v-for="tgt in exportTargets"
        :key="tgt.id"
        variant="outline"
        size="sm"
        :disabled="!recon.currentRunId || isExporting"
        @click="exportNow(tgt.id)"
      >
        <LmtIcon :name="tgt.icon" :size="13" />
        {{ tgt.label }}
      </Button>
    </div>
  </div>
</template>
