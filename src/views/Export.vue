<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useRouter, useRoute } from "vue-router";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import Button from "@/components/ui/Button.vue";

const { t } = useI18n();
const router = useRouter();
const route = useRoute();

const targets: { id: string; icon: string }[] = [
  { id: "disguise", icon: "monitor-cog" },
  { id: "unreal", icon: "gamepad-2" },
  { id: "neutral", icon: "package" },
];

function gotoPreview() {
  router.push(`/projects/${route.params.id}/preview`);
}
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtPageHeader
      :eyebrow="t('export.eyebrow')"
      :title="t('export.title')"
      :description="t('export.description')"
    >
      <template #actions>
        <Button variant="default" @click="gotoPreview">
          <LmtIcon name="arrow-right" :size="14" />
          {{ t("export.goPreview") }}
        </Button>
      </template>
    </LmtPageHeader>

    <section class="grid gap-3 md:grid-cols-3">
      <article
        v-for="tgt in targets"
        :key="tgt.id"
        class="flex flex-col gap-3 rounded-lg border bg-card p-5 transition-colors hover:border-primary/40"
      >
        <div class="flex items-center justify-between">
          <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
            TARGET
          </p>
          <LmtIcon :name="tgt.icon" :size="18" class="text-muted-foreground" />
        </div>
        <div>
          <p class="font-display text-2xl font-extrabold text-foreground">
            {{ t(`export.tile.${tgt.id}.title`) }}
          </p>
          <p class="mt-1 text-sm text-muted-foreground">
            {{ t(`export.tile.${tgt.id}.desc`) }}
          </p>
        </div>
        <div class="mt-auto">
          <Button variant="outline" size="sm" class="w-full" @click="gotoPreview">
            <LmtIcon name="download" :size="13" />
            {{ t("export.goPreview") }}
          </Button>
        </div>
      </article>
    </section>
  </div>
</template>
