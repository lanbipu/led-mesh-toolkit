<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtStatusBadge from "@/components/primitives/LmtStatusBadge.vue";
import Button from "@/components/ui/Button.vue";

const { t } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const ui = useUiStore();

const id = computed(() => Number(route.params.id));
const html = ref<string | null>(null);
const pdfPath = ref<string | null>(null);

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
    if (proj.id !== snapshotId) return; // discard stale
    html.value = result.htmlContent;
    pdfPath.value = result.pdfPath;
    ui.toast("success", t("instruct.generated"));
  } catch (e) {
    ui.toast("error", `${e}`);
  } finally {
    isGenerating.value = false;
  }
}
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
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
      <Button variant="default" :disabled="!projectReady || isGenerating" @click="generate">
        <LmtIcon name="printer" :size="14" />
        {{ t("instruct.generate") }}
      </Button>
      <span class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        screen
      </span>
      <span class="font-mono text-xs">{{ screenId }}</span>
      <span
        v-if="pdfPath"
        class="ml-auto rounded bg-muted px-2 py-1 font-mono text-[11px] text-muted-foreground"
      >
        PDF → {{ pdfPath }}
      </span>
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
