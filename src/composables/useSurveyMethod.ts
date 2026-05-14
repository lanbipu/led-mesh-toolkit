import { computed } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import type { SurveyMethod } from "@/services/tauri";

export function useSurveyMethod() {
  const proj = useCurrentProjectStore();
  const route = useRoute();

  const method = computed<SurveyMethod | null>(() => {
    const routeId = Number(route.params.id);
    if (!Number.isFinite(routeId) || proj.id !== routeId) return null;
    return proj.config?.project.method ?? null;
  });

  return { method, setMethod: proj.setMethod };
}
