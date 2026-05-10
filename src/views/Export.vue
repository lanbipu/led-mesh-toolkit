<script setup lang="ts">
import { useRouter, useRoute } from "vue-router";
const router = useRouter();
const route = useRoute();
</script>

<template>
  <div class="p-8">
    <h1 class="text-2xl font-bold">Export</h1>
    <p class="mt-2 text-sm text-muted-foreground">
      Export OBJ from the Preview view (the toolbar has 3 target buttons).
    </p>
    <button
      class="mt-4 rounded border px-3 py-1"
      @click="router.push(`/projects/${route.params.id}/preview`)"
    >
      Go to Preview
    </button>
  </div>
</template>
