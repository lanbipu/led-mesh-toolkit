# IA Redesign — Method Path Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure UI/IA around an explicit M1/M2 measurement-method choice: new `/method` route, sidebar that branches survey children by method, home sidebar that lists Recent Projects, and Export merged into Preview. project.yaml gains a `project.method` field round-tripped through Rust serde.

**Architecture:**
- **Backend:** add `SurveyMethod` enum + `Option<SurveyMethod>` field on `ProjectMeta`; serde defaults so legacy YAML still loads.
- **Frontend:** new shared primitives (`LmtBanner`, `LmtConfirmDialog`), composable (`useSurveyMethod`), new view (`Method.vue`), reworked `LmtSidebar` (two states: home / project); banner-based mismatch warnings on M1-only or M2-only pages; ui store grows a per-key dismiss set.
- **Cleanup:** delete `Export.vue` and `/export` route; router gains catch-all redirect; output order becomes Preview → Instruct → Runs.

**Tech Stack:** Vue 3 + Pinia + Vue Router + vue-i18n + reka-ui (Dialog), Vitest + happy-dom + @vue/test-utils, Rust + serde_yaml.

**Source spec:** `docs/superpowers/specs/2026-05-14-ia-redesign-method-split-design.md` (commit `8359dd7`).

---

## File map

### Rust (modify)
- `src-tauri/src/dto.rs` — add `SurveyMethod` enum + `ProjectMeta.method`
- `src-tauri/src/commands/projects.rs` — add roundtrip tests

### TypeScript / Vue (create)
- `src/components/primitives/LmtBanner.vue`
- `src/components/primitives/__tests__/LmtBanner.spec.ts`
- `src/components/primitives/LmtConfirmDialog.vue`
- `src/components/primitives/__tests__/LmtConfirmDialog.spec.ts`
- `src/components/shell/LmtMethodMismatchBanner.vue`
- `src/composables/useSurveyMethod.ts`
- `src/composables/__tests__/useSurveyMethod.spec.ts`
- `src/views/Method.vue`
- `src/views/__tests__/Method.spec.ts`
- `src/components/shell/__tests__/LmtSidebar.spec.ts`

### TypeScript / Vue (modify)
- `src/services/tauri.ts` — `SurveyMethod` type + optional `ProjectMeta.method`
- `src/stores/currentProject.ts` — `setMethod()` action
- `src/stores/__tests__/currentProject.test.ts` — setMethod tests
- `src/stores/ui.ts` — `dismissedBanners` + `isBannerDismissed()` + `dismissBanner()`
- `src/router/index.ts` — add `/method`, remove `/export`, add catch-all
- `src/components/shell/LmtSidebar.vue` — rewrite (project-internal method-driven + home recent)
- `src/views/Design.vue` — embed method-pending banner
- `src/views/Import.vue` — embed mismatch banner
- `src/views/Charuco.vue` — embed mismatch banner
- `src/views/Photoplan.vue` — embed mismatch banner
- `src/views/Instruct.vue` — embed mismatch banner
- `src/views/Preview.vue` — embed mismatch banner
- `src/views/Runs.vue` — embed mismatch banner
- `src/locales/en.json` — add/remove keys
- `src/locales/zh.json` — add/remove keys

### Delete
- `src/views/Export.vue`

---

## Conventions for every task

- Steps marked **[code]** require copy-paste of the exact code block shown.
- Steps marked **[verify]** show the command + expected output.
- Test-first. Every implementation step has a failing test that precedes it.
- Commit after each task. Use the commit message template in the task.
- All commits append:
  ```
  Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
  ```
- Working directory for all commands: `/Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit`.

---

## Task 1: Rust DTO — `SurveyMethod` enum + `ProjectMeta.method`

**Files:**
- Modify: `src-tauri/src/dto.rs`
- Modify: `src-tauri/src/commands/projects.rs` (add tests at file bottom)

- [ ] **Step 1.1: Append failing dto.rs unit tests**

Open `src-tauri/src/dto.rs`. Append at the very bottom (after the last item) **[code]**:

```rust
#[cfg(test)]
mod method_tests {
    use super::*;

    fn parse(yaml: &str) -> ProjectConfig {
        serde_yaml::from_str(yaml).unwrap()
    }

    const BASE: &str = r#"
project:
  name: Test
  unit: mm
{method_line}
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V004_R001
  xy_plane_point: MAIN_V001_R002
output:
  target: disguise
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;

    fn build(method_line: &str) -> String {
        BASE.replace("{method_line}", method_line)
    }

    #[test]
    fn method_missing_yaml_parses_as_none() {
        let cfg = parse(&build(""));
        assert_eq!(cfg.project.method, None);
    }

    #[test]
    fn method_null_yaml_parses_as_none() {
        let cfg = parse(&build("  method: null"));
        assert_eq!(cfg.project.method, None);
    }

    #[test]
    fn method_m1_yaml_roundtrips() {
        let cfg = parse(&build("  method: m1"));
        assert_eq!(cfg.project.method, Some(SurveyMethod::M1));
        let s = serde_yaml::to_string(&cfg).unwrap();
        assert!(s.contains("method: m1"), "serialized form: {}", s);
    }

    #[test]
    fn method_m2_yaml_roundtrips() {
        let cfg = parse(&build("  method: m2"));
        assert_eq!(cfg.project.method, Some(SurveyMethod::M2));
        let s = serde_yaml::to_string(&cfg).unwrap();
        assert!(s.contains("method: m2"), "serialized form: {}", s);
    }

    #[test]
    fn method_invalid_value_errors() {
        let result: Result<ProjectConfig, _> = serde_yaml::from_str(&build("  method: m3"));
        assert!(result.is_err());
    }

    #[test]
    fn none_omitted_on_serialize() {
        let cfg = parse(&build(""));
        let s = serde_yaml::to_string(&cfg).unwrap();
        assert!(!s.contains("method:"), "expected method field omitted, got: {}", s);
    }
}
```

- [ ] **Step 1.2: Run tests to confirm they fail**

```bash
cd src-tauri && cargo test method_tests 2>&1 | tail -20
```

Expected: compile error mentioning `SurveyMethod` not found and `project.method` not a field on `ProjectMeta`. **This is the desired failure.**

- [ ] **Step 1.3: Implement `SurveyMethod` enum and field in dto.rs** **[code]**

In `src-tauri/src/dto.rs`, locate `pub struct ProjectMeta { ... }` and replace it with:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<SurveyMethod>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SurveyMethod {
    M1,
    M2,
}
```

- [ ] **Step 1.4: Run tests to confirm they pass**

```bash
cd src-tauri && cargo test method_tests 2>&1 | tail -10
```

Expected:
```
running 6 tests
test method_tests::method_invalid_value_errors ... ok
test method_tests::method_m1_yaml_roundtrips ... ok
test method_tests::method_m2_yaml_roundtrips ... ok
test method_tests::method_missing_yaml_parses_as_none ... ok
test method_tests::method_null_yaml_parses_as_none ... ok
test method_tests::none_omitted_on_serialize ... ok
test result: ok. 6 passed
```

- [ ] **Step 1.5: Append load/save integration tests in projects.rs** **[code]**

Open `src-tauri/src/commands/projects.rs`. At the very bottom append:

```rust
#[cfg(test)]
mod project_yaml_method_tests {
    use super::*;
    use crate::dto::{ProjectConfig, ProjectMeta, SurveyMethod};
    use tempfile::tempdir;

    fn minimal_config(method: Option<SurveyMethod>) -> ProjectConfig {
        use crate::dto::{
            CoordinateSystemConfig, OutputConfig, ScreenConfig, ShapeMode, ShapePriorConfig,
        };
        use std::collections::BTreeMap;

        let mut screens = BTreeMap::new();
        screens.insert(
            "MAIN".to_string(),
            ScreenConfig {
                cabinet_count: [4, 2],
                cabinet_size_mm: [500.0, 500.0],
                pixels_per_cabinet: None,
                shape_prior: ShapePriorConfig::Flat,
                shape_mode: ShapeMode::Rectangle,
                irregular_mask: vec![],
                bottom_completion: None,
            },
        );
        ProjectConfig {
            project: ProjectMeta {
                name: "X".into(),
                unit: "mm".into(),
                method,
            },
            screens,
            coordinate_system: CoordinateSystemConfig {
                origin_point: "MAIN_V001_R001".into(),
                x_axis_point: "MAIN_V004_R001".into(),
                xy_plane_point: "MAIN_V001_R002".into(),
            },
            output: OutputConfig {
                target: "disguise".into(),
                obj_filename: "{screen_id}.obj".into(),
                weld_vertices_tolerance_mm: 1.0,
                triangulate: true,
            },
        }
    }

