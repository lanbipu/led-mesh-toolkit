<script setup lang="ts">
import { computed } from "vue";
import { useUiStore } from "@/stores/ui";
import LmtIcon from "./LmtIcon.vue";

type Tone = "info" | "warn";

const props = withDefaults(
  defineProps<{
    tone?: Tone;
    icon?: string;
    title: string;
    actionLabel?: string;
    dismissKey: string;
  }>(),
  { tone: "info" },
);

const emit = defineEmits<{
  (e: "action"): void;
}>();

const ui = useUiStore();

const dismissed = computed(() => ui.isBannerDismissed(props.dismissKey));

const toneClass = computed(() => {
  return props.tone === "warn"
    ? "border-amber-500/30 bg-amber-500/10 text-amber-500"
    : "border-status-info/30 bg-status-info/10 text-status-info";
});

const iconName = computed(() => props.icon ?? (props.tone === "warn" ? "alert-triangle" : "info"));

function dismiss() {
  ui.dismissBanner(props.dismissKey);
}
</script>

<template>
  <div
    v-if="!dismissed"
    data-banner
    class="flex items-center gap-3 rounded-md border px-4 py-2"
    :class="toneClass"
  >
    <LmtIcon :name="iconName" :size="15" class="shrink-0" />
    <span class="flex-1 text-sm leading-snug">{{ title }}</span>
    <button
      v-if="actionLabel"
      type="button"
      data-banner-action
      class="rounded-md border border-current/30 px-2.5 py-1 font-display text-xs font-bold uppercase tracking-wide hover:bg-current/10"
      @click="emit('action')"
    >
      {{ actionLabel }}
    </button>
    <button
      type="button"
      data-banner-dismiss
      :aria-label="'Dismiss'"
      class="inline-flex size-6 items-center justify-center rounded-md hover:bg-current/10"
      @click="dismiss"
    >
      <LmtIcon name="x" :size="13" />
    </button>
  </div>
</template>
