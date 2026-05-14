<script setup lang="ts">
import { computed } from "vue";
import { cn } from "@/lib/utils";

const props = withDefaults(
  defineProps<{
    modelValue?: string | number;
    type?: string;
    class?: string;
    placeholder?: string;
  }>(),
  {
    type: "text",
  },
);

const emit = defineEmits<{
  "update:modelValue": [value: string];
}>();

const cls = computed(() =>
  cn(
    "flex h-9 w-full min-w-0 rounded-md border border-input bg-transparent px-3 py-1 text-sm outline-none transition-colors placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-50 focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/50",
    props.class,
  ),
);
</script>

<template>
  <input
    :type="type"
    :value="modelValue"
    :placeholder="placeholder"
    :class="cls"
    @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
  />
</template>