    #[test]
    fn load_save_roundtrip_with_method_m1() {
        let dir = tempdir().unwrap();
        let cfg = minimal_config(Some(SurveyMethod::M1));
        save_project_yaml_to_path(dir.path(), &cfg).unwrap();
        let loaded = load_project_yaml_from_path(dir.path()).unwrap();
        assert_eq!(loaded.project.method, Some(SurveyMethod::M1));
    }

    #[test]
    fn load_save_roundtrip_with_method_m2() {
        let dir = tempdir().unwrap();
        let cfg = minimal_config(Some(SurveyMethod::M2));
        save_project_yaml_to_path(dir.path(), &cfg).unwrap();
        let loaded = load_project_yaml_from_path(dir.path()).unwrap();
        assert_eq!(loaded.project.method, Some(SurveyMethod::M2));
    }

    #[test]
    fn load_legacy_yaml_without_method() {
        let dir = tempdir().unwrap();
        let legacy = r#"
project:
  name: Legacy
  unit: mm
screens:
  MAIN:
    cabinet_count: [4, 2]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V004_R001
  xy_plane_point: MAIN_V001_R002
output:
  target: disguise
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;
        std::fs::write(dir.path().join("project.yaml"), legacy).unwrap();
        let loaded = load_project_yaml_from_path(dir.path()).unwrap();
        assert_eq!(loaded.project.method, None);
    }
}
```

- [ ] **Step 1.6: Confirm `tempfile` dev-dependency exists**

```bash
grep -n "tempfile" src-tauri/Cargo.toml
```

Expected: at least one line in `[dev-dependencies]`. If **not found**, add it:

```bash
cd src-tauri && cargo add --dev tempfile
```

- [ ] **Step 1.7: Run integration tests**

```bash
cd src-tauri && cargo test project_yaml_method_tests 2>&1 | tail -10
```

Expected: all 3 tests pass.

- [ ] **Step 1.8: Run full Rust test suite to confirm no regressions**

```bash
cd src-tauri && cargo test 2>&1 | tail -5
```

Expected: `test result: ok` and `0 failed`.

- [ ] **Step 1.9: Commit**

```bash
cd /Users/bip.lan/AIWorkspace/vp/led-mesh-toolkit
git add src-tauri/src/dto.rs src-tauri/src/commands/projects.rs src-tauri/Cargo.toml
git commit -m "$(cat <<'EOF'
feat(dto): add SurveyMethod enum + ProjectMeta.method (Option, serde default)

Round-trips m1/m2/null/missing through serde_yaml. Legacy project.yaml
without a method field loads as None.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: TS schema sync + `currentProject.setMethod()`

**Files:**
- Modify: `src/services/tauri.ts`
- Modify: `src/stores/currentProject.ts`
- Modify: `src/stores/__tests__/currentProject.test.ts`

- [ ] **Step 2.1: Add failing test for setMethod**

Open `src/stores/__tests__/currentProject.test.ts`. Inside the `describe("useCurrentProjectStore", ...)` block, **before** the closing `});`, append:

```ts
  it("setMethod writes project.method and saves", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 9, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce(sampleConfig);
    (tauriApi.saveProjectYaml as any).mockResolvedValueOnce(undefined);
    const s = useCurrentProjectStore();
    await s.load(9);
    expect(s.config?.project.method).toBeUndefined();
    await s.setMethod("m1");
    expect(s.config?.project.method).toBe("m1");
    expect(tauriApi.saveProjectYaml).toHaveBeenCalled();
    expect(s.dirty).toBe(false);
  });

  it("setMethod is a no-op when no project loaded", async () => {
    const s = useCurrentProjectStore();
    await s.setMethod("m2");
    expect(tauriApi.saveProjectYaml).not.toHaveBeenCalled();
  });
```

- [ ] **Step 2.2: Run test to confirm it fails**

```bash
pnpm test --run src/stores/__tests__/currentProject.test.ts 2>&1 | tail -15
```

Expected: TypeScript error `Property 'setMethod' does not exist on type ...` or runtime error.

- [ ] **Step 2.3: Update `src/services/tauri.ts`** **[code]**

Find the `ProjectMeta` interface and replace with:

```ts
export type SurveyMethod = "m1" | "m2";

export interface ProjectMeta {
  name: string;
  unit: string;
  method?: SurveyMethod;
}
```

- [ ] **Step 2.4: Add `setMethod()` action to `src/stores/currentProject.ts`** **[code]**

Open `src/stores/currentProject.ts`. Add this import to the top:

```ts
import { tauriApi, type ProjectConfig, type ScreenConfig, type SurveyMethod } from "@/services/tauri";
```

(Replace the existing import — same path, just add `SurveyMethod`.)

Inside the store factory, after `updateScreen`, add:

```ts
  async function setMethod(method: SurveyMethod) {
    if (!config.value || !absPath.value) return;
    config.value = {
      ...config.value,
      project: { ...config.value.project, method },
    };
    dirty.value = true;
    await save();
  }
```

Then add `setMethod` to the `return { ... }` at the bottom of the store.

- [ ] **Step 2.5: Run test to confirm pass**

```bash
pnpm test --run src/stores/__tests__/currentProject.test.ts 2>&1 | tail -10
```

Expected: all tests pass (original 4 + new 2 = 6).

- [ ] **Step 2.6: Run typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: no errors.

- [ ] **Step 2.7: Commit**

```bash
git add src/services/tauri.ts src/stores/currentProject.ts src/stores/__tests__/currentProject.test.ts
git commit -m "$(cat <<'EOF'
feat(store): currentProject.setMethod + SurveyMethod TS type

Mirrors the Rust DTO. Writes through to project.yaml via save().
Optional `method` field on ProjectMeta — undefined means not yet chosen.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: ui store — `dismissedBanners` set

**Files:**
- Modify: `src/stores/ui.ts`
- Create: `src/stores/__tests__/ui.test.ts`

- [ ] **Step 3.1: Create failing ui store test** **[code]**

Create `src/stores/__tests__/ui.test.ts`:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useUiStore } from "../ui";

describe("useUiStore — dismissedBanners", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("isBannerDismissed returns false by default", () => {
    const ui = useUiStore();
    expect(ui.isBannerDismissed("any-key")).toBe(false);
  });

  it("dismissBanner records the key", () => {
    const ui = useUiStore();
    ui.dismissBanner("design-method-banner-7");
    expect(ui.isBannerDismissed("design-method-banner-7")).toBe(true);
    expect(ui.isBannerDismissed("other-key")).toBe(false);
  });

  it("dismissals are session-scoped (not localStorage)", () => {
    const ui = useUiStore();
    ui.dismissBanner("k");
    // No localStorage write should happen for banner dismissals
    expect(localStorage.getItem("lmt.dismissedBanners")).toBeNull();
  });
});
```

- [ ] **Step 3.2: Run test to confirm fail**

```bash
pnpm test --run src/stores/__tests__/ui.test.ts 2>&1 | tail -10
```

Expected: `isBannerDismissed is not a function` or similar.

- [ ] **Step 3.3: Extend `src/stores/ui.ts`** **[code]**

Inside the `useUiStore` store factory, add this state and functions (after `toasts`/`toast`):

```ts
  const dismissedBanners = ref<Set<string>>(new Set());

  function isBannerDismissed(key: string): boolean {
    return dismissedBanners.value.has(key);
  }

  function dismissBanner(key: string) {
    if (dismissedBanners.value.has(key)) return;
    const next = new Set(dismissedBanners.value);
    next.add(key);
    dismissedBanners.value = next;
  }
```

Then add `dismissedBanners`, `isBannerDismissed`, `dismissBanner` to the `return { ... }`.

- [ ] **Step 3.4: Run test to confirm pass**

```bash
pnpm test --run src/stores/__tests__/ui.test.ts 2>&1 | tail -5
```

Expected: 3 pass.

- [ ] **Step 3.5: Commit**

