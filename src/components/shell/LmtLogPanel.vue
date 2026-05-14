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
      <div class="flex items-center gap-2">
        <LmtIcon name="terminal" :size="14" />
        <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
          {{ t("shell.activityLog") }}
        </p>
      </div>
      <Button
        variant="ghost"
        size="icon-sm"
        :aria-label="t('shell.closeLog')"
        @click="emit('close')"
      >
        <LmtIcon name="x" :size="14" />
      </Button>
    </header>
    <div class="h-[calc(100%-2.5rem)] overflow-auto px-4 py-2 font-mono text-xs">
      <div
        v-if="rows.length === 0"
        class="flex h-full items-center justify-center rounded-md bg-hatched py-6 text-[11px] uppercase tracking-[0.18em] text-muted-foreground"
      >
        {{ t("shell.noActivity") }}
      </div>
      <div
        v-for="row in rows"
        :key="`${row.time}-${row.text}`"
        class="grid grid-cols-[5rem_4rem_1fr] gap-3 py-1 text-muted-foreground"
      >
        <span>{{ row.time }}</span>
        <span class="uppercase">{{ row.level }}</span>
        <span class="text-foreground">{{ row.text }}</span>
      </div>
    </div>
  </section>
</template>
