<script setup lang="ts">
import { computed } from "vue";
import { useCurrentProjectStore } from "@/stores/currentProject";

const props = defineProps<{ modelValue: string }>();
const emit = defineEmits<{ "update:modelValue": [v: string] }>();
const proj = useCurrentProjectStore();
const screens = computed(() => Object.keys(proj.config?.screens ?? {}));
</script>

<template>
  <select
    :value="modelValue"
    class="rounded border bg-background px-2 py-1"
    @change="emit('update:modelValue', ($event.target as HTMLSelectElement).value)"
  >
    <option v-for="id in screens" :key="id" :value="id">{{ id }}</option>
  </select>
</template>