```bash
git add src/stores/ui.ts src/stores/__tests__/ui.test.ts
git commit -m "$(cat <<'EOF'
feat(ui-store): per-key dismissedBanners (session-scoped)

Banner components call dismissBanner(key); isBannerDismissed(key) decides
whether to render. State lives only in memory — reload re-shows banners.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: `LmtBanner` primitive

**Files:**
- Create: `src/components/primitives/LmtBanner.vue`
- Create: `src/components/primitives/__tests__/LmtBanner.spec.ts`

- [ ] **Step 4.1: Create failing component test** **[code]**

Create `src/components/primitives/__tests__/LmtBanner.spec.ts`:

```ts
import { describe, it, expect, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import LmtBanner from "../LmtBanner.vue";
import { useUiStore } from "@/stores/ui";

describe("LmtBanner", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("renders title and action label", () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hello", actionLabel: "Go", dismissKey: "k1" },
    });
    expect(w.text()).toContain("Hello");
    expect(w.text()).toContain("Go");
  });

  it("emits action when action button clicked", async () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", actionLabel: "Go", dismissKey: "k2" },
    });
    await w.find("button[data-banner-action]").trigger("click");
    expect(w.emitted("action")).toBeTruthy();
  });

  it("dismiss button calls ui.dismissBanner with the key", async () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", dismissKey: "design-banner-3" },
    });
    const ui = useUiStore();
    await w.find("button[data-banner-dismiss]").trigger("click");
    expect(ui.isBannerDismissed("design-banner-3")).toBe(true);
  });

  it("renders nothing when already dismissed", () => {
    const ui = useUiStore();
    ui.dismissBanner("dismissed-key");
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", dismissKey: "dismissed-key" },
    });
    expect(w.find("[data-banner]").exists()).toBe(false);
  });

  it("info tone applies status-info classes", () => {
    const w = mount(LmtBanner, {
      props: { tone: "info", title: "Hi", dismissKey: "kt" },
    });
    const root = w.find("[data-banner]");
    expect(root.classes().join(" ")).toContain("status-info");
  });
});
```

- [ ] **Step 4.2: Run test to confirm fail**

```bash
pnpm test --run src/components/primitives/__tests__/LmtBanner.spec.ts 2>&1 | tail -10
```

Expected: "Failed to resolve component LmtBanner" or import error.

- [ ] **Step 4.3: Create `src/components/primitives/LmtBanner.vue`** **[code]**

```vue
<script setup lang="ts">
import { computed } from "vue";
import { useUiStore } from "@/stores/ui";
import LmtIcon from "./LmtIcon.vue";

type Tone = "info" | "warn";

const props = withDefaults(
  defineProps<{
    tone?: Tone;
    icon?: string;
    title: string;
    actionLabel?: string;
    dismissKey: string;
  }>(),
  { tone: "info" },
);

const emit = defineEmits<{
  (e: "action"): void;
}>();

const ui = useUiStore();

const dismissed = computed(() => ui.isBannerDismissed(props.dismissKey));

const toneClass = computed(() => {
  return props.tone === "warn"
    ? "border-amber-500/30 bg-amber-500/10 text-amber-500"
    : "border-status-info/30 bg-status-info/10 text-status-info";
});

const iconName = computed(() => props.icon ?? (props.tone === "warn" ? "alert-triangle" : "info"));

function dismiss() {
  ui.dismissBanner(props.dismissKey);
}
</script>

<template>
  <div
    v-if="!dismissed"
    data-banner
    class="flex items-center gap-3 rounded-md border px-4 py-2"
    :class="toneClass"
  >
    <LmtIcon :name="iconName" :size="15" />
    <span class="flex-1 truncate text-sm">{{ title }}</span>
    <button
      v-if="actionLabel"
      type="button"
      data-banner-action
      class="rounded-md border border-current/30 px-2.5 py-1 font-display text-xs font-bold uppercase tracking-wide hover:bg-current/10"
      @click="emit('action')"
    >
      {{ actionLabel }}
    </button>
    <button
      type="button"
      data-banner-dismiss
      :aria-label="'Dismiss'"
      class="inline-flex size-6 items-center justify-center rounded-md hover:bg-current/10"
      @click="dismiss"
    >
      <LmtIcon name="x" :size="13" />
    </button>
  </div>
</template>
```

- [ ] **Step 4.4: Run test to confirm pass**

```bash
pnpm test --run src/components/primitives/__tests__/LmtBanner.spec.ts 2>&1 | tail -10
```

Expected: 5 pass.

- [ ] **Step 4.5: Typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: 0 errors.

- [ ] **Step 4.6: Commit**

```bash
git add src/components/primitives/LmtBanner.vue src/components/primitives/__tests__/LmtBanner.spec.ts
git commit -m "$(cat <<'EOF'
feat(primitive): LmtBanner — dismissable info/warn banner with action slot

Tones map to status-info / amber tokens. Dismissal persists in ui store
keyed by dismissKey prop (per-instance).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: `LmtConfirmDialog` primitive

**Files:**
- Create: `src/components/primitives/LmtConfirmDialog.vue`
- Create: `src/components/primitives/__tests__/LmtConfirmDialog.spec.ts`

- [ ] **Step 5.1: Create failing test** **[code]**

Create `src/components/primitives/__tests__/LmtConfirmDialog.spec.ts`:

```ts
import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import LmtConfirmDialog from "../LmtConfirmDialog.vue";

describe("LmtConfirmDialog", () => {
  it("renders title and body when open=true", async () => {
    const w = mount(LmtConfirmDialog, {
      props: {
        open: true,
        title: "Switch?",
        body: "Are you sure?",
        okLabel: "Yes",
        cancelLabel: "No",
      },
      attachTo: document.body,
    });
    // reka-ui portals content to body; query the document
    expect(document.body.textContent).toContain("Switch?");
    expect(document.body.textContent).toContain("Are you sure?");
    w.unmount();
  });

  it("emits confirm when ok clicked", async () => {
    const w = mount(LmtConfirmDialog, {
      props: {
        open: true,
        title: "T",
        body: "B",
        okLabel: "Yes",
        cancelLabel: "No",
      },
      attachTo: document.body,
    });
    const ok = document.querySelector("button[data-confirm-ok]") as HTMLButtonElement | null;
    expect(ok).not.toBeNull();
    ok!.click();
    expect(w.emitted("confirm")).toBeTruthy();
    w.unmount();
  });

  it("emits update:open(false) when cancel clicked", async () => {
    const w = mount(LmtConfirmDialog, {
      props: {
        open: true,
        title: "T",
        body: "B",
        okLabel: "Yes",
        cancelLabel: "No",
      },
      attachTo: document.body,
    });
    const cancel = document.querySelector("button[data-confirm-cancel]") as HTMLButtonElement | null;
    cancel!.click();
    const events = w.emitted("update:open") ?? [];
    expect(events.at(-1)).toEqual([false]);
    w.unmount();
  });
});
```

- [ ] **Step 5.2: Run test to confirm fail**

```bash
pnpm test --run src/components/primitives/__tests__/LmtConfirmDialog.spec.ts 2>&1 | tail -10
```

Expected: module not found.

- [ ] **Step 5.3: Create `src/components/primitives/LmtConfirmDialog.vue`** **[code]**

```vue
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

const props = defineProps<{
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
```

- [ ] **Step 5.4: Verify `Button.vue` has a `destructive` variant**

```bash
grep -n "destructive" src/components/ui/Button.vue
```

Expected: at least one match. If **none**, the `:variant` binding above will silently fall back to `default`; remove the `okTone` prop usage from any call site if you spot issues later. (No code change needed here — note the limitation.)

- [ ] **Step 5.5: Run test to confirm pass**

```bash
pnpm test --run src/components/primitives/__tests__/LmtConfirmDialog.spec.ts 2>&1 | tail -10
```

Expected: 3 pass.

- [ ] **Step 5.6: Commit**

```bash
git add src/components/primitives/LmtConfirmDialog.vue src/components/primitives/__tests__/LmtConfirmDialog.spec.ts
git commit -m "$(cat <<'EOF'
feat(primitive): LmtConfirmDialog — reka-ui Dialog with ok/cancel slots

Two-way binding via update:open. Emits 'confirm' on OK before closing.
Used by Method page for path-switch confirmation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: `useSurveyMethod` composable

**Files:**
- Create: `src/composables/useSurveyMethod.ts`
- Create: `src/composables/__tests__/useSurveyMethod.spec.ts`

- [ ] **Step 6.1: Create failing test** **[code]**

Create `src/composables/__tests__/useSurveyMethod.spec.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { ref } from "vue";

