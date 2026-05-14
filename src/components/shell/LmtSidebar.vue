<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useProjectsStore } from "@/stores/projects";
import { useSurveyMethod } from "@/composables/useSurveyMethod";
import LmtIcon from "@/components/primitives/LmtIcon.vue";

const route = useRoute();
const { t } = useI18n();
const projects = useProjectsStore();
// currentProject subscription happens via useSurveyMethod (composable owns the race-safe read).
const { method } = useSurveyMethod();

onMounted(() => {
  projects.load().catch(() => {});
});

const projectId = computed(() => (route.params.id as string | undefined) ?? null);

const recentTop5 = computed(() => projects.recent.slice(0, 5));
const pinnedId = computed(() => recentTop5.value[0]?.id ?? null);

type NavItem = { to: string; label: string; icon: string };

const surveyItems = computed<NavItem[]>(() => {
  if (!projectId.value) return [];
  if (method.value === "m1") {
    return [
      { to: `/projects/${projectId.value}/import`, label: t("nav.import"), icon: "upload" },
    ];
  }
  if (method.value === "m2") {
    return [
      { to: `/projects/${projectId.value}/charuco`, label: t("nav.charuco"), icon: "qr-code" },
      { to: `/projects/${projectId.value}/photoplan`, label: t("nav.photoplan"), icon: "camera" },
    ];
  }
  return [];
});

const outputItems = computed<NavItem[]>(() => {
  if (!projectId.value) return [];
  return [
    { to: `/projects/${projectId.value}/preview`, label: t("nav.preview"), icon: "box" },
    { to: `/projects/${projectId.value}/instruct`, label: t("nav.instruct"), icon: "printer" },
    { to: `/projects/${projectId.value}/runs`, label: t("nav.runs"), icon: "list-checks" },
  ];
});

const outputDimmed = computed(() => !!projectId.value && method.value === null);
</script>

<template>
  <aside class="flex w-60 shrink-0 flex-col gap-6 border-r bg-sidebar p-3 text-sidebar-foreground">
    <div class="px-3 pb-1 pt-2">
      <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">LMT</p>
      <p class="mt-0.5 font-display text-sm font-extrabold text-sidebar-foreground">
        {{ t("app.title") }}
      </p>
    </div>

    <!-- Workspace -->
    <nav class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.workspace") }}
      </p>
      <RouterLink
        to="/"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon name="house" :size="15" />
        <span class="truncate">{{ t("nav.home") }}</span>
      </RouterLink>
    </nav>

    <!-- Home: Recent Projects -->
    <nav v-if="!projectId && recentTop5.length > 0" class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("home.recentProjects") }}
        <span class="font-mono text-[10px] text-muted-foreground/70">({{ recentTop5.length }})</span>
      </p>
      <RouterLink
        v-for="p in recentTop5"
        :key="p.id"
        data-recent-project
        :to="`/projects/${p.id}/design`"
        class="group flex items-center gap-2 truncate rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
      >
        <LmtIcon
          :name="p.id === pinnedId ? 'diamond' : 'folder'"
          :size="13"
          :class="p.id === pinnedId ? 'text-primary' : 'text-muted-foreground'"
        />
        <span class="truncate" :class="p.id === pinnedId ? 'font-bold text-foreground' : ''">
          {{ p.display_name }}
        </span>
      </RouterLink>
    </nav>

    <!-- Project-internal: Design group -->
    <nav v-if="projectId" class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.design") }}
      </p>
      <RouterLink
        :to="`/projects/${projectId}/design`"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon name="layout-grid" :size="15" />
        <span class="truncate">{{ t("nav.design") }}</span>
      </RouterLink>
      <RouterLink
        :to="`/projects/${projectId}/method`"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon name="compass" :size="15" />
        <span class="flex-1 truncate">{{ t("nav.method") }}</span>
        <LmtIcon
          v-if="method === null"
          name="diamond"
          :size="12"
          class="text-status-critical"
        />
        <span
          v-else
          class="font-mono text-[10px] uppercase tracking-wide text-muted-foreground"
        >{{ method }}</span>
      </RouterLink>
    </nav>

    <!-- Project-internal: Survey group (method-driven) -->
    <nav v-if="projectId && surveyItems.length > 0" class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.survey") }}
      </p>
      <RouterLink
        v-for="it in surveyItems"
        :key="it.to"
        :to="it.to"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon :name="it.icon" :size="15" />
        <span class="truncate">{{ it.label }}</span>
      </RouterLink>
    </nav>

    <!-- Project-internal: Output group (always renders; dimmed when method=null) -->
    <nav
      v-if="projectId"
      class="flex flex-col gap-0.5"
      :class="outputDimmed ? 'opacity-50' : ''"
    >
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.output") }}
      </p>
      <RouterLink
        v-for="it in outputItems"
        :key="it.to"
        data-output-item
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
