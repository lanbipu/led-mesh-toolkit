<script setup lang="ts">
import { computed } from "vue";
import LmtIcon from "./LmtIcon.vue";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    label?: string;
    placeholder?: string;
    valid?: boolean | null;
  }>(),
  {
    label: "Path",
    placeholder: "D:\\Work\\Project",
    valid: null,
  },
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "validate"): void;
}>();

const validityClass = computed(() => {
  if (props.valid === true) return "border-status-healthy/60";
  if (props.valid === false) return "border-status-critical/60";
  return "border-input";
});
</script>

<template>
  <label data-path-input class="block space-y-1">
    <span class="text-xs font-bold text-muted-foreground">{{ label }}</span>
    <span class="flex items-center gap-2">
      <input
        class="h-9 min-w-0 flex-1 rounded-md border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        :class="validityClass"
        :value="modelValue"
        :placeholder="placeholder"
        @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      />
      <button
        type="button"
        data-path-validate
        class="inline-flex size-9 items-center justify-center rounded-md border text-muted-foreground hover:bg-accent hover:text-accent-foreground"
        title="Validate path"
        @click="emit('validate')"
      >
        <LmtIcon name="check" size="15" />
      </button>
    </span>
  </label>
</template>