const routeMock = { params: ref<{ id: string }>({ id: "5" }) };
vi.mock("vue-router", () => ({
  useRoute: () => ({
    get params() {
      return routeMock.params.value;
    },
  }),
}));

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    loadProjectYaml: vi.fn(),
    saveProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import { useSurveyMethod } from "../useSurveyMethod";
import { useCurrentProjectStore } from "@/stores/currentProject";

describe("useSurveyMethod", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
    routeMock.params.value = { id: "5" };
  });

  it("returns null when no project loaded", () => {
    const { method } = useSurveyMethod();
    expect(method.value).toBeNull();
  });

  it("returns 'm1' when project.method == 'm1' and route matches", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project: { name: "X", unit: "mm", method: "m1" },
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    const proj = useCurrentProjectStore();
    await proj.load(5);
    const { method } = useSurveyMethod();
    expect(method.value).toBe("m1");
  });

  it("returns null when proj.id mismatches route (race during switch)", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 7, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project: { name: "X", unit: "mm", method: "m2" },
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    const proj = useCurrentProjectStore();
    await proj.load(7);
    routeMock.params.value = { id: "99" }; // route param now points to a different project
    const { method } = useSurveyMethod();
    expect(method.value).toBeNull();
  });

  it("returns null when method field is missing", async () => {
    (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
      { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
    ]);
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project: { name: "X", unit: "mm" }, // no method key
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    const proj = useCurrentProjectStore();
    await proj.load(5);
    const { method } = useSurveyMethod();
    expect(method.value).toBeNull();
  });
});
```

- [ ] **Step 6.2: Run test to confirm fail**

```bash
pnpm test --run src/composables/__tests__/useSurveyMethod.spec.ts 2>&1 | tail -10
```

Expected: module not found.

- [ ] **Step 6.3: Create `src/composables/useSurveyMethod.ts`** **[code]**

```ts
import { computed } from "vue";
import { useRoute } from "vue-router";
import { useCurrentProjectStore } from "@/stores/currentProject";
import type { SurveyMethod } from "@/services/tauri";

export function useSurveyMethod() {
  const proj = useCurrentProjectStore();
  const route = useRoute();

  const method = computed<SurveyMethod | null>(() => {
    const routeId = Number(route.params.id);
    if (!Number.isFinite(routeId) || proj.id !== routeId) return null;
    return proj.config?.project.method ?? null;
  });

  return { method, setMethod: proj.setMethod };
}
```

- [ ] **Step 6.4: Run test to confirm pass**

```bash
pnpm test --run src/composables/__tests__/useSurveyMethod.spec.ts 2>&1 | tail -10
```

Expected: 4 pass.

- [ ] **Step 6.5: Commit**

```bash
git add src/composables/useSurveyMethod.ts src/composables/__tests__/useSurveyMethod.spec.ts
git commit -m "$(cat <<'EOF'
feat(composable): useSurveyMethod — race-safe accessor for project.method

Returns null whenever route id and store id disagree (project-switch race).
Used by sidebar, Method view, and mismatch banners.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: i18n keys — bulk add/remove

**Files:**
- Modify: `src/locales/en.json`
- Modify: `src/locales/zh.json`

(Tests come later — i18n changes are validated by view tests that depend on them.)

- [ ] **Step 7.1: Update `src/locales/en.json`** **[code]**

Apply these edits one at a time:

**1) `nav.group` block — add `survey`:**

Find:
```json
    "group": {
      "workspace": "Workspace",
      "design": "Design",
      "output": "Output"
    },
```
Replace with:
```json
    "group": {
      "workspace": "Workspace",
      "design": "Design",
      "survey": "Survey",
      "output": "Output"
    },
```

**2) `nav` block — replace existing children:**

Find:
```json
    "home": "Home",
    "design": "Screen Design",
    "preview": "Preview",
    "export": "Export",
    "runs": "Runs",
    "import": "Import",
    "instruct": "Instruct",
    "charuco": "ChArUco",
    "photoplan": "Photo Plan"
```
Replace with:
```json
    "home": "Home",
    "design": "Screen Design",
    "method": "Method",
    "preview": "Preview",
    "runs": "Runs",
    "import": "Import",
    "instruct": "Instruct",
    "charuco": "ChArUco",
    "photoplan": "Photo Plan"
```
(Removed `export`.)

**3) `home` block — add new keys for sidebar recent list:**

Find any existing `"home": { ... }` block. Inside it (at the end, before the closing `}`), add:
```json
    "recentProjects": "Recent Projects",
    "pinned": "PINNED",
```
(Keep existing home keys.)

**4) Add a top-level `method` block** — place it right after `instruct` block:
```json
  "method": {
    "eyebrow": "MEASUREMENT METHOD",
    "title": "Method",
    "description": "Pick how you'll measure cabinet vertices for this project.",
    "m1": {
      "title": "M1 · Total Station",
      "desc": "Use a total station to capture vertex coordinates as CSV.",
      "bullets": [
        "CSV import",
        "Millimeter-level precision",
        "Requires pro hardware"
      ]
    },
    "m2": {
      "title": "M2 · Visual Back-Calc",
      "desc": "Recover surface from ArUco/Charuco photos taken with any camera.",
      "bullets": [
        "ArUco / Charuco markers",
        "Any phone or camera",
        "Low equipment cost"
      ]
    },
    "current": "CURRENT",
    "available": "AVAILABLE",
    "useM1": "Use M1",
    "useM2": "Use M2",
    "continueM1": "Continue with M1",
    "continueM2": "Continue with M2",
    "switchToM1": "Switch to M1",
    "switchToM2": "Switch to M2",
    "coexistNote": "Switching preserves existing data — measurements/ and aruco/ directories coexist, swap any time.",
    "confirmSwitch": {
      "title": "Switch method",
      "body": "Switching to {target}. Existing data is preserved and you can switch back any time.",
      "ok": "Confirm switch",
      "cancel": "Cancel"
    },
    "mismatch": {
      "m1Only": "This page is M1-only. Current method: {current}.",
      "m2Only": "This page is M2-only. Current method: {current}.",
      "unset": "Measurement method not selected yet.",
      "goPick": "Go to Method →"
    }
  },
```

**5) Add a `design.banner` block** — inside the existing `"design": { ... }` block (at the end):
```json
    "banner": {
      "methodPending": "Design ready · pick your measurement method",
      "go": "Choose →"
    }
```

**6) Delete the entire `"export": { ... }` block** — including all its sub-keys.

- [ ] **Step 7.2: Update `src/locales/zh.json`** **[code]**

Mirror all edits in zh.json with Chinese strings:

**1) `nav.group`:**
```json
    "group": {
      "workspace": "工作区",
      "design": "设计阶段",
      "survey": "测量",
      "output": "输出"
    },
```

**2) `nav` children:**
```json
    "home": "首页",
    "design": "屏体设计",
    "method": "测量方式",
    "preview": "预览",
    "runs": "运行历史",
    "import": "导入测量",
    "instruct": "施工指令",
    "charuco": "ChArUco 码",
    "photoplan": "拍摄规划"
```

**3) `home` adds:**
```json
    "recentProjects": "最近项目",
    "pinned": "已置顶",
```

**4) `method` block:**
```json
  "method": {
    "eyebrow": "MEASUREMENT METHOD",
    "title": "测量方式选择",
    "description": "选择本项目所用的测量方式。",
    "m1": {
      "title": "M1 · 全站仪",
      "desc": "用全站仪测量顶点坐标，导入 CSV。",
      "bullets": [
        "导入 CSV",
        "毫米级精度",
        "需要专业设备"
      ]
    },
    "m2": {
      "title": "M2 · 视觉反算",
      "desc": "用 ArUco/Charuco 标记拍照，反算出顶点空间位置。",
      "bullets": [
        "ArUco / Charuco 标记",
        "手机或相机即可",
        "设备门槛低"
      ]
    },
    "current": "CURRENT",
    "available": "AVAILABLE",
    "useM1": "使用 M1",
    "useM2": "使用 M2",
    "continueM1": "继续使用 M1",
    "continueM2": "继续使用 M2",
    "switchToM1": "切换到 M1",
    "switchToM2": "切换到 M2",
    "coexistNote": "切换 method 不会删除已有产物；measurements/ 与 aruco/ 目录共存，可随时切回。",
    "confirmSwitch": {
      "title": "切换测量方式",
      "body": "切换到 {target}。已有数据将保留，可随时切回。",
      "ok": "确认切换",
      "cancel": "取消"
    },
    "mismatch": {
      "m1Only": "本页仅用于 M1，当前 method：{current}。",
      "m2Only": "本页仅用于 M2，当前 method：{current}。",
      "unset": "尚未选择测量方式。",
      "goPick": "去选择 →"
    }
  },
```

