<script setup lang="ts">
defineProps<{ label: string; value?: string | number | null }>();
</script>

<template>
  <div class="min-w-0">
    <dt class="text-[11px] font-bold uppercase tracking-wide text-muted-foreground">{{ label }}</dt>
    <dd class="mt-1 break-words font-mono text-xs text-foreground">
      <slot>{{ value ?? "-" }}</slot>
    </dd>
  </div>
</template>
