<script setup lang="ts">
import { computed } from "vue";
import LmtIcon from "./LmtIcon.vue";
import type { LmtSize, LmtTone } from "./types";

const props = withDefaults(
  defineProps<{
    tone?: LmtTone;
    label: string;
    size?: LmtSize;
    icon?: string;
  }>(),
  {
    tone: "unknown",
    size: "md",
  },
);

const cls = computed(() => {
  const tone: Record<LmtTone, string> = {
    healthy: "border-status-healthy/30 bg-status-healthy/10 text-status-healthy",
    warning: "border-status-warning/30 bg-status-warning/10 text-status-warning",
    critical: "border-status-critical/30 bg-status-critical/10 text-status-critical",
    info: "border-status-info/30 bg-status-info/10 text-status-info",
    offline: "border-status-offline/30 bg-status-offline/10 text-muted-foreground",
    unknown: "border-status-unknown/30 bg-status-unknown/10 text-muted-foreground",
    progress: "border-status-info/30 bg-status-info/10 text-status-info",
    na: "border-status-unknown/30 bg-status-unknown/10 text-muted-foreground",
  };
  const size: Record<LmtSize, string> = {
    sm: "h-6 px-2 text-[11px]",
    md: "h-7 px-2.5 text-xs",
    lg: "h-8 px-3 text-sm",
  };
  return `${tone[props.tone]} ${size[props.size]}`;
});

const iconName = computed(() => props.icon ?? (props.tone === "critical" ? "alert-triangle" : props.tone === "healthy" ? "check-circle-2" : "info"));
</script>

<template>
  <span data-status-badge class="inline-flex items-center gap-1.5 rounded-full border font-display font-bold uppercase tracking-wide" :class="cls">
    <LmtIcon :name="iconName" size="13" />
    {{ label }}
  </span>
</template>