**5) `design.banner`:**
```json
    "banner": {
      "methodPending": "平体设计就绪，下一步去选择测量方式",
      "go": "去选择 →"
    }
```

**6) Delete `"export": { ... }` block.**

- [ ] **Step 7.3: Verify JSON validity**

```bash
node -e "JSON.parse(require('fs').readFileSync('src/locales/en.json','utf-8')); JSON.parse(require('fs').readFileSync('src/locales/zh.json','utf-8')); console.log('OK')"
```

Expected: `OK`.

- [ ] **Step 7.4: Typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: 0 errors (i18n is JSON-only; types unaffected).

- [ ] **Step 7.5: Commit**

```bash
git add src/locales/en.json src/locales/zh.json
git commit -m "$(cat <<'EOF'
i18n: add method/mismatch/banner keys, drop export.* keys, add survey group

en + zh both updated. nav.method, design.banner.methodPending, full method.*
namespace with m1/m2 bullets as i18n arrays.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Method view + `/method` route

**Files:**
- Create: `src/views/Method.vue`
- Create: `src/views/__tests__/Method.spec.ts`
- Modify: `src/router/index.ts` (add route only — `/export` removal is Task 11)

- [ ] **Step 8.1: Add the route entry**

In `src/router/index.ts` add `import Method from "@/views/Method.vue";` near the other view imports. Then inside the `routes` array, immediately after the `design` route, insert:

```ts
  {
    path: "/projects/:id/method",
    name: "method",
    component: Method,
    props: true,
  },
```

- [ ] **Step 8.2: Write Method.spec.ts** **[code]**

Create `src/views/__tests__/Method.spec.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import { createMemoryHistory, createRouter } from "vue-router";
import { createI18n } from "vue-i18n";
import en from "@/locales/en.json";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    loadProjectYaml: vi.fn(),
    saveProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import Method from "../Method.vue";
import { useCurrentProjectStore } from "@/stores/currentProject";

async function mountWith(method: "m1" | "m2" | null) {
  setActivePinia(createPinia());
  (tauriApi.listRecentProjects as any).mockResolvedValueOnce([
    { id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" },
  ]);
  const project: any = { name: "X", unit: "mm" };
  if (method) project.method = method;
  (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
    project,
    screens: {},
    coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
    output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
  });
  (tauriApi.saveProjectYaml as any).mockResolvedValue(undefined);
  const proj = useCurrentProjectStore();
  await proj.load(5);

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [{ path: "/projects/:id/method", name: "method", component: Method }],
  });
  await router.push("/projects/5/method");
  await router.isReady();

  const i18n = createI18n({ legacy: false, locale: "en", messages: { en } });

  return mount(Method, {
    global: { plugins: [router, i18n] },
    attachTo: document.body,
  });
}

describe("Method.vue", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders both method cards with bullets", async () => {
    const w = await mountWith(null);
    expect(w.text()).toContain("M1 · Total Station");
    expect(w.text()).toContain("M2 · Visual Back-Calc");
    expect(w.text()).toContain("CSV import");
    expect(w.text()).toContain("ArUco / Charuco markers");
    w.unmount();
  });

  it("shows AVAILABLE on both cards when method is unset", async () => {
    const w = await mountWith(null);
    const text = w.text();
    const availableCount = (text.match(/AVAILABLE/g) || []).length;
    expect(availableCount).toBeGreaterThanOrEqual(2);
    expect(text).not.toContain("CURRENT");
    w.unmount();
  });

  it("shows CURRENT on M1 card when method=m1", async () => {
    const w = await mountWith("m1");
    expect(w.text()).toContain("CURRENT");
    w.unmount();
  });

  it("clicking 'Use M1' on unset project calls setMethod", async () => {
    const w = await mountWith(null);
    const btns = w.findAll("button");
    const useM1 = btns.find((b) => b.text().includes("Use M1"));
    expect(useM1).toBeDefined();
    await useM1!.trigger("click");
    // No confirm dialog when unset → goes through directly
    expect(tauriApi.saveProjectYaml).toHaveBeenCalled();
    w.unmount();
  });

  it("clicking 'Switch to M2' opens confirm dialog (no save yet)", async () => {
    const w = await mountWith("m1");
    const btns = w.findAll("button");
    const switchM2 = btns.find((b) => b.text().includes("Switch to M2"));
    await switchM2!.trigger("click");
    expect(document.body.textContent).toContain("Switch method");
    expect(tauriApi.saveProjectYaml).not.toHaveBeenCalled();
    w.unmount();
  });
});
```

- [ ] **Step 8.3: Run test to confirm fail**

```bash
pnpm test --run src/views/__tests__/Method.spec.ts 2>&1 | tail -10
```

Expected: module not found / Method.vue missing.

- [ ] **Step 8.4: Create `src/views/Method.vue`** **[code]**

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useUiStore } from "@/stores/ui";
import { useSurveyMethod } from "@/composables/useSurveyMethod";
import type { SurveyMethod } from "@/services/tauri";
import LmtPageHeader from "@/components/primitives/LmtPageHeader.vue";
import LmtIcon from "@/components/primitives/LmtIcon.vue";
import LmtConfirmDialog from "@/components/primitives/LmtConfirmDialog.vue";
import Button from "@/components/ui/Button.vue";

const { t, tm, rt } = useI18n();
const route = useRoute();
const proj = useCurrentProjectStore();
const ui = useUiStore();
const { method } = useSurveyMethod();

const id = computed(() => Number(route.params.id));

onMounted(async () => {
  try {
    if (proj.id !== id.value) await proj.load(id.value);
  } catch (e) {
    ui.toast("error", `${e}`);
  }
});

const dialogOpen = ref(false);
const pendingTarget = ref<SurveyMethod | null>(null);

function bullets(key: "m1" | "m2"): string[] {
  return (tm(`method.${key}.bullets`) as unknown as string[]).map((b) => rt(b));
}

function isCurrent(m: SurveyMethod): boolean {
  return method.value === m;
}

function buttonLabel(m: SurveyMethod): string {
  if (method.value === null) return m === "m1" ? t("method.useM1") : t("method.useM2");
  if (method.value === m) return m === "m1" ? t("method.continueM1") : t("method.continueM2");
  return m === "m1" ? t("method.switchToM1") : t("method.switchToM2");
}

async function onCardAction(m: SurveyMethod) {
  // No-op when continuing current
  if (method.value === m) return;
  // First-time pick: go through directly
  if (method.value === null) {
    await proj.setMethod(m);
    ui.toast("success", "Method set");
    return;
  }
  // Switch path: open confirm
  pendingTarget.value = m;
  dialogOpen.value = true;
}

async function doSwitch() {
  if (!pendingTarget.value) return;
  await proj.setMethod(pendingTarget.value);
  ui.toast("success", "Method switched");
  pendingTarget.value = null;
}

const confirmBody = computed(() => {
  if (!pendingTarget.value) return "";
  const target = pendingTarget.value === "m1" ? t("method.m1.title") : t("method.m2.title");
  return t("method.confirmSwitch.body", { target });
});
</script>

<template>
  <div class="flex h-full flex-col gap-6 p-6">
    <LmtPageHeader
      :eyebrow="t('method.eyebrow')"
      :title="t('method.title')"
      :description="t('method.description')"
    />

    <section class="grid gap-4 md:grid-cols-2">
      <article
        v-for="m in (['m1', 'm2'] as const)"
        :key="m"
        class="flex flex-col gap-4 rounded-lg border p-5 transition-colors"
        :class="
          isCurrent(m)
            ? 'border-primary bg-primary/5'
            : 'border-border bg-card hover:border-primary/40'
        "
      >
        <div class="flex items-center justify-between">
          <LmtIcon :name="m === 'm1' ? 'radio-tower' : 'scan-eye'" :size="24" />
          <span
            class="rounded-full border px-2 py-0.5 font-display text-[11px] font-bold uppercase tracking-wide"
            :class="
              isCurrent(m)
                ? 'border-primary/30 bg-primary/10 text-primary'
                : 'border-border bg-muted/30 text-muted-foreground'
            "
          >
            {{ isCurrent(m) ? t("method.current") : t("method.available") }}
          </span>
        </div>

        <div>
          <p class="font-display text-2xl font-extrabold text-foreground">
            {{ t(`method.${m}.title`) }}
          </p>
          <p class="mt-1 text-sm text-muted-foreground">
            {{ t(`method.${m}.desc`) }}
          </p>
        </div>

        <ul class="space-y-1 text-xs text-muted-foreground">
          <li v-for="(b, i) in bullets(m)" :key="i" class="flex items-start gap-2">
            <LmtIcon name="check" :size="12" class="mt-0.5 text-status-healthy" />
            <span>{{ b }}</span>
          </li>
        </ul>

        <div class="mt-auto">
          <Button
            :variant="isCurrent(m) ? 'outline' : 'default'"
            size="sm"
            class="w-full"
            :disabled="method === m"
            @click="onCardAction(m)"
          >
            {{ buttonLabel(m) }}
          </Button>
        </div>
      </article>
    </section>

    <p class="text-xs text-muted-foreground">
      <LmtIcon name="info" :size="12" class="mr-1 inline align-text-bottom" />
      {{ t("method.coexistNote") }}
    </p>

    <LmtConfirmDialog
      v-model:open="dialogOpen"
      :title="t('method.confirmSwitch.title')"
      :body="confirmBody"
      :ok-label="t('method.confirmSwitch.ok')"
      :cancel-label="t('method.confirmSwitch.cancel')"
      @confirm="doSwitch"
    />
  </div>
</template>
```

