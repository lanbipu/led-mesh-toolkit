<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useEditorStore } from "@/stores/editor";
import { useUiStore } from "@/stores/ui";
import CabinetGrid from "@/components/design/CabinetGrid.vue";
import CabinetGridLegend from "@/components/design/CabinetGridLegend.vue";
import DesignToolbar from "@/components/design/DesignToolbar.vue";
import ScreenPicker from "@/components/design/ScreenPicker.vue";

const route = useRoute();
const proj = useCurrentProjectStore();
const editor = useEditorStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));
const currentScreenId = ref<string>("MAIN");

async function load() {
  await proj.load(id.value);
  if (proj.config) {
    const ids = Object.keys(proj.config.screens);
    currentScreenId.value = ids[0] ?? "MAIN";
    editor.initFromScreen(
      proj.config.screens[currentScreenId.value],
      proj.config.coordinate_system,
    );
  }
}

watch(currentScreenId, (next) => {
  if (proj.config?.screens[next])
    editor.initFromScreen(proj.config.screens[next], proj.config?.coordinate_system);
});

function onKey(e: KeyboardEvent) {
  if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
  if (e.metaKey || e.ctrlKey) {
    if (e.key.toLowerCase() === "z" && !e.shiftKey) {
      e.preventDefault();
      editor.undo();
    } else if ((e.key.toLowerCase() === "z" && e.shiftKey) || e.key.toLowerCase() === "y") {
      e.preventDefault();
      editor.redo();
    }
    return;
  }
  if (e.key === "m" || e.key === "M") editor.setMode("mask");
  else if (e.key === "r" || e.key === "R") editor.setMode("refs");
  else if (e.key === "b" || e.key === "B") editor.setMode("baseline");
  else if (editor.mode === "refs") {
    if (e.key === "1") editor.setCurrentRefRole("origin");
    else if (e.key === "2") editor.setCurrentRefRole("x_axis");
    else if (e.key === "3") editor.setCurrentRefRole("xy_plane");
  }
}

onMounted(() => {
  load().catch((e) => ui.toast("error", `${e}`));
  window.addEventListener("keydown", onKey);
});
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div class="flex h-full flex-col">
    <DesignToolbar />
    <div class="flex items-center gap-3 border-b bg-card px-4 py-2 text-sm">
      <span>Screen:</span>
      <ScreenPicker v-model="currentScreenId" />
      <CabinetGridLegend class="ml-auto" />
    </div>
    <div class="min-h-0 flex-1 overflow-auto p-4">
      <CabinetGrid />
    </div>
  </div>
</template>
