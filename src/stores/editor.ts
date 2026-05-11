import { defineStore } from "pinia";
import { computed, ref } from "vue";
import type { CoordinateSystemConfig, ScreenConfig } from "@/services/tauri";

export type EditorMode = "mask" | "refs" | "baseline";
export type RefRole = "origin" | "x_axis" | "xy_plane";

interface Snapshot {
  mask: Set<string>; // "col,row"
  refs: { origin: string | null; x_axis: string | null; xy_plane: string | null };
  baselineRow: number | null;
}

const MAX_UNDO = 50;

function deepCopy(s: Snapshot): Snapshot {
  return {
    mask: new Set(s.mask),
    refs: { ...s.refs },
    baselineRow: s.baselineRow,
  };
}

// deepCopy is used indirectly via snapshot()
void deepCopy;

export const useEditorStore = defineStore("editor", () => {
  const cols = ref(0);
  const rows = ref(0);
  const mode = ref<EditorMode>("mask");
  const currentRefRole = ref<RefRole>("origin");
  const mask = ref(new Set<string>());
  const refs = ref<Snapshot["refs"]>({ origin: null, x_axis: null, xy_plane: null });
  const baselineRow = ref<number | null>(null);
  const undoStack = ref<Snapshot[]>([]);
  const redoStack = ref<Snapshot[]>([]);

  function snapshot(): Snapshot {
    return {
      mask: new Set(mask.value),
      refs: { ...refs.value },
      baselineRow: baselineRow.value,
    };
  }

  function applySnapshot(s: Snapshot) {
    mask.value = new Set(s.mask);
    refs.value = { ...s.refs };
    baselineRow.value = s.baselineRow;
  }

  function pushUndo() {
    undoStack.value.push(snapshot());
    if (undoStack.value.length > MAX_UNDO) undoStack.value.shift();
    redoStack.value = [];
  }

  function initFromScreen(
    screen: ScreenConfig,
    cs?: CoordinateSystemConfig | null,
    baselineOverride?: number | null,
  ) {
    cols.value = screen.cabinet_count[0];
    rows.value = screen.cabinet_count[1];
    mask.value = new Set(screen.irregular_mask.map(([c, r]) => `${c},${r}`));
    refs.value = {
      origin: cs?.origin_point ?? null,
      x_axis: cs?.x_axis_point ?? null,
      xy_plane: cs?.xy_plane_point ?? null,
    };
    baselineRow.value =
      baselineOverride !== undefined
        ? baselineOverride
        : screen.bottom_completion?.lowest_measurable_row ?? null;
    undoStack.value = [];
    redoStack.value = [];
  }

  function setMode(m: EditorMode) {
    mode.value = m;
  }

  function setCurrentRefRole(r: RefRole) {
    currentRefRole.value = r;
  }

  function isAbsent(col: number, row: number): boolean {
    return mask.value.has(`${col},${row}`);
  }

  function toggleCell(col: number, row: number) {
    pushUndo();
    const k = `${col},${row}`;
    if (mask.value.has(k)) mask.value.delete(k);
    else mask.value.add(k);
    mask.value = new Set(mask.value); // trigger reactivity
  }

  function setRef(role: RefRole, name: string) {
    pushUndo();
    refs.value = { ...refs.value, [role]: name };
  }

  function setBaseline(row: number) {
    pushUndo();
    baselineRow.value = row;
  }

  function undo() {
    if (undoStack.value.length === 0) return;
    redoStack.value.push(snapshot());
    const s = undoStack.value.pop()!;
    applySnapshot(s);
  }

  function redo() {
    if (redoStack.value.length === 0) return;
    undoStack.value.push(snapshot());
    const s = redoStack.value.pop()!;
    applySnapshot(s);
  }

  function clearStacks() {
    undoStack.value = [];
    redoStack.value = [];
  }

  const undoDepth = computed(() => undoStack.value.length);
  const redoDepth = computed(() => redoStack.value.length);

  /** Commit current editor state back to a ScreenConfig shape (mask only;
   *  refs + baseline are owned by the wider project config). */
  function commitMaskToScreen(screen: ScreenConfig): ScreenConfig {
    return {
      ...screen,
      shape_mode: mask.value.size > 0 ? "irregular" : screen.shape_mode,
      irregular_mask: Array.from(mask.value).map((k) => {
        const [c, r] = k.split(",").map(Number);
        return [c, r] as [number, number];
      }),
    };
  }

  return {
    cols,
    rows,
    mode,
    currentRefRole,
    mask,
    refs,
    baselineRow,
    undoDepth,
    redoDepth,
    initFromScreen,
    setMode,
    setCurrentRefRole,
    isAbsent,
    toggleCell,
    setRef,
    setBaseline,
    undo,
    redo,
    clearStacks,
    commitMaskToScreen,
  };
});
