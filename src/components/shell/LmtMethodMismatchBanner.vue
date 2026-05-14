<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { useSurveyMethod } from "@/composables/useSurveyMethod";
import LmtBanner from "@/components/primitives/LmtBanner.vue";

const props = defineProps<{
  expects: "m1" | "m2" | "any";
}>();

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const { method } = useSurveyMethod();

const id = computed(() => route.params.id as string);

const mismatch = computed(() => {
  if (props.expects === "any") return method.value === null;
  return method.value !== props.expects;
});

const title = computed(() => {
  if (method.value === null) return t("method.mismatch.unset");
  const current = method.value === "m1" ? t("method.m1.title") : t("method.m2.title");
  if (props.expects === "m1") return t("method.mismatch.m1Only", { current });
  if (props.expects === "m2") return t("method.mismatch.m2Only", { current });
  return "";
});

const key = computed(() => `mismatch-${id.value}-${route.name?.toString()}`);

function goPick() {
  router.push(`/projects/${id.value}/method`);
}
</script>

<template>
  <LmtBanner
    v-if="mismatch"
    tone="warn"
    icon="alert-triangle"
    :title="title"
    :action-label="t('method.mismatch.goPick')"
    :dismiss-key="key"
    @action="goPick"
  />
</template>
