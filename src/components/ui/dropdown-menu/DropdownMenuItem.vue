<script setup lang="ts">
import { computed } from "vue";
import { DropdownMenuItem, type DropdownMenuItemProps, useForwardProps } from "reka-ui";
import { cn } from "@/lib/utils";

const props = defineProps<DropdownMenuItemProps & { class?: string; inset?: boolean }>();
const delegated = computed(() => {
  const { class: _, inset: __, ...rest } = props;
  return rest;
});
const forwarded = useForwardProps(delegated);
</script>

<template>
  <DropdownMenuItem
    v-bind="forwarded"
    :data-inset="inset || undefined"
    :class="cn('relative flex cursor-default select-none items-center gap-2 rounded-sm px-2 py-1.5 text-sm outline-none focus:bg-accent focus:text-accent-foreground data-[disabled]:pointer-events-none data-[disabled]:opacity-50 data-[inset]:pl-8', props.class)"
  >
    <slot />
  </DropdownMenuItem>
</template>
