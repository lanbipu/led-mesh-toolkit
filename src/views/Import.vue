<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { open } from "@tauri-apps/plugin-dialog";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtStatusBadge from "@/components/primitives/LmtStatusBadge.vue";
import LmtKV from "@/components/primitives/LmtKV.vue";
import Button from "@/components/ui/Button.vue";

const { t } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));

const hasMeasurements = computed(() => recon.measurementsPath != null);

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
    ui.toast("success", t("import.loaded", { n: mp.points.length }));
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtPageHeader
      :eyebrow="t('import.eyebrow')"
      :title="t('import.title')"
      :description="t('import.description')"
    />

    <section class="grid gap-4 lg:grid-cols-[2fr_1fr]">
      <div class="flex flex-col gap-4 rounded-lg border bg-card p-5">
        <div class="flex items-center justify-between">
          <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
            MEASURED.YAML
          </p>
          <LmtStatusBadge
            :tone="hasMeasurements ? 'healthy' : 'unknown'"
            :label="hasMeasurements ? 'loaded' : 'empty'"
            size="sm"
          />
        </div>

        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <LmtKV :label="t('import.current')">
            <span v-if="recon.measurementsPath">{{ recon.measurementsPath }}</span>
            <span v-else class="italic text-muted-foreground">{{ t("import.none") }}</span>
          </LmtKV>
          <LmtKV label="PROJECT PATH" :value="proj.absPath ?? '—'" />
        </div>

        <div class="flex flex-wrap gap-2">
          <Button variant="default" :disabled="!proj.absPath" @click="loadMeasured">
            <LmtIcon name="upload" :size="14" />
            {{ t("import.loadMeasured") }}
          </Button>
        </div>
      </div>

      <aside class="flex flex-col gap-3 rounded-lg border bg-card p-5">
        <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
          ROADMAP
        </p>
        <ul class="space-y-3 text-xs text-muted-foreground">
          <li class="flex items-start gap-2">
            <LmtIcon name="check-circle-2" :size="13" class="mt-0.5 text-status-healthy" />
            <span><span class="font-bold text-foreground">M0.2</span> — manual measured.yaml</span>
          </li>
          <li class="flex items-start gap-2">
            <LmtIcon name="circle-dot" :size="13" class="mt-0.5 text-status-info" />
            <span><span class="font-bold text-foreground">M1</span> — total-station CSV adapter</span>
          </li>
          <li class="flex items-start gap-2">
            <LmtIcon name="circle" :size="13" class="mt-0.5 text-muted-foreground" />
            <span><span class="font-bold text-foreground">M2</span> — visual back-calculation</span>
          </li>
        </ul>
      </aside>
    </section>
  </div>
</template>
