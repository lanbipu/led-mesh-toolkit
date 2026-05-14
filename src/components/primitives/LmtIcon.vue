<script setup lang="ts">
import { computed } from "vue";
import * as Lucide from "lucide-vue-next";

defineOptions({ inheritAttrs: true });

const props = withDefaults(
  defineProps<{ name: string; size?: number | string; stroke?: number | string }>(),
  {
    size: 16,
    stroke: 1.5,
  },
);

function toPascal(kebab: string): string {
  return kebab
    .split("-")
    .filter(Boolean)
    .map((p) => p.charAt(0).toUpperCase() + p.slice(1))
    .join("");
}

const Comp = computed(() => {
  const key = toPascal(props.name) as keyof typeof Lucide;
  return (Lucide as Record<string, unknown>)[key] ?? Lucide.HelpCircle;
});
</script>

<template>
  <component
    :is="Comp"
    :size="Number(size)"
    :stroke-width="Number(stroke)"
    class="inline-flex shrink-0 align-middle"
    aria-hidden="true"
  />
</template>
