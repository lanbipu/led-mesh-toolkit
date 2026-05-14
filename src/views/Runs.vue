<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useReconstructionStore } from "@/stores/reconstruction";
import { useUiStore } from "@/stores/ui";
import { tauriApi } from "@/services/tauri";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtStatusBadge from "@/components/primitives/LmtStatusBadge.vue";
import LmtMethodMismatchBanner from "@/components/shell/LmtMethodMismatchBanner.vue";

const { t } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const recon = useReconstructionStore();
const ui = useUiStore();
const id = computed(() => Number(route.params.id));
const expanded = ref<number | null>(null);
const reportCache = ref<Record<number, unknown>>({});

function fmtTime(s: string | null | undefined): string {
  if (!s) return "—";
  const d = new Date(s);
  if (Number.isNaN(d.getTime())) return s;
  return d.toISOString().slice(0, 19).replace("T", " ");
}

function fmtPath(s: string | null | undefined): string {
  if (!s) return "—";
  return s.length > 48 ? `…${s.slice(-46)}` : s;
}

function rmsTone(rms: number): "healthy" | "warning" | "critical" {
  if (rms < 3) return "healthy";
  if (rms < 8) return "warning";
  return "critical";
}

async function load() {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
    if (proj.absPath) await recon.loadRuns(proj.absPath);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
}

async function toggle(runId: number) {
  expanded.value = expanded.value === runId ? null : runId;
  if (expanded.value !== null && reportCache.value[runId] === undefined) {
    try {
      reportCache.value[runId] = await tauriApi.getRunReport(runId);
    } catch (e) {
      reportCache.value[runId] = { error: `${e}` };
    }
  }
}

onMounted(load);
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtMethodMismatchBanner expects="any" />
    <LmtPageHeader
      :eyebrow="t('runs.eyebrow')"
      :title="t('runs.title')"
      :description="t('runs.description')"
    >
      <template #actions>
        <span class="font-mono text-[11px] text-muted-foreground">
          {{ recon.recentRuns.length }} runs
        </span>
      </template>
    </LmtPageHeader>

    <div class="flex-1 overflow-hidden rounded-lg border bg-card">
      <div
        v-if="recon.recentRuns.length === 0"
        class="flex h-full flex-col items-center justify-center gap-3 bg-hatched py-12 text-center"
      >
        <LmtIcon name="history" :size="28" class="text-muted-foreground" />
        <p class="max-w-md text-sm text-muted-foreground">{{ t("runs.empty") }}</p>
      </div>

      <div v-else class="h-full overflow-auto">
        <table class="w-full border-collapse text-sm">
          <thead class="sticky top-0 z-10 bg-muted/60 backdrop-blur-none">
            <tr class="border-b text-left text-[11px] font-bold uppercase tracking-wide text-muted-foreground">
              <th class="py-2 pl-5 pr-3 font-bold">{{ t("runs.col.created") }}</th>
              <th class="py-2 px-3 font-bold">{{ t("runs.col.screen") }}</th>
              <th class="py-2 px-3 font-bold">{{ t("runs.col.method") }}</th>
              <th class="py-2 px-3 text-right font-bold">{{ t("runs.col.rms") }}</th>
              <th class="py-2 px-3 text-right font-bold">{{ t("runs.col.vertices") }}</th>
              <th class="py-2 px-3 font-bold">{{ t("runs.col.target") }}</th>
              <th class="py-2 px-3 pr-5 font-bold">{{ t("runs.col.obj") }}</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="r in recon.recentRuns" :key="r.id">
              <tr
                class="cursor-pointer border-b transition-colors even:bg-muted/30 hover:bg-accent/40"
                @click="toggle(r.id)"
              >
                <td class="py-2 pl-5 pr-3 font-mono text-xs tabular-nums">{{ fmtTime(r.created_at) }}</td>
                <td class="py-2 px-3 font-mono text-xs">{{ r.screen_id }}</td>
                <td class="py-2 px-3 font-mono text-xs uppercase">{{ r.method }}</td>
                <td class="py-2 px-3 text-right">
                  <LmtStatusBadge
                    :tone="rmsTone(r.estimated_rms_mm)"
                    :label="r.estimated_rms_mm.toFixed(2)"
                    size="sm"
                    icon="activity"
                  />
                </td>
                <td class="py-2 px-3 text-right font-mono text-xs tabular-nums">
                  {{ r.vertex_count }}
                </td>
                <td class="py-2 px-3 font-mono text-xs">{{ r.target ?? "—" }}</td>
                <td class="py-2 px-3 pr-5 font-mono text-xs text-muted-foreground">
                  <span class="inline-flex items-center gap-1.5">
                    <LmtIcon
                      :name="expanded === r.id ? 'chevron-down' : 'chevron-right'"
                      :size="12"
                    />
                    {{ fmtPath(r.output_obj_path) }}
                  </span>
                </td>
              </tr>
              <tr v-if="expanded === r.id" class="border-b">
                <td colspan="7" class="bg-muted/20 px-5 py-3">
                  <pre
                    class="overflow-auto rounded border bg-background p-3 font-mono text-[11px] leading-relaxed text-muted-foreground"
                  >{{ JSON.stringify(reportCache[r.id], null, 2) }}</pre>
                </td>
              </tr>
            </template>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>
