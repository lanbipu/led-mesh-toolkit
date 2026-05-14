<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { cva } from "class-variance-authority";
import { useEditorStore, type EditorMode, type RefRole } from "@/stores/editor";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import Button from "@/components/ui/Button.vue";

const props = defineProps<{ screenId: string }>();

const { t } = useI18n();
const editor = useEditorStore();
const proj = useCurrentProjectStore();
const ui = useUiStore();

const segItem = cva(
  "inline-flex h-8 items-center gap-1.5 border-y border-r px-3 font-display text-xs font-bold uppercase tracking-wide transition-colors first:rounded-l-md first:border-l last:rounded-r-md focus-visible:relative focus-visible:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
  {
    variants: {
      active: {
        true: "bg-primary text-primary-foreground border-primary",
        false: "bg-card text-muted-foreground hover:bg-accent hover:text-accent-foreground",
      },
    },
    defaultVariants: { active: false },
  },
);

const refItem = cva(
  "inline-flex h-7 items-center gap-1.5 rounded-md border px-2.5 font-mono text-[11px] font-bold uppercase tracking-wide transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
  {
    variants: {
      tone: {
        idle: "border-border bg-card text-muted-foreground hover:bg-accent hover:text-accent-foreground",
        origin: "border-status-critical/40 bg-status-critical/10 text-status-critical",
        x_axis: "border-status-healthy/40 bg-status-healthy/10 text-status-healthy",
        xy_plane: "border-status-info/40 bg-status-info/10 text-status-info",
      },
    },
    defaultVariants: { tone: "idle" },
  },
);

const modes: { value: EditorMode; label: string; key: string; icon: string }[] = [
  { value: "mask", label: t("design.toolbar.mask"), key: "M", icon: "eraser" },
  { value: "refs", label: t("design.toolbar.refs"), key: "R", icon: "target" },
  { value: "baseline", label: t("design.toolbar.baseline"), key: "B", icon: "minus" },
];

const refRoles: { value: RefRole; label: string; key: string }[] = [
  { value: "origin", label: t("design.ref.origin"), key: "1" },
  { value: "x_axis", label: t("design.ref.x_axis"), key: "2" },
  { value: "xy_plane", label: t("design.ref.xy_plane"), key: "3" },
];

const canSave = computed(() => editor.undoDepth > 0 || proj.dirty);

async function save() {
  if (!proj.config) return;
  const screen = proj.config.screens[props.screenId];
  if (!screen) {
    ui.toast("error", `Screen "${props.screenId}" not found in project`);
    return;
  }
  const next = editor.commitToScreen(screen);
  proj.updateScreen(props.screenId, next);
  proj.updateCoordinateSystem({
    origin_point: editor.refs.origin ?? proj.config.coordinate_system.origin_point,
    x_axis_point: editor.refs.x_axis ?? proj.config.coordinate_system.x_axis_point,
    xy_plane_point: editor.refs.xy_plane ?? proj.config.coordinate_system.xy_plane_point,
  });
  await proj.save();
  editor.clearStacks();
  ui.toast("success", "Saved");
}
</script>

<template>
  <div class="flex flex-wrap items-center gap-3 border-b bg-background px-6 py-2.5">
    <div class="flex items-center gap-2">
      <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("design.mode") }}
      </p>
      <div class="inline-flex">
        <button
          v-for="m in modes"
          :key="m.value"
          type="button"
          :class="segItem({ active: editor.mode === m.value })"
          @click="editor.setMode(m.value)"
        >
          <LmtIcon :name="m.icon" :size="13" />
          {{ m.label }}
          <kbd
            class="ml-1 rounded border bg-background/50 px-1 font-mono text-[10px] text-muted-foreground"
          >{{ m.key }}</kbd>
        </button>
      </div>
    </div>

    <div v-if="editor.mode === 'refs'" class="flex items-center gap-2">
      <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("design.refRole") }}
      </p>
      <div class="flex gap-1">
        <button
          v-for="r in refRoles"
          :key="r.value"
          type="button"
          :class="
            refItem({ tone: editor.currentRefRole === r.value ? (r.value as 'origin' | 'x_axis' | 'xy_plane') : 'idle' })
          "
          @click="editor.setCurrentRefRole(r.value)"
        >
          {{ r.label }}
          <kbd class="font-mono text-[10px] text-muted-foreground">{{ r.key }}</kbd>
        </button>
      </div>
    </div>

    <div class="ml-auto flex items-center gap-2">
      <Button
        variant="outline"
        size="sm"
        :disabled="editor.undoDepth === 0"
        @click="editor.undo()"
      >
        <LmtIcon name="undo-2" :size="13" />
        {{ t("design.undo") }}
        <span class="font-mono text-[10px] text-muted-foreground">{{ editor.undoDepth }}</span>
      </Button>
      <Button
        variant="outline"
        size="sm"
        :disabled="editor.redoDepth === 0"
        @click="editor.redo()"
      >
        <LmtIcon name="redo-2" :size="13" />
        {{ t("design.redo") }}
        <span class="font-mono text-[10px] text-muted-foreground">{{ editor.redoDepth }}</span>
      </Button>
      <Button variant="default" size="sm" :disabled="!canSave" @click="save">
        <LmtIcon name="save" :size="13" />
        {{ t("design.save") }}
      </Button>
    </div>
  </div>
</template>
