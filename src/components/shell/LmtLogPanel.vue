<script setup lang="ts">
import { useI18n } from "vue-i18n";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import Button from "@/components/ui/Button.vue";

const emit = defineEmits<{ close: [] }>();
const { t } = useI18n();

const rows: { time: string; level: string; text: string }[] = [];
</script>

<template>
  <section class="h-48 shrink-0 border-t bg-card">
    <header class="flex h-10 items-center justify-between border-b px-4">
      <div class="flex items-center gap-2 text-sm font-bold">
        <LmtIcon name="terminal" />
        {{ t("shell.activityLog") }}
      </div>
      <Button variant="ghost" size="icon-sm" @click="emit('close')">
        <LmtIcon name="x" />
      </Button>
    </header>
    <div class="h-[calc(100%-2.5rem)] overflow-auto p-3 font-mono text-xs">
      <p v-if="rows.length === 0" class="py-3 text-muted-foreground">
        {{ t("shell.noActivity") }}
      </p>
      <div v-for="row in rows" :key="`${row.time}-${row.text}`" class="grid grid-cols-[5rem_4rem_1fr] gap-3 py-1 text-muted-foreground">
        <span>{{ row.time }}</span>
        <span class="uppercase">{{ row.level }}</span>
        <span class="text-foreground">{{ row.text }}</span>
      </div>
    </div>
  </section>
</template>
