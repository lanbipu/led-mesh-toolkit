<script setup lang="ts">
import LmtIcon from "./LmtIcon.vue";
import Button from "@/components/ui/Button.vue";
import { useColorMode, type ThemeMode } from "@/composables/useColorMode";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";

const { mode } = useColorMode();

const options: { value: ThemeMode; label: string; icon: string }[] = [
  { value: "dark", label: "Dark", icon: "moon" },
  { value: "light", label: "Light", icon: "sun" },
  { value: "system", label: "System", icon: "monitor" },
];
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button variant="ghost" size="icon-sm" aria-label="Theme">
        <LmtIcon :name="mode === 'light' ? 'sun' : mode === 'system' ? 'monitor' : 'moon'" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end">
      <DropdownMenuItem v-for="option in options" :key="option.value" @select="mode = option.value">
        <LmtIcon :name="option.icon" />
        {{ option.label }}
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
