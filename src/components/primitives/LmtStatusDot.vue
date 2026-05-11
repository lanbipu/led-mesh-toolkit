<script setup lang="ts">
import { computed } from "vue";
import type { LmtTone } from "./types";

const props = withDefaults(
  defineProps<{
    tone?: LmtTone;
    pulse?: boolean;
  }>(),
  {
    tone: "unknown",
    pulse: false,
  },
);

const toneClass = computed(() => {
  const tones: Record<LmtTone, string> = {
    healthy: "bg-status-healthy",
    warning: "bg-status-warning",
    critical: "bg-status-critical",
    info: "bg-status-info",
    offline: "bg-status-offline",
    unknown: "bg-status-unknown",
    progress: "bg-status-info",
    na: "bg-status-unknown",
  };
  return tones[props.tone];
});
</script>

<template>
  <span class="relative inline-flex size-2.5">
    <span v-if="pulse" class="absolute inline-flex size-full animate-ping rounded-full opacity-50" :class="toneClass" />
    <span class="relative inline-flex size-2.5 rounded-full" :class="toneClass" />
  </span>
</template>
