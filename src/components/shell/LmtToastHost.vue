<script setup lang="ts">
import { computed } from "vue";
import { useUiStore } from "@/stores/ui";
import LmtIcon from "@/components/primitives/LmtIcon.vue";

const ui = useUiStore();

function toneClass(kind: "success" | "error" | "info"): string {
  if (kind === "success")
    return "border-status-healthy/30 bg-status-healthy/10 text-status-healthy";
  if (kind === "error")
    return "border-status-critical/30 bg-status-critical/10 text-status-critical";
  return "border-status-info/30 bg-status-info/10 text-status-info";
}

function toneIcon(kind: "success" | "error" | "info"): string {
  if (kind === "success") return "check-circle-2";
  if (kind === "error") return "alert-triangle";
  return "info";
}

const toasts = computed(() => ui.toasts);
</script>

<template>
  <div
    class="pointer-events-none fixed inset-x-0 bottom-6 z-50 flex flex-col items-center gap-2 px-4"
  >
    <div
      v-for="t in toasts"
      :key="t.id"
      class="pointer-events-auto flex max-w-xl items-center gap-2.5 rounded-md border bg-card px-4 py-2 font-mono text-xs"
      :class="toneClass(t.kind)"
    >
      <LmtIcon :name="toneIcon(t.kind)" :size="14" />
      <span class="break-all">{{ t.msg }}</span>
    </div>
  </div>
</template>
