<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import { useSurveyMethod } from "@/composables/useSurveyMethod";
import type { SurveyMethod } from "@/services/tauri";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtConfirmDialog from "@/components/primitives/LmtConfirmDialog.vue";
import Button from "@/components/ui/Button.vue";

const { t, tm, rt } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const ui = useUiStore();
const { method } = useSurveyMethod();

const id = computed(() => Number(route.params.id));

onMounted(async () => {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
});

const dialogOpen = ref(false);
const pendingTarget = ref<SurveyMethod | null>(null);

function bullets(key: "m1" | "m2"): string[] {
  const arr = tm(`method.${key}.bullets`) as unknown as string[];
  return Array.isArray(arr) ? arr.map((b) => rt(b)) : [];
}

function isCurrent(m: SurveyMethod): boolean {
  return method.value === m;
}

function buttonLabel(m: SurveyMethod): string {
  if (method.value === null) return m === "m1" ? t("method.useM1") : t("method.useM2");
  if (method.value === m) return m === "m1" ? t("method.continueM1") : t("method.continueM2");
  return m === "m1" ? t("method.switchToM1") : t("method.switchToM2");
}

async function onCardAction(m: SurveyMethod) {
  if (method.value === m) return;
  if (method.value === null) {
    await proj.setMethod(m);
    ui.toast("success", "Method set");
    return;
  }
  pendingTarget.value = m;
  dialogOpen.value = true;
}

async function doSwitch() {
  if (!pendingTarget.value) return;
  await proj.setMethod(pendingTarget.value);
  ui.toast("success", "Method switched");
  pendingTarget.value = null;
}

const confirmBody = computed(() => {
  if (!pendingTarget.value) return "";
  const target = pendingTarget.value === "m1" ? t("method.m1.title") : t("method.m2.title");
  return t("method.confirmSwitch.body", { target });
});
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtPageHeader
      :eyebrow="t('method.eyebrow')"
      :title="t('method.title')"
      :description="t('method.description')"
    />

    <section class="grid gap-4 md:grid-cols-2">
      <article
        v-for="m in (['m1', 'm2'] as const)"
        :key="m"
        class="flex flex-col gap-4 rounded-lg border p-5 transition-colors"
        :class="
          isCurrent(m)
            ? 'border-primary bg-primary/5'
            : 'border-border bg-card hover:border-primary/40'
        "
      >
        <div class="flex items-center justify-between">
          <LmtIcon :name="m === 'm1' ? 'radio-tower' : 'scan-eye'" :size="24" />
          <span
            class="rounded-full border px-2 py-0.5 font-display text-[11px] font-bold uppercase tracking-wide"
            :class="
              isCurrent(m)
                ? 'border-primary/30 bg-primary/10 text-primary'
                : 'border-border bg-muted/30 text-muted-foreground'
            "
          >
            {{ isCurrent(m) ? t("method.current") : t("method.available") }}
          </span>
        </div>

        <div>
          <p class="font-display text-2xl font-extrabold text-foreground">
            {{ t(`method.${m}.title`) }}
          </p>
          <p class="mt-1 text-sm text-muted-foreground">
            {{ t(`method.${m}.desc`) }}
          </p>
        </div>

        <ul class="space-y-1 text-xs text-muted-foreground">
          <li v-for="(b, i) in bullets(m)" :key="i" class="flex items-start gap-2">
            <LmtIcon name="check" :size="12" class="mt-0.5 text-status-healthy" />
            <span>{{ b }}</span>
          </li>
        </ul>

        <div class="mt-auto">
          <Button
            :variant="isCurrent(m) ? 'outline' : 'default'"
            size="sm"
            class="w-full"
            :disabled="method === m"
            @click="onCardAction(m)"
          >
            {{ buttonLabel(m) }}
          </Button>
        </div>
      </article>
    </section>

    <p class="text-xs text-muted-foreground">
      <LmtIcon name="info" :size="12" class="mr-1 inline align-text-bottom" />
      {{ t("method.coexistNote") }}
    </p>

    <LmtConfirmDialog
      v-model:open="dialogOpen"
      :title="t('method.confirmSwitch.title')"
      :body="confirmBody"
      :ok-label="t('method.confirmSwitch.ok')"
      :cancel-label="t('method.confirmSwitch.cancel')"
      @confirm="doSwitch"
    />
  </div>
</template>
