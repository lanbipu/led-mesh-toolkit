<script setup lang="ts">
import { useUiStore } from "@/stores/ui";
const ui = useUiStore();
</script>

<template>
  <div class="pointer-events-none fixed inset-x-0 bottom-4 z-50 flex flex-col items-center gap-2 px-4">
    <div
      v-for="t in ui.toasts"
      :key="t.id"
      class="pointer-events-auto rounded-md px-4 py-2 text-sm shadow-lg"
      :class="{
        'bg-emerald-600 text-white': t.kind === 'success',
        'bg-rose-600 text-white': t.kind === 'error',
        'bg-zinc-800 text-zinc-100': t.kind === 'info',
      }"
    >
      {{ t.msg }}
    </div>
  </div>
</template>
