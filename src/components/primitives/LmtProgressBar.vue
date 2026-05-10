<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(
  defineProps<{
    value?: number | null;
    label?: string | null;
    indeterminate?: boolean;
  }>(),
  {
    value: null,
    label: null,
    indeterminate: false,
  },
);

const pct = computed(() => {
  if (props.value == null || Number.isNaN(props.value)) return null;
  return Math.max(0, Math.min(100, props.value <= 1 ? props.value * 100 : props.value));
});

const width = computed(() => `${pct.value ?? 35}%`);
</script>

<template>
  <div data-progress-bar class="space-y-1">
    <div class="flex h-4 items-center justify-between text-[11px] font-bold text-muted-foreground">
      <span>{{ label ?? (pct == null ? "Running" : "Progress") }}</span>
      <span v-if="pct != null">{{ Math.round(pct) }}%</span>
    </div>
    <div class="h-2 overflow-hidden rounded-full bg-muted">
      <div
        class="h-full rounded-full bg-primary transition-all"
        :class="{ 'animate-pulse': indeterminate || pct == null }"
        :style="{ width }"
      ></div>
    </div>
  </div>
</template>