- [ ] **Step 8.5: Run test to confirm pass**

```bash
pnpm test --run src/views/__tests__/Method.spec.ts 2>&1 | tail -15
```

Expected: 5 pass.

- [ ] **Step 8.6: Typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: 0 errors.

- [ ] **Step 8.7: Commit**

```bash
git add src/router/index.ts src/views/Method.vue src/views/__tests__/Method.spec.ts
git commit -m "$(cat <<'EOF'
feat(view): Method — pick / switch M1 vs M2 with confirm dialog

New route /projects/:id/method. Two cards display current/available state
and trigger setMethod directly when method was unset, or open the confirm
dialog when switching between m1/m2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: LmtSidebar rewrite (project-internal + home)

**Files:**
- Modify: `src/components/shell/LmtSidebar.vue`
- Create: `src/components/shell/__tests__/LmtSidebar.spec.ts`

- [ ] **Step 9.1: Write LmtSidebar.spec.ts** **[code]**

Create `src/components/shell/__tests__/LmtSidebar.spec.ts`:

```ts
import { describe, it, expect, beforeEach, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import { createMemoryHistory, createRouter } from "vue-router";
import { createI18n } from "vue-i18n";
import en from "@/locales/en.json";

vi.mock("@/services/tauri", () => ({
  tauriApi: {
    listRecentProjects: vi.fn(),
    loadProjectYaml: vi.fn(),
    saveProjectYaml: vi.fn(),
  },
}));

import { tauriApi } from "@/services/tauri";
import LmtSidebar from "../LmtSidebar.vue";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useProjectsStore } from "@/stores/projects";

async function mountSidebar(routePath: string, options?: {
  recent?: Array<{ id: number; abs_path: string; display_name: string; last_opened_at: string }>;
  loadedProject?: { id: number; method?: "m1" | "m2" };
}) {
  setActivePinia(createPinia());
  const recent = options?.recent ?? [];
  (tauriApi.listRecentProjects as any).mockResolvedValue(recent);

  if (options?.loadedProject) {
    const proj = useCurrentProjectStore();
    const project: any = { name: "X", unit: "mm" };
    if (options.loadedProject.method) project.method = options.loadedProject.method;
    (tauriApi.loadProjectYaml as any).mockResolvedValueOnce({
      project,
      screens: {},
      coordinate_system: { origin_point: "", x_axis_point: "", xy_plane_point: "" },
      output: { target: "disguise", obj_filename: "", weld_vertices_tolerance_mm: 1, triangulate: true },
    });
    await proj.load(options.loadedProject.id);
  }
  // Also seed projects store
  const projects = useProjectsStore();
  await projects.load();

  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: "/", name: "home", component: { template: "<div />" } },
      { path: "/projects/:id/design", name: "design", component: { template: "<div />" } },
      { path: "/projects/:id/method", name: "method", component: { template: "<div />" } },
      { path: "/projects/:id/import", name: "import", component: { template: "<div />" } },
      { path: "/projects/:id/charuco", name: "charuco", component: { template: "<div />" } },
      { path: "/projects/:id/photoplan", name: "photoplan", component: { template: "<div />" } },
      { path: "/projects/:id/preview", name: "preview", component: { template: "<div />" } },
      { path: "/projects/:id/instruct", name: "instruct", component: { template: "<div />" } },
      { path: "/projects/:id/runs", name: "runs", component: { template: "<div />" } },
    ],
  });
  await router.push(routePath);
  await router.isReady();

  const i18n = createI18n({ legacy: false, locale: "en", messages: { en } });

  return mount(LmtSidebar, { global: { plugins: [router, i18n] } });
}

describe("LmtSidebar — home state", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders Recent Projects group when recent list non-empty", async () => {
    const w = await mountSidebar("/", {
      recent: [
        { id: 1, abs_path: "/a", display_name: "alpha", last_opened_at: "2026-01-01" },
        { id: 2, abs_path: "/b", display_name: "beta", last_opened_at: "2026-02-01" },
      ],
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(w.text()).toContain("Recent Projects");
    expect(w.text()).toContain("alpha");
    expect(w.text()).toContain("beta");
  });

  it("omits Recent Projects group when list empty", async () => {
    const w = await mountSidebar("/", { recent: [] });
    await new Promise((r) => setTimeout(r, 0));
    expect(w.text()).not.toContain("Recent Projects");
  });

  it("limits Recent Projects to 5", async () => {
    const recent = Array.from({ length: 8 }).map((_, i) => ({
      id: i + 1,
      abs_path: `/p${i}`,
      display_name: `proj-${i}`,
      last_opened_at: `2026-01-0${(i % 7) + 1}`,
    }));
    const w = await mountSidebar("/", { recent });
    await new Promise((r) => setTimeout(r, 0));
    expect(w.findAll("[data-recent-project]")).toHaveLength(5);
  });
});

describe("LmtSidebar — project-internal state", () => {
  beforeEach(() => vi.clearAllMocks());

  it("hides SURVEY group when method is unset", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5 },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(w.text()).not.toContain("Survey");
    expect(w.text()).not.toContain("Import");
  });

  it("shows Import under SURVEY when method=m1", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5, method: "m1" },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(w.text()).toContain("Survey");
    expect(w.text()).toContain("Import");
    expect(w.text()).not.toContain("ChArUco");
  });

  it("shows Charuco + Photoplan when method=m2", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5, method: "m2" },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(w.text()).toContain("ChArUco");
    expect(w.text()).toContain("Photo Plan");
    expect(w.text()).not.toContain("Import");
  });

  it("output group order is Preview / Instruct / Runs (no Export)", async () => {
    const w = await mountSidebar("/projects/5/design", {
      recent: [{ id: 5, abs_path: "/p", display_name: "P", last_opened_at: "x" }],
      loadedProject: { id: 5, method: "m1" },
    });
    await new Promise((r) => setTimeout(r, 0));
    expect(w.text()).not.toContain("Export");
    const labels = w
      .findAll("[data-output-item]")
      .map((n) => n.text());
    expect(labels).toEqual(["Preview", "Instruct", "Runs"]);
  });
});
```

- [ ] **Step 9.2: Run test to confirm fail**

```bash
pnpm test --run src/components/shell/__tests__/LmtSidebar.spec.ts 2>&1 | tail -15
```

Expected: multiple failures (current sidebar still has Export, no Survey group, no Recent Projects).

- [ ] **Step 9.3: Replace `src/components/shell/LmtSidebar.vue` entirely** **[code]**

```vue
<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useRoute } from "vue-router";
import { useI18n } from "vue-i18n";
import { useProjectsStore } from "@/stores/projects";
import { useCurrentProjectStore } from "@/stores/currentProject";
import { useSurveyMethod } from "@/composables/useSurveyMethod";
import LmtIcon from "@/components/primitives/LmtIcon.vue";

const route = useRoute();
const { t } = useI18n();
const projects = useProjectsStore();
const proj = useCurrentProjectStore();
const { method } = useSurveyMethod();

