<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
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
const isImporting = ref(false);

onMounted(async () => {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
});

async function loadMeasured() {
  if (!proj.absPath || isImporting.value) return;
  isImporting.value = true;
  const snapshotId = proj.id;
  const snapshotAbsPath = proj.absPath;
  try {
    const file = await open({
      title: "Select measured.yaml",
      filters: [{ name: "YAML", extensions: ["yaml", "yml"] }],
      defaultPath: `${snapshotAbsPath}/measurements`,
    });
    if (!file) return;
    if (proj.id !== snapshotId) {
      ui.toast("info", "project changed during file pick — load cancelled");
      return;
    }
    const mp = await tauriApi.loadMeasurementsYaml(String(file));
    if (proj.id !== snapshotId) return;
    const rel = String(file).startsWith(snapshotAbsPath)
      ? String(file).slice(snapshotAbsPath.length).replace(/^[\\/]+/, "")
      : String(file);
    recon.setMeasurementsPath(rel);
    ui.toast("success", t("import.loaded", { n: mp.points.length }));
  } catch (e) {
    ui.toast("error", `${e}`);
  } finally {
    isImporting.value = false;
  }
}

async function loadCsv() {
  if (!proj.absPath || isImporting.value) return;
  isImporting.value = true;
  // Snapshot the project identity *before* the file picker opens so a
  // mid-flight project switch can't redirect the import to the wrong project.
  const snapshotId = proj.id;
  const snapshotAbsPath = proj.absPath;
  const screenIds = Object.keys(proj.config?.screens ?? {});
  const screenId = screenIds[0] ?? "MAIN";
  try {
    const file = await open({
      title: "Select total-station CSV",
      filters: [{ name: "CSV", extensions: ["csv"] }],
      defaultPath: `${snapshotAbsPath}/measurements`,
    });
    if (!file) return;
    if (proj.id !== snapshotId) {
      ui.toast("info", "project changed during file pick — import cancelled");
      return;
    }
    const result = await tauriApi.importTotalStationCsv(
      snapshotAbsPath,
      String(file),
      screenId,
    );
    if (proj.id !== snapshotId) return; // user switched away while waiting on backend
    recon.setImportReport(result);
    recon.setMeasurementsPath(result.measurementsYamlPath);
    const summary = t("import.csvSummary", {
      m: result.measuredCount,
      f: result.fabricatedCount,
      o: result.outlierCount,
      x: result.missingCount,
    });
    if (result.warnings.length > 0) {
      ui.toast("info", `${summary} · ${result.warnings.length} warning(s)`);
    } else {
      ui.toast("success", summary);
    }
  } catch (e) {
    ui.toast("error", `${e}`);
  } finally {
    isImporting.value = false;
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
          <Button variant="default" :disabled="!proj.absPath || isImporting" @click="loadCsv">
            <LmtIcon name="upload" :size="14" />
            {{ t("import.loadCsv") }}
          </Button>
          <Button variant="outline" :disabled="!proj.absPath || isImporting" @click="loadMeasured">
            <LmtIcon name="upload" :size="14" />
            {{ t("import.loadMeasured") }}
          </Button>
        </div>
      </div>

      <section
        v-if="recon.importReport"
        class="rounded-lg border bg-card p-5"
      >
        <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground mb-3">
          {{ t("import.reportHeader") }}
        </p>
        <div class="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <LmtKV :label="t('import.measured')" :value="String(recon.importReport.measuredCount)" />
          <LmtKV :label="t('import.fabricated')" :value="String(recon.importReport.fabricatedCount)" />
          <LmtKV :label="t('import.outliers')" :value="String(recon.importReport.outlierCount)" />
          <LmtKV :label="t('import.missing')" :value="String(recon.importReport.missingCount)" />
        </div>
        <ul v-if="recon.importReport.warnings.length > 0" class="mt-3 space-y-1 text-xs text-amber-500">
          <li v-for="(w, i) in recon.importReport.warnings" :key="i">⚠ {{ w }}</li>
        </ul>
      </section>

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
            <LmtIcon name="check-circle-2" :size="13" class="mt-0.5 text-status-healthy" />
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
