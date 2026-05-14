<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useCurrentProjectStore } from "@/stores/currentProject";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtThemeToggle from "@/components/primitives/LmtThemeToggle.vue";
import LmtLanguageToggle from "@/components/primitives/LmtLanguageToggle.vue";
import Button from "@/components/ui/Button.vue";

defineProps<{ logOpen: boolean }>();
const emit = defineEmits<{ toggleLog: [] }>();

const { t } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();

const eyebrow = computed(() => {
  const seg = (route.name as string | undefined) ?? "home";
  return seg.toUpperCase();
});

const title = computed(() => {
  const path = proj.absPath;
  if (!path) return t("app.title");
  const seg = path.split(/[\\/]/).filter(Boolean).pop();
  return seg ?? t("app.title");
});

const subtitle = computed(() => proj.absPath ?? t("app.tagline"));
</script>

<template>
  <header
    class="flex h-16 shrink-0 items-center justify-between gap-3 border-b bg-background px-6"
  >
    <div class="min-w-0 flex-1">
      <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ eyebrow }}
      </p>
      <p class="truncate font-display text-sm font-extrabold text-foreground">
        {{ title }}
      </p>
      <p v-if="subtitle" class="truncate font-mono text-[11px] text-muted-foreground">
        {{ subtitle }}
      </p>
    </div>

    <div class="flex items-center gap-1">
      <LmtLanguageToggle />
      <LmtThemeToggle />
      <Button
        variant="ghost"
        size="icon-sm"
        :aria-pressed="logOpen"
        :aria-label="t('shell.toggleLog')"
        @click="emit('toggleLog')"
      >
        <LmtIcon :name="logOpen ? 'panel-bottom-close' : 'terminal'" :size="15" />
      </Button>
    </div>
  </header>
</template>
