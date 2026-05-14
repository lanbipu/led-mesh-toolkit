<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useUiStore } from "@/stores/ui";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import PreviewToolbar from "@/components/preview/PreviewToolbar.vue";
import MeshPreview from "@/components/preview/MeshPreview.vue";

const { t } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));

const hasSurface = computed(() => recon.currentSurface != null);
const vertexCount = computed(() => recon.currentSurface?.vertices?.length ?? 0);

onMounted(async () => {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
});
</script>

<template>
  <div class="flex h-full flex-col">
    <div class="px-6 pb-2 pt-5">
      <LmtPageHeader
        :eyebrow="t('preview.eyebrow')"
        :title="t('preview.title')"
        :description="t('preview.description')"
      />
    </div>

    <PreviewToolbar />

    <div class="min-h-0 flex-1 p-6">
      <div class="relative h-full overflow-hidden rounded-lg border bg-card">
        <div
          v-if="!hasSurface"
          class="flex h-full flex-col items-center justify-center gap-3 bg-hatched text-center"
        >
          <LmtIcon name="box" :size="28" class="text-muted-foreground" />
          <p class="max-w-sm text-sm text-muted-foreground">
            {{ t("preview.description") }}
          </p>
          <p class="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
            {{ recon.canReconstruct ? t("preview.reconstruct") : t("import.title") }}
          </p>
        </div>
        <MeshPreview v-else :surface="recon.currentSurface" />
        <div
          v-if="hasSurface"
          class="pointer-events-none absolute bottom-3 left-3 flex items-center gap-2 rounded-md border bg-background/80 px-2.5 py-1 font-mono text-[11px] text-muted-foreground backdrop-blur-none"
        >
          <LmtIcon name="hash" :size="12" />
          {{ vertexCount }} vertices
        </div>
      </div>
    </div>
  </div>
</template>
