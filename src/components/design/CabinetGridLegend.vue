<script setup lang="ts">
import { useI18n } from "vue-i18n";
const { t } = useI18n();

const fillItems = [
  { key: "present", cls: "bg-primary/70 border border-primary" },
  { key: "absent", cls: "bg-muted border border-border" },
];

const refItems = [
  { key: "origin", cls: "border-2 border-status-critical bg-transparent" },
  { key: "x_axis", cls: "border-2 border-status-healthy bg-transparent" },
  { key: "xy_plane", cls: "border-2 border-status-info bg-transparent" },
];
</script>

<template>
  <div class="flex flex-wrap items-center gap-x-4 gap-y-1.5 text-[11px]">
    <span class="flex items-center gap-1.5" v-for="it in fillItems" :key="it.key">
      <span class="inline-block size-3 rounded-sm" :class="it.cls" />
      <span class="font-mono uppercase tracking-wide text-muted-foreground">
        {{ t(`design.legend.${it.key}`) }}
      </span>
    </span>
    <span class="hidden h-3 w-px bg-border sm:inline-block" />
    <span class="flex items-center gap-1.5" v-for="it in refItems" :key="it.key">
      <span class="inline-block size-3 rounded-sm" :class="it.cls" />
      <span class="font-mono uppercase tracking-wide text-muted-foreground">
        {{ t(`design.legend.${it.key}`) }}
      </span>
    </span>
  </div>
</template>
