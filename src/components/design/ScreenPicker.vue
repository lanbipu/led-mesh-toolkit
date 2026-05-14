<script setup lang="ts">
import { computed } from "vue";
import { useCurrentProjectStore } from "@/stores/currentProject";
import LmtIcon from "@/components/primitives/LmtIcon.vue";

const props = defineProps<{ modelValue: string }>();
const emit = defineEmits<{ "update:modelValue": [v: string] }>();
const proj = useCurrentProjectStore();
const screens = computed(() => Object.keys(proj.config?.screens ?? {}));
</script>

<template>
  <div class="relative inline-flex items-center">
    <LmtIcon
      name="monitor"
      :size="13"
      class="pointer-events-none absolute left-2.5 text-muted-foreground"
    />
    <select
      :value="props.modelValue"
      class="h-8 appearance-none rounded-md border bg-card pl-8 pr-8 font-mono text-xs text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
      @change="emit('update:modelValue', ($event.target as HTMLSelectElement).value)"
    >
      <option v-for="id in screens" :key="id" :value="id">{{ id }}</option>
    </select>
    <LmtIcon
      name="chevron-down"
      :size="13"
      class="pointer-events-none absolute right-2 text-muted-foreground"
    />
  </div>
</template>
