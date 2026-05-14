<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtStatusBadge from "@/components/primitives/LmtStatusBadge.vue";
import LmtMethodMismatchBanner from "@/components/shell/LmtMethodMismatchBanner.vue";
import Button from "@/components/ui/Button.vue";

const { t } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const ui = useUiStore();

const id = computed(() => Number(route.params.id));
const html = ref<string | null>(null);
const lastSavedPdf = ref<string | null>(null);

// M1.1 single-screen scope; pick the first screen from project config.
const screenId = computed(() => Object.keys(proj.config?.screens ?? {})[0] ?? "MAIN");

onMounted(async () => {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
});

const isGenerating = ref(false);

const projectReady = computed(
  () => proj.absPath != null && proj.id === id.value && !proj.loading,
);

async function generate() {
  if (!projectReady.value || isGenerating.value) return;
  const sid = screenId.value;
  if (!sid) {
    ui.toast("error", "no screen in project");
    return;
  }
  isGenerating.value = true;
  const snapshotId = proj.id;
  const snapshotAbsPath = proj.absPath!;
  try {
    const result = await tauriApi.generateInstructionCard(snapshotAbsPath, sid);
    if (proj.id !== snapshotId) return;
    html.value = result.htmlContent;
    ui.toast("success", t("instruct.generated"));
  } catch (e) {
    ui.toast("error", `${e}`);
  } finally {
    isGenerating.value = false;
  }
}

const isExporting = ref(false);

async function openLastPdf() {
  if (!lastSavedPdf.value) return;
  try {
    await openPath(lastSavedPdf.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function exportPdf() {
  if (!projectReady.value || isExporting.value) return;
  const sid = screenId.value;
  if (!sid) {
    ui.toast("error", "no screen in project");
    return;
  }
  isExporting.value = true;
  const snapshotId = proj.id;
  const snapshotAbsPath = proj.absPath!;
  try {
    const defaultPath = `${snapshotAbsPath}/output/instruction-${sid}.pdf`;
    const target = await saveDialog({
      title: t("instruct.exportPdf"),
      defaultPath,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!target) return;
    if (proj.id !== snapshotId) {
      ui.toast("info", "project changed during file pick — export cancelled");
      return;
    }
    const dst = String(target);
    const written = await tauriApi.saveInstructionPdf(snapshotAbsPath, sid, dst);
    if (proj.id !== snapshotId) return;
    lastSavedPdf.value = written;
    ui.toast("success", t("instruct.exportedTo", { path: written }));
  } catch (e) {
    ui.toast("error", `${e}`);
  } finally {
    isExporting.value = false;
  }
}
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtMethodMismatchBanner expects="m1" />
    <LmtPageHeader
      :eyebrow="t('instruct.eyebrow')"
      :title="t('instruct.title')"
      :description="t('instruct.description')"
    >
      <template #actions>
        <LmtStatusBadge tone="healthy" label="M1" icon="check-circle-2" size="md" />
      </template>
    </LmtPageHeader>

    <section class="flex flex-wrap items-center gap-3 rounded-lg border bg-card p-4">
      <Button
        variant="default"
        :disabled="!projectReady || isGenerating || isExporting"
        @click="generate"
      >
        <LmtIcon name="printer" :size="14" />
        {{ t("instruct.generate") }}
      </Button>
      <Button
        variant="outline"
        :disabled="!projectReady || !html || isGenerating || isExporting"
        @click="exportPdf"
      >
        <LmtIcon name="download" :size="14" />
        {{ t("instruct.exportPdf") }}
      </Button>
      <span v-if="lastSavedPdf" class="flex items-center gap-1.5 font-mono text-xs">
        <span class="text-muted-foreground">PDF —</span>
        <button
          type="button"
          class="rounded bg-muted px-2 py-1 text-muted-foreground hover:bg-muted/70 hover:text-foreground hover:underline"
          :title="t('instruct.openPdfTitle')"
          @click="openLastPdf"
        >
          {{ lastSavedPdf }}
        </button>
      </span>
      <span class="ml-auto text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        screen
      </span>
      <span class="font-mono text-xs">{{ screenId }}</span>
    </section>

    <section
      v-if="html"
      class="flex flex-1 flex-col rounded-lg border bg-card overflow-hidden"
    >
      <iframe
        :srcdoc="html"
        class="flex-1 w-full bg-white"
        sandbox=""
      />
    </section>

    <section
      v-else
      class="flex flex-1 flex-col items-center justify-center gap-3 rounded-lg border bg-hatched py-16 text-center"
    >
      <LmtIcon name="printer" :size="40" class="text-muted-foreground" />
      <p class="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("instruct.empty") }}
      </p>
      <p class="max-w-md text-sm text-muted-foreground">
        {{ t("instruct.description") }}
      </p>
    </section>
  </div>
</template>
