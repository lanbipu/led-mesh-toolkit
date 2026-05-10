<script setup lang="ts">
import { onMounted } from "vue";
import { useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { useProjectsStore } from "@/stores/projects";
import { useUiStore } from "@/stores/ui";
import { open } from "@tauri-apps/plugin-dialog";

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
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">{{ t("home.recent") }}</h1>
    <div v-if="projects.recent.length === 0" class="mt-6 text-muted-foreground">
      {{ t("home.empty") }}
    </div>
    <ul v-else class="mt-4 divide-y">
      <li v-for="p in projects.recent" :key="p.id" class="flex items-center gap-4 py-2">
        <RouterLink :to="`/projects/${p.id}/design`" class="flex-1 hover:underline">
          <div class="font-medium">{{ p.display_name }}</div>
          <div class="text-xs text-muted-foreground">{{ p.abs_path }}</div>
        </RouterLink>
        <button class="text-xs text-destructive" @click="projects.remove(p.id)">
          {{ t("home.remove") }}
        </button>
      </li>
    </ul>

    <div class="mt-8 flex flex-wrap gap-3">
      <button class="rounded bg-primary px-4 py-2 text-primary-foreground" @click="createExample('curved-flat')">
        {{ t("home.create_curved_flat") }}
      </button>
      <button class="rounded bg-primary px-4 py-2 text-primary-foreground" @click="createExample('curved-arc')">
        {{ t("home.create_curved_arc") }}
      </button>
      <button class="rounded border px-4 py-2" @click="openExisting">
        {{ t("home.open_existing") }}
      </button>
    </div>
  </div>
</template>
