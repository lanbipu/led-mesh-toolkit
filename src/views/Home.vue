<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { open } from "@tauri-apps/plugin-dialog";
import { useProjectsStore } from "@/stores/projects";
import { useUiStore } from "@/stores/ui";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import Button from "@/components/ui/Button.vue";

const { t } = useI18n();
const router = useRouter();
const projects = useProjectsStore();
const ui = useUiStore();

onMounted(() => projects.load());

async function createExample(name: string) {
  try {
    const target = await open({ directory: true, title: "Choose where to seed example" });
    if (!target) return;
    const created = await projects.createFromExample(name, target as string);
    router.push(`/projects/${created.id}/design`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function openExisting() {
  try {
    const folder = await open({ directory: true, title: "Open project folder" });
    if (!folder) return;
    const name = String(folder).split(/[/\\]/).pop() ?? "Project";
    const created = await projects.openExisting(folder as string, name);
    router.push(`/projects/${created.id}/design`);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

function fmtDate(s: string | null | undefined): string {
  if (!s) return "—";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  return d.toISOString().slice(0, 16).replace("T", " ");
}
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtPageHeader
      :eyebrow="t('home.eyebrow')"
      :title="t('home.title')"
      :description="t('home.description')"
    >
      <template #actions>
        <Button variant="outline" @click="openExisting">
          <LmtIcon name="folder-open" :size="15" />
          {{ t("home.open_existing") }}
        </Button>
        <Button variant="default" @click="createExample('curved-flat')">
          <LmtIcon name="plus" :size="15" />
          {{ t("home.create_curved_flat") }}
        </Button>
      </template>
    </LmtPageHeader>

    <section class="flex flex-1 flex-col gap-3 overflow-hidden">
      <div class="flex items-center justify-between">
        <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
          {{ t("home.recent") }}
        </p>
        <p class="font-mono text-[11px] text-muted-foreground">
          {{ projects.recent.length }} {{ projects.recent.length === 1 ? "project" : "projects" }}
        </p>
      </div>

      <div
        v-if="projects.recent.length === 0"
        class="flex flex-1 flex-col items-center justify-center gap-3 rounded-lg border bg-hatched py-12 text-center"
      >
        <LmtIcon name="folder-plus" :size="28" class="text-muted-foreground" />
        <p class="max-w-sm text-sm text-muted-foreground">{{ t("home.empty") }}</p>
        <div class="mt-2 flex flex-wrap items-center justify-center gap-2">
          <Button variant="default" size="sm" @click="createExample('curved-flat')">
            <LmtIcon name="plus" :size="14" />
            {{ t("home.create_curved_flat") }}
          </Button>
          <Button variant="default" size="sm" @click="createExample('curved-arc')">
            <LmtIcon name="plus" :size="14" />
            {{ t("home.create_curved_arc") }}
          </Button>
          <Button variant="outline" size="sm" @click="openExisting">
            <LmtIcon name="folder-open" :size="14" />
            {{ t("home.open_existing") }}
          </Button>
        </div>
      </div>

      <ul v-else class="grid auto-rows-min grid-cols-1 gap-3 overflow-auto xl:grid-cols-2">
        <li
          v-for="p in projects.recent"
          :key="p.id"
          class="group flex items-center gap-3 rounded-lg border bg-card p-4 transition-colors hover:border-primary/40 hover:bg-accent/40"
        >
          <RouterLink
            :to="`/projects/${p.id}/design`"
            class="flex min-w-0 flex-1 flex-col gap-1"
          >
            <span class="flex items-center gap-2 truncate font-display text-sm font-bold text-foreground group-hover:text-primary">
              <LmtIcon name="folder" :size="14" />
              {{ p.display_name }}
            </span>
            <span class="truncate font-mono text-[11px] text-muted-foreground">
              {{ p.abs_path }}
            </span>
            <span class="font-mono text-[11px] text-muted-foreground">
              {{ fmtDate(p.last_opened_at) }}
            </span>
          </RouterLink>
          <button
            type="button"
            class="inline-flex size-8 shrink-0 items-center justify-center rounded-md border border-transparent text-muted-foreground transition-colors hover:border-destructive/30 hover:bg-destructive/10 hover:text-destructive"
            :aria-label="t('home.remove')"
            :title="t('home.remove')"
            @click.stop.prevent="projects.remove(p.id)"
          >
            <LmtIcon name="trash-2" :size="14" />
          </button>
        </li>
      </ul>
    </section>

    <section class="rounded-lg border bg-card p-4">
      <div class="flex items-start gap-3">
        <div class="mt-0.5 flex size-8 items-center justify-center rounded-md border bg-secondary text-secondary-foreground">
          <LmtIcon name="sparkles" :size="14" />
        </div>
        <div class="min-w-0 flex-1">
          <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
            {{ t("home.actionsTitle") }}
          </p>
          <p class="mt-1 text-sm text-muted-foreground">{{ t("home.actionsDesc") }}</p>
        </div>
        <div class="flex shrink-0 gap-2">
          <Button variant="outline" size="sm" @click="createExample('curved-flat')">
            {{ t("home.create_curved_flat") }}
          </Button>
          <Button variant="outline" size="sm" @click="createExample('curved-arc')">
            {{ t("home.create_curved_arc") }}
          </Button>
        </div>
      </div>
    </section>
  </div>
</template>
