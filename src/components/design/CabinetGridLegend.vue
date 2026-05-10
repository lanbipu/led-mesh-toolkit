<script setup lang="ts">
import { useI18n } from "vue-i18n";
const { t } = useI18n();
</script>

<template>
  <div class="flex flex-wrap gap-4 text-xs">
    <span class="flex items-center gap-1">
      <span class="inline-block h-3 w-3 bg-sky-500"></span>
      {{ t("design.legend.present") }}
    </span>
    <span class="flex items-center gap-1">
      <span class="inline-block h-3 w-3 bg-zinc-700"></span>
      {{ t("design.legend.absent") }}
    </span>
    <span class="flex items-center gap-1">
      <span class="inline-block h-3 w-3 border-2 border-red-500"></span>
      {{ t("design.legend.origin") }}
    </span>
    <span class="flex items-center gap-1">
      <span class="inline-block h-3 w-3 border-2 border-green-500"></span>
      {{ t("design.legend.x_axis") }}
    </span>
    <span class="flex items-center gap-1">
      <span class="inline-block h-3 w-3 border-2 border-blue-500"></span>
      {{ t("design.legend.xy_plane") }}
    </span>
  </div>
</template>
