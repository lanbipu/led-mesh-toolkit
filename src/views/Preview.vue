<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useUiStore } from "@/stores/ui";
import PreviewToolbar from "@/components/preview/PreviewToolbar.vue";
import MeshPreview from "@/components/preview/MeshPreview.vue";

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
</script>

<template>
  <div class="flex h-full flex-col">
    <PreviewToolbar />
    <div class="min-h-0 flex-1">
      <MeshPreview :surface="recon.currentSurface" />
    </div>
  </div>
</template>
