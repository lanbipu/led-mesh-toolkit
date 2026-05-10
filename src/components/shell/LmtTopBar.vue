<script setup lang="ts">
import { useI18n } from "vue-i18n";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtThemeToggle from "@/components/primitives/LmtThemeToggle.vue";
import LmtLanguageToggle from "@/components/primitives/LmtLanguageToggle.vue";
import Button from "@/components/ui/Button.vue";

defineProps<{ logOpen: boolean }>();
const emit = defineEmits<{ toggleLog: [] }>();
const { t } = useI18n();
</script>

<template>
  <header class="flex h-14 shrink-0 items-center justify-between gap-3 border-b bg-background px-4">
    <span class="font-display text-sm font-bold text-foreground">{{ t("app.title") }}</span>

    <div class="flex items-center gap-2">
      <LmtLanguageToggle />
      <LmtThemeToggle />
      <Button variant="ghost" size="icon-sm" :aria-pressed="logOpen" @click="emit('toggleLog')">
        <LmtIcon :name="logOpen ? 'panel-bottom-close' : 'terminal'" />
      </Button>
    </div>
  </header>
</template>
