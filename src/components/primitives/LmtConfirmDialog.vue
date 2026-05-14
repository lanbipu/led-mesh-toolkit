<script setup lang="ts">
import {
  DialogRoot,
  DialogPortal,
  DialogOverlay,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "reka-ui";
import Button from "@/components/ui/Button.vue";

defineProps<{
  open: boolean;
  title: string;
  body: string;
  okLabel: string;
  cancelLabel: string;
  okTone?: "default" | "destructive";
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "confirm"): void;
}>();

function onUpdate(v: boolean) {
  emit("update:open", v);
}

function ok() {
  emit("confirm");
  emit("update:open", false);
}

function cancel() {
  emit("update:open", false);
}
</script>

<template>
  <DialogRoot :open="open" @update:open="onUpdate">
    <DialogPortal>
      <DialogOverlay class="fixed inset-0 z-40 bg-background/60" />
      <DialogContent
        class="fixed left-1/2 top-1/2 z-50 w-[min(420px,92vw)] -translate-x-1/2 -translate-y-1/2 rounded-lg border bg-card p-5"
      >
        <DialogTitle class="font-display text-base font-extrabold text-foreground">
          {{ title }}
        </DialogTitle>
        <DialogDescription class="mt-2 text-sm text-muted-foreground">
          {{ body }}
        </DialogDescription>
        <div class="mt-5 flex justify-end gap-2">
          <Button variant="outline" size="sm" data-confirm-cancel @click="cancel">
            {{ cancelLabel }}
          </Button>
          <Button
            :variant="okTone === 'destructive' ? 'destructive' : 'default'"
            size="sm"
            data-confirm-ok
            @click="ok"
          >
            {{ okLabel }}
          </Button>
        </div>
      </DialogContent>
    </DialogPortal>
  </DialogRoot>
</template>
