<script setup lang="ts">
import { computed } from "vue";
import { useEditorStore } from "@/stores/editor";

const props = defineProps<{
  cellPx?: number;
}>();

const editor = useEditorStore();
const cell = computed(() => props.cellPx ?? Math.max(8, Math.min(24, Math.floor(960 / Math.max(1, editor.cols)))));

const stageWidth = computed(() => editor.cols * cell.value + 80);
const stageHeight = computed(() => editor.rows * cell.value + 80);

interface CellModel {
  col: number;
  row: number;
  x: number;
  y: number;
  absent: boolean;
  refRole: "origin" | "x_axis" | "xy_plane" | null;
  belowBaseline: boolean;
  name: string;
}

const cells = computed<CellModel[]>(() => {
  const out: CellModel[] = [];
  for (let r = 1; r <= editor.rows; r++) {
    for (let c = 1; c <= editor.cols; c++) {
      const name = `MAIN_V${String(c).padStart(3, "0")}_R${String(r).padStart(3, "0")}`;
      let role: CellModel["refRole"] = null;
      if (editor.refs.origin === name) role = "origin";
      else if (editor.refs.x_axis === name) role = "x_axis";
      else if (editor.refs.xy_plane === name) role = "xy_plane";
      out.push({
        col: c,
        row: r,
        // Konva y points down; R001 at bottom, so y = (rows - r) * cell
        x: 40 + (c - 1) * cell.value,
        y: 40 + (editor.rows - r) * cell.value,
        absent: editor.isAbsent(c, r),
        refRole: role,
        belowBaseline: editor.baselineRow !== null && r < editor.baselineRow,
        name,
      });
    }
  }
  return out;
});

function fillFor(c: CellModel): string {
  if (c.absent) return "#3f3f46";
  if (c.belowBaseline) return "#1e293b";
  return "#0ea5e9";
}
function strokeFor(c: CellModel): string {
  if (c.refRole === "origin") return "#ef4444";
  if (c.refRole === "x_axis") return "#22c55e";
  if (c.refRole === "xy_plane") return "#3b82f6";
  return "#1e293b";
}
function strokeWidthFor(c: CellModel): number {
  return c.refRole !== null ? 3 : 1;
}

function onCellClick(c: CellModel) {
  if (editor.mode === "mask") {
    editor.toggleCell(c.col, c.row);
  } else if (editor.mode === "refs") {
    editor.setRef(editor.currentRefRole, c.name);
  } else if (editor.mode === "baseline") {
    editor.setBaseline(c.row);
  }
}

function baselineY(): number[] {
  if (editor.baselineRow === null) return [];
  const y = 40 + (editor.rows - editor.baselineRow + 1) * cell.value;
  return [40, y, 40 + editor.cols * cell.value, y];
}
</script>

<template>
  <v-stage :config="{ width: stageWidth, height: stageHeight }">
    <v-layer>
      <v-rect
        v-for="c in cells"
        :key="c.name"
        :config="{
          x: c.x,
          y: c.y,
          width: cell - 1,
          height: cell - 1,
          fill: fillFor(c),
          stroke: strokeFor(c),
          strokeWidth: strokeWidthFor(c),
        }"
        @click="onCellClick(c)"
        @tap="onCellClick(c)"
      />
      <v-line
        v-if="editor.baselineRow !== null"
        :config="{
          points: baselineY(),
          stroke: '#fbbf24',
          strokeWidth: 2,
          dash: [6, 6],
        }"
      />
    </v-layer>
  </v-stage>
</template>