onMounted(() => {
  projects.load().catch(() => {});
});

const projectId = computed(() => (route.params.id as string | undefined) ?? null);

const recentTop5 = computed(() => projects.recent.slice(0, 5));
const pinnedId = computed(() => recentTop5.value[0]?.id ?? null);

type NavItem = { to: string; label: string; icon: string };

const surveyItems = computed<NavItem[]>(() => {
  if (!projectId.value) return [];
  if (method.value === "m1") {
    return [
      { to: `/projects/${projectId.value}/import`, label: t("nav.import"), icon: "upload" },
    ];
  }
  if (method.value === "m2") {
    return [
      { to: `/projects/${projectId.value}/charuco`, label: t("nav.charuco"), icon: "qr-code" },
      { to: `/projects/${projectId.value}/photoplan`, label: t("nav.photoplan"), icon: "camera" },
    ];
  }
  return [];
});

const outputItems = computed<NavItem[]>(() => {
  if (!projectId.value) return [];
  return [
    { to: `/projects/${projectId.value}/preview`, label: t("nav.preview"), icon: "box" },
    { to: `/projects/${projectId.value}/instruct`, label: t("nav.instruct"), icon: "printer" },
    { to: `/projects/${projectId.value}/runs`, label: t("nav.runs"), icon: "list-checks" },
  ];
});

const outputDimmed = computed(() => !!projectId.value && method.value === null);
</script>

<template>
  <aside class="flex w-60 shrink-0 flex-col gap-6 border-r bg-sidebar p-3 text-sidebar-foreground">
    <div class="px-3 pb-1 pt-2">
      <p class="text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">LMT</p>
      <p class="mt-0.5 font-display text-sm font-extrabold text-sidebar-foreground">
        {{ t("app.title") }}
      </p>
    </div>

    <!-- Workspace -->
    <nav class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.workspace") }}
      </p>
      <RouterLink
        to="/"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon name="house" :size="15" />
        <span class="truncate">{{ t("nav.home") }}</span>
      </RouterLink>
    </nav>

    <!-- Home: Recent Projects -->
    <nav v-if="!projectId && recentTop5.length > 0" class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("home.recentProjects") }}
        <span class="font-mono text-[10px] text-muted-foreground/70">({{ recentTop5.length }})</span>
      </p>
      <RouterLink
        v-for="p in recentTop5"
        :key="p.id"
        data-recent-project
        :to="`/projects/${p.id}/design`"
        class="group flex items-center gap-2 truncate rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
      >
        <LmtIcon
          :name="p.id === pinnedId ? 'diamond' : 'folder'"
          :size="13"
          :class="p.id === pinnedId ? 'text-primary' : 'text-muted-foreground'"
        />
        <span class="truncate" :class="p.id === pinnedId ? 'font-bold text-foreground' : ''">
          {{ p.display_name }}
        </span>
      </RouterLink>
    </nav>

    <!-- Project-internal: Design group -->
    <nav v-if="projectId" class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.design") }}
      </p>
      <RouterLink
        :to="`/projects/${projectId}/design`"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon name="layout-grid" :size="15" />
        <span class="truncate">{{ t("nav.design") }}</span>
      </RouterLink>
      <RouterLink
        :to="`/projects/${projectId}/method`"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon name="compass" :size="15" />
        <span class="flex-1 truncate">{{ t("nav.method") }}</span>
        <LmtIcon
          v-if="method === null"
          name="diamond"
          :size="12"
          class="text-status-critical"
        />
        <span
          v-else
          class="font-mono text-[10px] uppercase tracking-wide text-muted-foreground"
        >{{ method }}</span>
      </RouterLink>
    </nav>

    <!-- Project-internal: Survey group (method-driven) -->
    <nav v-if="projectId && surveyItems.length > 0" class="flex flex-col gap-0.5">
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.survey") }}
      </p>
      <RouterLink
        v-for="it in surveyItems"
        :key="it.to"
        :to="it.to"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon :name="it.icon" :size="15" />
        <span class="truncate">{{ it.label }}</span>
      </RouterLink>
    </nav>

    <!-- Project-internal: Output group (always renders; dimmed when method=null) -->
    <nav
      v-if="projectId"
      class="flex flex-col gap-0.5"
      :class="outputDimmed ? 'opacity-50' : ''"
    >
      <p class="px-3 pb-1 text-[11px] font-bold uppercase tracking-[0.18em] text-muted-foreground">
        {{ t("nav.group.output") }}
      </p>
      <RouterLink
        v-for="it in outputItems"
        :key="it.to"
        data-output-item
        :to="it.to"
        class="group relative flex items-center gap-2.5 rounded-md border-l-2 border-transparent px-3 py-1.5 text-sm text-sidebar-foreground/80 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
        active-class="!border-sidebar-primary bg-sidebar-accent text-sidebar-accent-foreground font-bold"
      >
        <LmtIcon :name="it.icon" :size="15" />
        <span class="truncate">{{ it.label }}</span>
      </RouterLink>
    </nav>
  </aside>
</template>
```

- [ ] **Step 9.4: Run test to confirm pass**

```bash
pnpm test --run src/components/shell/__tests__/LmtSidebar.spec.ts 2>&1 | tail -15
```

Expected: all 7 pass.

- [ ] **Step 9.5: Typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: 0 errors.

- [ ] **Step 9.6: Commit**

```bash
git add src/components/shell/LmtSidebar.vue src/components/shell/__tests__/LmtSidebar.spec.ts
git commit -m "$(cat <<'EOF'
feat(sidebar): rewrite — two states, method-driven survey, recent on home

Home: Workspace + Recent Projects (top 5, pinned ◆ on most-recent).
Project-internal: Workspace + Design+Method, Survey (m1 → Import; m2 →
Charuco+Photoplan; null → hidden), Output (Preview/Instruct/Runs).
Output dimmed when method=null; sidebar subscribes to projects + current
project stores; race-safe via useSurveyMethod.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: `LmtMethodMismatchBanner` + embed in 6 views

**Files:**
- Create: `src/components/shell/LmtMethodMismatchBanner.vue`
- Modify: `src/views/Import.vue`
- Modify: `src/views/Charuco.vue`
- Modify: `src/views/Photoplan.vue`
- Modify: `src/views/Instruct.vue`
- Modify: `src/views/Preview.vue`
- Modify: `src/views/Runs.vue`

- [ ] **Step 10.1: Create `src/components/shell/LmtMethodMismatchBanner.vue`** **[code]**

```vue
<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { useSurveyMethod } from "@/composables/useSurveyMethod";
import LmtBanner from "@/components/primitives/LmtBanner.vue";

const props = defineProps<{
  expects: "m1" | "m2" | "any";
}>();

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const { method } = useSurveyMethod();

const id = computed(() => route.params.id as string);

const mismatch = computed(() => {
  if (props.expects === "any") return method.value === null;
  return method.value !== props.expects;
});

const title = computed(() => {
  if (method.value === null) return t("method.mismatch.unset");
  const current = method.value === "m1" ? t("method.m1.title") : t("method.m2.title");
  if (props.expects === "m1") return t("method.mismatch.m1Only", { current });
  if (props.expects === "m2") return t("method.mismatch.m2Only", { current });
  return "";
});

const key = computed(() => `mismatch-${id.value}-${route.name?.toString()}`);

function goPick() {
  router.push(`/projects/${id.value}/method`);
}
</script>

<template>
  <LmtBanner
    v-if="mismatch"
    tone="warn"
    icon="alert-triangle"
    :title="title"
    :action-label="t('method.mismatch.goPick')"
    :dismiss-key="key"
    @action="goPick"
  />
</template>
```

- [ ] **Step 10.2: Embed in `src/views/Import.vue`**

Open `src/views/Import.vue`. Add an import:

```ts
import LmtMethodMismatchBanner from "@/components/shell/LmtMethodMismatchBanner.vue";
```

In the `<template>`, immediately after the opening wrapper `<div class="flex h-full flex-col gap-6 p-6">` and **before** `<LmtPageHeader ...>`, insert:

```vue
    <LmtMethodMismatchBanner expects="m1" />
```

- [ ] **Step 10.3: Embed in `src/views/Charuco.vue`** (same pattern, `expects="m2"`)

Add the import and the `<LmtMethodMismatchBanner expects="m2" />` line right after the wrapper div opens.

- [ ] **Step 10.4: Embed in `src/views/Photoplan.vue`** (same, `expects="m2"`)

