<script setup lang="ts">
import { useI18n } from "vue-i18n";
const { t } = useI18n();
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">{{ t("nav.runs") }}</h1>
    <p class="mt-4 text-muted-foreground">M0.2 stub. Implementation pending.</p>
  </div>
</template>
