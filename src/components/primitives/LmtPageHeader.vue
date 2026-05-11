<script setup lang="ts">
defineProps<{
  title: string;
  eyebrow?: string;
  description?: string;
}>();
</script>

<template>
  <header class="flex flex-wrap items-end justify-between gap-4">
    <div>
      <p v-if="eyebrow" class="text-xs font-bold uppercase tracking-[0.18em] text-muted-foreground">{{ eyebrow }}</p>
      <h1 class="mt-1 font-display text-3xl font-extrabold text-foreground">{{ title }}</h1>
      <p v-if="description" class="mt-2 max-w-3xl text-sm text-muted-foreground">{{ description }}</p>
    </div>
    <div v-if="$slots.actions" class="flex items-center gap-2">
      <slot name="actions" />
    </div>
  </header>
</template>
