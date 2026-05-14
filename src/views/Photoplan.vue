<script setup lang="ts">
import { useI18n } from "vue-i18n";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtStatusBadge from "@/components/primitives/LmtStatusBadge.vue";
import LmtMethodMismatchBanner from "@/components/shell/LmtMethodMismatchBanner.vue";

const { t } = useI18n();

defineProps<{ id?: string }>();
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtMethodMismatchBanner expects="m2" />
    <LmtPageHeader
      :eyebrow="t('photoplan.eyebrow')"
      :title="t('photoplan.title')"
      :description="t('photoplan.description')"
    >
      <template #actions>
        <LmtStatusBadge tone="info" label="M2" icon="circle-dot" size="md" />
      </template>
    </LmtPageHeader>

    <section
      class="flex flex-1 flex-col items-center justify-center gap-3 rounded-lg border bg-hatched py-16 text-center"
    >
      <LmtIcon name="camera" :size="40" class="text-muted-foreground" />
      <p class="font-mono text-[11px] uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("photoplan.pending") }}
      </p>
      <p class="max-w-md text-sm text-muted-foreground">
        {{ t("photoplan.description") }}
      </p>
    </section>
  </div>
</template>
