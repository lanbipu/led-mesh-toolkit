<script setup lang="ts">
withDefaults(defineProps<{ name: string; size?: number | string; stroke?: number | string }>(), {
  size: 16,
  stroke: 1.5,
});
</script>

<template>
  <!-- placeholder: real icons wired in Phase 5 -->
  <span class="inline-flex shrink-0 items-center justify-center align-middle" aria-hidden="true" />
</template>
