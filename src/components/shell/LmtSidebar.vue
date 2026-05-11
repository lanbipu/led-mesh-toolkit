<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";

const route = useRoute();
const { t } = useI18n();

const projectId = computed(() => route.params.id as string | undefined);

const items = computed(() => {
  const id = projectId.value;
  if (!id) return [{ to: "/", label: t("nav.home") }];
  return [
    { to: "/", label: t("nav.home") },
    { to: `/projects/${id}/design`, label: t("nav.design") },
    { to: `/projects/${id}/import`, label: t("nav.import") },
    { to: `/projects/${id}/preview`, label: t("nav.preview") },
    { to: `/projects/${id}/export`, label: t("nav.export") },
    { to: `/projects/${id}/runs`, label: t("nav.runs") },
    { to: `/projects/${id}/instruct`, label: t("nav.instruct") },
    { to: `/projects/${id}/charuco`, label: t("nav.charuco") },
    { to: `/projects/${id}/photoplan`, label: t("nav.photoplan") },
  ];
});
</script>

<template>
  <nav class="flex w-56 flex-col gap-1 border-r bg-card p-3">
    <RouterLink
      v-for="it in items"
      :key="it.to"
      :to="it.to"
      class="rounded px-3 py-2 text-sm hover:bg-accent"
      active-class="bg-accent font-semibold"
    >
      {{ it.label }}
    </RouterLink>
  </nav>
</template>
