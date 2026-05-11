<script setup lang="ts">
import { ref } from "vue";
import { RouterView } from "vue-router";
import LmtSidebar from "./LmtSidebar.vue";
import LmtTopBar from "./LmtTopBar.vue";
import LmtLogPanel from "./LmtLogPanel.vue";
import LmtToastHost from "./LmtToastHost.vue";

const logOpen = ref(false);
</script>

<template>
  <div class="flex h-full w-full overflow-hidden bg-background text-foreground">
    <LmtSidebar />
    <div class="flex min-w-0 flex-1 flex-col overflow-hidden">
      <LmtTopBar :log-open="logOpen" @toggle-log="logOpen = !logOpen" />
      <main class="min-h-0 flex-1 overflow-auto">
        <RouterView />
      </main>
      <LmtLogPanel v-if="logOpen" @close="logOpen = false" />
    </div>
    <LmtToastHost />
  </div>
</template>
