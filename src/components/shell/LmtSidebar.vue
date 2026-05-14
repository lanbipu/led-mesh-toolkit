<script setup lang="ts">
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import LmtIcon from "@/components/primitives/LmtIcon.vue";

const route = useRoute();
const { t } = useI18n();

const projectId = computed(() => route.params.id as string | undefined);

type NavGroup = {
  key: string;
  label: string;
  items: { to: string; label: string; icon: string }[];
};

const groups = computed<NavGroup[]>(() => {
  const id = projectId.value;
  if (!id) {
    return [
      {
        key: "workspace",
        label: t("nav.group.workspace"),
        items: [{ to: "/", label: t("nav.home"), icon: "house" }],
      },
    ];
  }
  return [
    {
      key: "workspace",
      label: t("nav.group.workspace"),
      items: [{ to: "/", label: t("nav.home"), icon: "house" }],
    },
    {
      key: "design",
      label: t("nav.group.design"),
      items: [
        { to: `/projects/${id}/design`, label: t("nav.design"), icon: "layout-grid" },
        { to: `/projects/${id}/charuco`, label: t("nav.charuco"), icon: "qr-code" },
        { to: `/projects/${id}/photoplan`, label: t("nav.photoplan"), icon: "camera" },
        { to: `/projects/${id}/import`, label: t("nav.import"), icon: "upload" },
      ],
    },
    {
      key: "output",
      label: t("nav.group.output"),
      items: [
        { to: `/projects/${id}/preview`, label: t("nav.preview"), icon: "box" },
        { to: `/projects/${id}/runs`, label: t("nav.runs"), icon: "list-checks" },
        { to: `/projects/${id}/export`, label: t("nav.export"), icon: "file-output" },
        { to: `/projects/${id}/instruct`, label: t("nav.instruct"), icon: "printer" },
      ],
    },
  ];
});
</script>

<template>
  <aside class="flex w-60 shrink-0 flex-col gap-6 border-r bg-sidebar p-3 text-sidebar-foreground">
    <div class="px-3 pb-1 pt-2">
      <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">LMT</p>
      <p class="mt-0.5 font-display text-sm font-extrabold text-sidebar-foreground">
        {{ t("app.title") }}
      </p>
    </div>

    <nav v-for="g in groups" :key="g.key" class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ g.label }}
      </p>
      <RouterLink
        v-for="it in g.items"
        :key="it.to"
        :to="it.to"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon :name="it.icon" :size="15" />
        <span class="truncate">{{ it.label }}</span>
      </RouterLink>
    </nav>
  </aside>
</template>