- [ ] **Step 10.5: Embed in `src/views/Instruct.vue`** (`expects="m1"`)

- [ ] **Step 10.6: Embed in `src/views/Preview.vue`** (`expects="any"`)

Preview's template starts with `<div class="flex h-full flex-col">` then has `<div class="px-6 pb-2 pt-5">`. Insert the banner inside that inner padding wrapper, **before** `<LmtPageHeader ...>`:

```vue
    <div class="px-6 pb-2 pt-5">
      <LmtMethodMismatchBanner expects="any" />
      <LmtPageHeader ... />
    </div>
```

- [ ] **Step 10.7: Embed in `src/views/Runs.vue`** (`expects="any"`)

Add import, place inside outermost wrapper before any page content.

- [ ] **Step 10.8: Typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: 0 errors.

- [ ] **Step 10.9: Run full frontend test suite to confirm no regression**

```bash
pnpm test --run 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 10.10: Commit**

```bash
git add src/components/shell/LmtMethodMismatchBanner.vue src/views/Import.vue src/views/Charuco.vue src/views/Photoplan.vue src/views/Instruct.vue src/views/Preview.vue src/views/Runs.vue
git commit -m "$(cat <<'EOF'
feat(banner): LmtMethodMismatchBanner + embed in 6 views

M1-only views (Import, Instruct) and M2-only views (Charuco, Photoplan)
get a warn-tone banner with "Go to Method →" action when the current
method does not match. Preview/Runs use expects=any (warn only when null).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: Design page method-pending banner

**Files:**
- Modify: `src/views/Design.vue`

- [ ] **Step 11.1: Add imports**

In `src/views/Design.vue` script block, add:

```ts
import LmtBanner from "@/components/primitives/LmtBanner.vue";
import { useSurveyMethod } from "@/composables/useSurveyMethod";
import { useRouter } from "vue-router";
```

(`useRouter` may already be imported — skip if so.)

Inside `<script setup>` after the existing const declarations, add:

```ts
const router = useRouter();
const { method: surveyMethod } = useSurveyMethod();

function goPickMethod() {
  router.push(`/projects/${id.value}/method`);
}
```

- [ ] **Step 11.2: Insert banner in template**

In `src/views/Design.vue` template, after the existing `<LmtPageHeader ...>` block closes (i.e., after the `</div>` that wraps the page header at `px-6 pb-2 pt-5`), and **before** `<DesignToolbar ...>`, insert:

```vue
    <div v-if="surveyMethod === null && id" class="px-6 pb-2">
      <LmtBanner
        tone="info"
        icon="info"
        :title="$t('design.banner.methodPending')"
        :action-label="$t('design.banner.go')"
        :dismiss-key="`design-method-banner-${id}`"
        @action="goPickMethod"
      />
    </div>
```

- [ ] **Step 11.3: Typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: 0 errors.

- [ ] **Step 11.4: Run full frontend tests**

```bash
pnpm test --run 2>&1 | tail -10
```

Expected: all pass.

- [ ] **Step 11.5: Commit**

```bash
git add src/views/Design.vue
git commit -m "$(cat <<'EOF'
feat(design): method-pending banner — guides users to Method after Design

Banner appears when project.method is null. Dismiss state lives in ui store
keyed by project id; reload re-shows the prompt until user picks a method.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 12: Output cleanup — remove Export, add catch-all

**Files:**
- Delete: `src/views/Export.vue`
- Modify: `src/router/index.ts`

- [ ] **Step 12.1: Delete the Export view**

```bash
rm src/views/Export.vue
```

- [ ] **Step 12.2: Update router**

In `src/router/index.ts`:

1. Remove the `import Export from "@/views/Export.vue";` line.
2. Remove the entire route block:
   ```ts
   {
     path: "/projects/:id/export",
     name: "export",
     component: Export,
     props: true,
   },
   ```
3. At the **end** of the `routes` array, add the catch-all:
   ```ts
     { path: "/:pathMatch(.*)*", name: "not-found", redirect: "/" },
   ```

- [ ] **Step 12.3: Verify no stragglers reference Export**

```bash
grep -rn "Export.vue\|name: \"export\"\|/export" src/ 2>&1 | grep -v "exportObj\|exportDisguise\|exportUnreal\|exportNeutral\|exportPickPath\|preview.export"
```

Expected: empty output (or only matches inside i18n which were removed in Task 7).

- [ ] **Step 12.4: Typecheck**

```bash
pnpm typecheck 2>&1 | tail -5
```

Expected: 0 errors. If a stale reference surfaces, follow the error to fix it.

- [ ] **Step 12.5: Run full test suite**

```bash
pnpm test --run 2>&1 | tail -10
cd src-tauri && cargo test 2>&1 | tail -5 && cd ..
```

Expected: all pass.

- [ ] **Step 12.6: Commit**

```bash
git add src/views/Export.vue src/router/index.ts
git commit -m "$(cat <<'EOF'
refactor(router): remove /export route + view; add catch-all redirect

Export merged into Preview toolbar (already shipped). Catch-all sends any
unknown URL (including old /export hash links) back to /.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 13: Manual verification + final checks

**Files:** none (verification only)

- [ ] **Step 13.1: Run full Rust suite**

```bash
cd src-tauri && cargo test 2>&1 | tail -5 && cd ..
```

Expected: `0 failed`.

- [ ] **Step 13.2: Run full frontend suite**

```bash
pnpm test --run 2>&1 | tail -10
```

Expected: every test passes (target: ≥ original 17 + new tests from Tasks 2/3/4/5/6/8/9 ≈ 35+).

- [ ] **Step 13.3: Typecheck + Rust check**

```bash
pnpm typecheck && (cd src-tauri && cargo check) 2>&1 | tail -10
```

Expected: clean.

- [ ] **Step 13.4: Boot dev app**

```bash
pnpm tauri dev
```

Wait until the window appears. Open dev tools (Cmd+Opt+I).

- [ ] **Step 13.5: Walk the manual checklist (from spec §11.3)**

In the running dev app, verify each item below. Tick the boxes inline. If any check fails, note the gap and stop — don't paper over it.

- [ ] (i) No console errors at boot.
- [ ] (ii) Home sidebar has `Recent Projects` group with up to 5 entries; most-recent has the `◆` icon.
- [ ] (iii) With zero projects on disk, sidebar shows only `Workspace / Home`.
- [ ] (iv) Open a fresh `curved-flat` example: sidebar shows `Design + Method (◆)` but **no** Survey group; `Output` group is dimmed (`opacity-50`).
- [ ] (v) Design page shows the `methodPending` info banner at top; dismiss closes it; reload re-shows it.
- [ ] (vi) Open `/method` → two cards, both `AVAILABLE`. Click `Use M1` → toast, sidebar instantly grows `Survey > Import`, Output un-dims, Design banner disappears.
- [ ] (vii) In `/method`, click `Switch to M2` → confirm dialog appears; cancel keeps M1; confirm switches to M2; sidebar swaps to `Survey > ChArUco / Photo Plan`.
- [ ] (viii) `measurements/measured.yaml` (if it existed pre-switch) is still on disk after switching to M2.
- [ ] (ix) Output order in sidebar: `Preview / Instruct / Runs`. No `Export` item.
- [ ] (x) Visit `/projects/<id>/instruct` while method=m2 → warn-tone mismatch banner at the top.
- [ ] (xi) Visit `/projects/<id>/charuco` while method=m1 → mismatch banner.
- [ ] (xii) Type `/projects/<id>/export` in URL bar → catch-all redirects to `/`.
- [ ] (xiii) On Preview, the right-side `EXPORT OBJ` cluster still shows Disguise / Unreal / Neutral and each button writes an `.obj`.

- [ ] **Step 13.6: Confirm green git status**

```bash
git status
```

Expected: working tree clean.

- [ ] **Step 13.7: Final summary**

Write a one-line completion note (e.g. "IA redesign complete — 12 tasks, NN tests added, NN tests passing"). No commit needed; this is the plan-done marker.

---

## Done criteria

- All Tasks 1–12 commits present in `git log`.
- `pnpm test --run` passes (full frontend suite).
- `cargo test` passes inside `src-tauri/`.
- `pnpm typecheck` clean; `cargo check` clean.
- Manual checklist 13.5 (i)–(xiii) all ticked, no regressions found.
- Push to `origin/main` when user explicitly says so (not part of the plan).
