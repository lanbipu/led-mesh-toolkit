<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useEditorStore } from "@/stores/editor";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";

const { t } = useI18n();
const editor = useEditorStore();
const proj = useCurrentProjectStore();
const ui = useUiStore();

async function save() {
  if (!proj.config) return;
  const screenId = Object.keys(proj.config.screens)[0];
  const next = editor.commitMaskToScreen(proj.config.screens[screenId]);
  proj.updateScreen(screenId, next);
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
  <div class="flex items-center gap-2 border-b bg-card p-2">
    <button
      :class="['rounded px-3 py-1 text-sm', editor.mode === 'mask' && 'bg-accent']"
      @click="editor.setMode('mask')"
    >
      {{ t("design.toolbar.mask") }} <kbd>M</kbd>
    </button>
    <button
      :class="['rounded px-3 py-1 text-sm', editor.mode === 'refs' && 'bg-accent']"
      @click="editor.setMode('refs')"
    >
      {{ t("design.toolbar.refs") }} <kbd>R</kbd>
    </button>
    <button
      :class="['rounded px-3 py-1 text-sm', editor.mode === 'baseline' && 'bg-accent']"
      @click="editor.setMode('baseline')"
    >
      {{ t("design.toolbar.baseline") }} <kbd>B</kbd>
    </button>

    <div v-if="editor.mode === 'refs'" class="ml-4 flex gap-1">
      <button
        :class="['rounded border px-2 py-0.5 text-xs', editor.currentRefRole === 'origin' && 'bg-red-500 text-white']"
        @click="editor.setCurrentRefRole('origin')"
      >
        Origin <kbd>1</kbd>
      </button>
      <button
        :class="['rounded border px-2 py-0.5 text-xs', editor.currentRefRole === 'x_axis' && 'bg-green-500 text-white']"
        @click="editor.setCurrentRefRole('x_axis')"
      >
        X-axis <kbd>2</kbd>
      </button>
      <button
        :class="['rounded border px-2 py-0.5 text-xs', editor.currentRefRole === 'xy_plane' && 'bg-blue-500 text-white']"
        @click="editor.setCurrentRefRole('xy_plane')"
      >
        XY-plane <kbd>3</kbd>
      </button>
    </div>

    <div class="ml-auto flex gap-2">
      <button
        :disabled="editor.undoDepth === 0"
        class="rounded border px-2 py-1 text-xs disabled:opacity-50"
        @click="editor.undo()"
      >
        Undo ({{ editor.undoDepth }})
      </button>
      <button
        :disabled="editor.redoDepth === 0"
        class="rounded border px-2 py-1 text-xs disabled:opacity-50"
        @click="editor.redo()"
      >
        Redo ({{ editor.redoDepth }})
      </button>
      <button
        class="rounded bg-primary px-3 py-1 text-sm text-primary-foreground disabled:opacity-50"
        :disabled="editor.undoDepth === 0 && !proj.dirty"
        @click="save"
      >
        Save
      </button>
    </div>
  </div>
</template>
