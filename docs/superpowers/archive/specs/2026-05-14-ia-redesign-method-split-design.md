# IA Redesign — Method Path Split + Home Sidebar + Output Reorder (v2)

**Date:** 2026-05-14 (v2 — post Codex review revisions)
**Status:** Design — ready for implementation plan
**Scope:** UI/IA + project config DTO serialization. Touches:
- Rust DTO (`src-tauri/src/dto.rs`) + YAML round-trip
- Frontend: routes, sidebar, new Method view, Preview/Output cleanup, new shared primitives (Banner, ConfirmDialog), new composable for survey method
- No changes to reconstruction algorithms, no changes to M2 Charuco/Photoplan stubs themselves

---

## 1. Motivation

Five pieces of user feedback against the current mission-control UI, collapsed into three core decisions:

1. **路径分流**（feedback #2, #3）— M1 全站仪 / M2 视觉反算 当前在 sidebar 同组并列，技术路径不该混排。
2. **首页 sidebar 空白**（feedback #1）— 项目未选时 sidebar 只剩 Home 一项，浪费导航空间。
3. **输出区结构**（feedback #4, #5）— Output 顺序不直观，Export 视图是 stub（三张 tile 都跳回 Preview），独立路由冗余。

## 2. Direction Decisions (user-approved)

| 决策点 | 选择 |
|---|---|
| 路径分流入口 | **独立 Method 页**（新增 `/projects/:id/method`） |
| 首页 sidebar | **Recent Projects 快捷列表**（最多 5 个，pin 最后打开的） |
| Export 形态 | **合并进 Preview**（删除独立 `/export` 路由与视图） |
| Method 未选时 Output | **灰显可点**（不死锁导航） |
| project.yaml 字段命名 | `project.method: "m1" \| "m2"`（缺省即未选） |
| 切换 method 行为 | **不清空数据**（M1 / M2 产物共存） |
| Method 卡片 artifact 显示 | **不显示**（简化设计，未来再加 `get_project_artifacts` 命令） |
| `/export` 删除策略 | 直接删 route，加路由 catch-all redirect 兜底 |

## 3. Sidebar Structure

### 3.1 项目内（已选项目）

```
LMT · led-mesh-toolkit
─────────────────────────
WORKSPACE
  ⌂  Home
DESIGN
     Design
  ◆  Method          ← method 缺省时高亮 ◆ 提示「待选择」
SURVEY  (M1 / M2)    ← method 缺省时整个分组隐藏
     [子项随 method 而变]
OUTPUT
  ⊞  Preview         ← 合并了 Export
  ⎙  Instruct        ← M1-only flow，详见 §4
  ☰  Runs
```

**Method 状态判定（前端必须 nullish-coalesce）：**

```ts
// project.method 在 YAML/JSON 里可能 missing/undefined/null/"m1"/"m2"
const method = computed(() => proj.config?.project.method ?? null);
// 切勿写 === null，会漏掉 undefined（Rust serde skip_if 会 omit field）
```

**SURVEY 分组渲染：**
- `method == null` → 整组不渲染
- `method == "m1"` → 显示 `Import`
- `method == "m2"` → 显示 `Charuco` + `Photoplan`（并列）

**Method 项视觉：**
- 缺省 → 右侧 `◆`（`status-critical` 色） + tooltip「需要选择测量方式」
- 已选 → 右侧 mono small `M1` 或 `M2`

**Output 三项规则：**
- 任何状态都渲染。`method == null` 时整组 `opacity-50`，点击仍可进入。

### 3.2 首页（未选项目）

```
LMT · led-mesh-toolkit
─────────────────────────
WORKSPACE
  ⌂  Home
RECENT PROJECTS  (n)
  ◆  stage-east-wall    ← last_opened_at 最近，加粗 + ◆
     living-cube-2025
     showroom-v3
     truck-led-test
     arch-curve-r1
```

- 最多 5 个；超过 5 个第 6 起不进 sidebar，Home 主区列全部。
- 按 `last_opened_at` 倒序。
- 点击 → `router.push('/projects/${id}/design')`
- 项目数 = 0 时整组不渲染。

### 3.3 Sidebar Data Ownership（Codex Must-Fix）

**当前问题**：`LmtSidebar.vue` 只读 `route` + `i18n`，不订阅任何 store。`Home.vue` mount 时才 `projects.load()`。如果直接进项目 URL，sidebar 看不到 recent；项目切换时 sidebar 也看不到 method。

**改法**：
1. Sidebar 在 `onMounted` 调一次 `projects.load()`，订阅 `useProjectsStore().recent`（reactive）
2. Sidebar 订阅 `useCurrentProjectStore()`，读 `proj.config?.project.method`
3. **防 stale**：只有 `proj.id === Number(route.params.id)` 时才渲染基于 `proj.config` 的 SURVEY 子项；否则按"loading"态渲染（SURVEY 隐藏）
4. 不抽出 shared component（YAGNI——只有 sidebar 一处用）

## 4. Routes

```
/                                → Home
/projects/:id/design             → 平体设计
/projects/:id/method             → ★ 新增
/projects/:id/import             → M1
/projects/:id/charuco            → M2
/projects/:id/photoplan          → M2
/projects/:id/preview            → 预览 + 导出
/projects/:id/instruct           → 施工指令（M1-only flow，详见下）
/projects/:id/runs               → 运行历史
/:pathMatch(.*)*                 → ★ catch-all redirect to `/`
─ 删除 ─
/projects/:id/export             ×
```

### 4.1 Method Mismatch Banner（共享组件）

页面级 banner 由新增的 `<LmtMethodMismatchBanner>` 提供（不是装饰，是行为）：

| 页面 | 触发条件 | banner 内容 |
|---|---|---|
| `/import` | method != "m1" | 「当前 method 是 {x}，Import 仅用于 M1，去切换？」 + `[去 Method]` |
| `/charuco` | method != "m2" | 同上，M2 |
| `/photoplan` | method != "m2" | 同上，M2 |
| `/instruct` | method != "m1" | 「Instruct 当前仅支持 M1 流程（走 total-station adapter）」 + `[去 Method]` |
| `/preview`、`/runs` | method == null | 「未选测量方式，预览/历史数据可能为空」 + `[去 Method]` |
| `/method` | n/a | 不显示 banner |

页面继续渲染原内容（不强制空态），banner 只是顶部一条提示。

### 4.2 `/export` 删除后的兜底（Codex Should-Fix）

router 加 catch-all：
```ts
{ path: "/:pathMatch(.*)*", redirect: "/" }
```
保证旧 `/projects/:id/export` 书签或 hash 访问会跳回首页，不出现空白 RouterView。

### 4.3 Instruct = M1-only 的明确（Codex Must-Fix #5）

`Instruct.vue` 当前写死 `LmtStatusBadge label="M1"`，调用 total-station adapter。本 spec **不重写 Instruct**，只做两件事：
1. method != "m1" 时顶部出 method-mismatch banner（§4.1）
2. sidebar 上 Instruct 子项保持渲染（任何 method 都点得进去），但点开就看到 banner

未来要做 M2 版 Instruct 是单独的项目，不在本 spec 内。

## 5. Method 页设计

### 5.1 布局（简化版——去掉 artifact 显示）

```
┌──────────────────────────────────────────────────────┐
│ MEASUREMENT METHOD                                   │
│ 测量方式选择                                          │
│ ──────────────────────────────────────────────────── │
│  ┌──────────────────┐    ┌──────────────────┐        │
│  │ [radio-tower]     │    │ [scan-eye]        │      │
│  │  M1 · 全站仪      │    │  M2 · 视觉反算    │       │
│  │  ●  CURRENT      │    │  ○  AVAILABLE    │       │
│  │  ──────────────  │    │  ──────────────  │       │
│  │  • CSV 导入       │    │  • ArUco/Charuco │       │
│  │  • 毫米级精度     │    │  • 手机或相机即可  │       │
│  │  • 需要专业设备    │    │  • 设备门槛低     │       │
│  │  ──────────────  │    │  ──────────────  │       │
│  │  [ 继续使用 M1 ]  │    │  [ 切换到 M2 ]   │       │
│  └──────────────────┘    └──────────────────┘        │
│                                                      │
│  ⓘ 切换 method 不会删除已有产物；measurements/ 与     │
│    aruco/ 目录共存，可随时切回。                       │
└──────────────────────────────────────────────────────┘
```

**关键状态：**
- `●  CURRENT` — 当前选中（卡片整体 `border-primary`）
- `○  AVAILABLE` — 可切换
- `method == null` 时两张卡都是 `○`，按钮变 `[ 使用 M1 ]` / `[ 使用 M2 ]`

**图标：**
- M1 → lucide `radio-tower`
- M2 → lucide `scan-eye`
- Sidebar Method icon → lucide `compass`

**Artifact 显示**：本版 spec **不做**——卡片去掉"当前产物"区块。理由：
- 当前 frontend 无 `get_project_artifacts` 命令
- 让 spec 落地不要硬猜 filesystem 状态
- 产物状态用户在 Import / Charuco 页本身能看，不必再 Method 页重复
- 未来加产物显示是独立的小特性（新增 Tauri command + 卡片底部加 section），不阻塞本次 IA 重构

### 5.2 切换流程

新增共享组件 `<LmtConfirmDialog>`（基于 reka-ui Dialog 包一层）：

```
切换到 M2 · 视觉反算
──────────────────────
切换 method 不会删除已有数据，你可以随时切回。

[ 取消 ]  [ 确认切换 ]
```

文案不再引用具体 artifact count（去掉"measured.yaml · 84 vertices"）——与 §5.1 的去 artifact 决策一致。

**`<LmtConfirmDialog>` API（新增）：**
```vue
<LmtConfirmDialog
  v-model:open="open"
  :title="t('method.confirmSwitch.title')"
  :body="t('method.confirmSwitch.body', { target: 'M2 · 视觉反算' })"
  :ok-label="t('method.confirmSwitch.ok')"
  :cancel-label="t('method.confirmSwitch.cancel')"
  :ok-tone="'default'"
  @confirm="doSwitch"
/>
```

实现：用 reka-ui `Dialog`/`DialogContent`/`DialogTitle`/`DialogDescription` + 两个 Button。

确认后：
1. `proj.setMethod("m2")` （新增 store action）
2. 写 yaml（store 内部 await save）
3. sidebar 立即刷新
4. 停留在 `/method`，不跳转

### 5.3 初次进入引导 Banner

`method == null` 时，Design 页顶部插 banner。新增共享组件 `<LmtBanner>`（不是装饰，是 dismissable 提示）：

```
┌──────────────────────────────────────────────────┐
│ ⓘ 平体设计就绪，下一步去选择测量方式              │ × │
│                            [ 去选择 → ]              │
└──────────────────────────────────────────────────┘
```

**`<LmtBanner>` API（新增）：**
```vue
<LmtBanner
  tone="info"
  :icon="'info'"
  :title="t('design.banner.methodPending')"
  :action-label="t('design.banner.go')"
  :dismiss-key="`design-method-banner-${projectId}`"
  @action="router.push(`/projects/${projectId}/method`)"
/>
```

**关闭状态持久化（Codex Should-Fix #1）：**
- 关闭状态写 `ui` store 的 `dismissedBanners: Set<string>`（per-key dismiss）
- 跨 route 不丢，跨 session 重置（不写 localStorage——希望下次启动用户能再看到引导）
- `dismiss-key` 包含 `projectId` 以便不同项目独立计数

实现：`useUiStore()` 加一对 `isBannerDismissed(key)` / `dismissBanner(key)`。

## 6. Data Model Changes (Codex Must-Fix #1, #2)

### 6.1 `project.yaml` schema

```yaml
project:
  name: Curved-Flat-Demo
  unit: mm
  # method 可选字段。缺省/null/"m1"/"m2" 都合法。
  method: m1
screens: ...
```

**例子项目**：`examples/curved-flat/project.yaml`、`examples/curved-arc/project.yaml` **不**预填 `method`，保留示例的"初始状态"语义。

### 6.2 Rust (`src-tauri/src/dto.rs`)

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

**Serde 行为表**：

| YAML 内容 | `Option<SurveyMethod>` |
|---|---|
| 字段缺省 | `None` |
| `method:` (null) | `None` |
| `method: m1` | `Some(M1)` |
| `method: m2` | `Some(M2)` |
| `method: m3` | serde error → load 失败，前端收到 `LmtError` |

**Tauri JSON 行为**：序列化到前端时 `None` 走 `skip_serializing_if`，**字段直接消失**。前端必须用 `?? null`，绝不能 `=== null`。

### 6.3 TypeScript (`src/services/tauri.ts`)

```ts
export type SurveyMethod = "m1" | "m2";

export interface ProjectMeta {
  name: string;
  unit: string;
  method?: SurveyMethod;   // ★ 新增。undefined 即未选
}
```

### 6.4 Store (`src/stores/currentProject.ts`)

新增：
```ts
async function setMethod(method: SurveyMethod): Promise<void> {
  if (!config.value || !absPath.value) return;
  config.value = {
    ...config.value,
    project: { ...config.value.project, method },
  };
  dirty.value = true;
  await save();
}
```

### 6.5 Composable (`src/composables/useSurveyMethod.ts`，新增)

```ts
export function useSurveyMethod() {
  const proj = useCurrentProjectStore();
  const route = useRoute();
  const method = computed<SurveyMethod | null>(() => {
    if (proj.id !== Number(route.params.id)) return null;
    return proj.config?.project.method ?? null;
  });
  return { method, setMethod: proj.setMethod };
}
```

Sidebar、Method 页、所有 mismatch banner 都通过这个 composable 拿值，确保 race-safe。

## 7. Output 区合并

**删除：**
- `src/views/Export.vue`
- `src/router/index.ts` 中 `name: "export"` 路由
- i18n `nav.export` + `export.*` 命名空间（en + zh）
- `LmtSidebar` output 组里的 export 条目

**改：**
- LmtSidebar output 顺序：`preview → instruct → runs`
- router 加 `{ path: "/:pathMatch(.*)*", redirect: "/" }`（catch-all）

**PreviewToolbar.vue 不动**——现状已是目标形态：顶 Reconstruct 主按钮 + Status badge + 右侧 `EXPORT OBJ` micro-label + 三个 outline 按钮平铺。

## 8. New Shared Primitives (Codex Must-Fix #4)

下面 3 个组件不存在，必须新增：

| 文件 | 职责 | 依赖 |
|---|---|---|
| `src/components/primitives/LmtBanner.vue` | 一行行内提示，可关闭，含 action 按钮槽 | `LmtIcon`, `Button` |
| `src/components/primitives/LmtConfirmDialog.vue` | confirm 对话框，title/body/ok/cancel | `reka-ui` Dialog, `Button` |
| `src/components/shell/LmtMethodMismatchBanner.vue` | 业务封装：根据当前 route + method 决定显示哪条 mismatch banner | `LmtBanner`, `useSurveyMethod`, `vue-i18n` |

`reka-ui` 已在 `package.json:26` (^2.9.6)，无需新增依赖。

## 9. i18n Changes

### 9.1 新增 keys

```
nav.group.survey            "Survey" / "测量"
nav.method                  "Method" / "测量方式"

method.eyebrow              "MEASUREMENT METHOD"
method.title                "Method"
                            / "测量方式选择"
method.description          "Pick how you'll measure cabinet vertices for this project."
                            / "选择本项目所用的测量方式。"
method.m1.title             "M1 · Total Station" / "M1 · 全站仪"
method.m1.desc              "Use a total station to capture vertex coordinates as CSV."
                            / "用全站仪测量顶点坐标，导入 CSV。"
method.m1.bullets           i18n array of 3 strings (via vue-i18n `tm()`):
                            ["CSV import",
                             "Millimeter-level precision",
                             "Requires pro hardware"]
                          / ["导入 CSV",
                             "毫米级精度",
                             "需要专业设备"]
method.m2.title             "M2 · Visual Back-Calc" / "M2 · 视觉反算"
method.m2.desc              "Recover surface from ArUco/Charuco photos taken with any camera."
                            / "用 ArUco/Charuco 标记拍照，反算出顶点空间位置。"
method.m2.bullets           ["ArUco / Charuco markers",
                             "Any phone or camera",
                             "Low equipment cost"]
                          / ["ArUco / Charuco 标记",
                             "手机或相机即可",
                             "设备门槛低"]
method.current              "CURRENT"
method.available            "AVAILABLE"
method.useM1                "Use M1" / "使用 M1"
method.useM2                "Use M2" / "使用 M2"
method.continueM1           "Continue with M1" / "继续使用 M1"
method.continueM2           "Continue with M2" / "继续使用 M2"
method.switchToM1           "Switch to M1" / "切换到 M1"
method.switchToM2           "Switch to M2" / "切换到 M2"
method.coexistNote          "Switching preserves existing data — measurements/ and aruco/
                             directories coexist, swap any time."
                          / "切换 method 不会删除已有产物；measurements/ 与 aruco/ 目录
                             共存，可随时切回。"
method.confirmSwitch.title  "Switch method" / "切换测量方式"
method.confirmSwitch.body   Template with {target}:
                            "Switching to {target}. Existing data is preserved and you can
                             switch back any time."
                          / "切换到 {target}。已有数据将保留，可随时切回。"
method.confirmSwitch.ok     "Confirm switch" / "确认切换"
method.confirmSwitch.cancel "Cancel" / "取消"

method.mismatch.m1Only      "This page is M1-only. Current method: {current}."
                          / "本页仅用于 M1，当前 method：{current}。"
method.mismatch.m2Only      "This page is M2-only. Current method: {current}."
                          / "本页仅用于 M2，当前 method：{current}。"
method.mismatch.unset       "Measurement method not selected yet."
                          / "尚未选择测量方式。"
method.mismatch.goPick      "Go to Method →" / "去选择 →"

home.recentProjects         "Recent Projects" / "最近项目"
home.pinned                 "PINNED"

design.banner.methodPending "Design ready · pick your measurement method"
                          / "平体设计就绪，下一步去选择测量方式"
design.banner.go            "Choose →" / "去选择 →"
```

### 9.2 删除 keys
- `nav.export`
- 全部 `export.*` 子树
- `home.actionsTitle` / `home.actionsDesc`（同步精简 Home 主区底部 actions 卡片——保留按钮，删 eyebrow）

## 10. Visual / Token Usage

- Method 卡片选中：`border-primary` + `bg-primary/5`
- Method 卡片未选：`border-border` + `bg-card`
- `CURRENT` badge：`bg-primary/10 text-primary border-primary/30`
- `AVAILABLE` badge：`bg-muted/30 text-muted-foreground border-border`
- LmtBanner tones：
  - `info` → `bg-status-info/10 border-status-info/30 text-status-info`
  - `warn` → `bg-amber-500/10 border-amber-500/30 text-amber-500`
- 沿用现有 mission-control flat-only 规则（无 shadow、无 gradient）

## 11. Testing Strategy

### 11.1 Rust tests (Codex Must-Fix #6/Should-Fix)

新增 `src-tauri/src/dto.rs` 同模块的 `#[cfg(test)] mod tests`：

| 用例 | 期望 |
|---|---|
| `method_missing_yaml_parses_as_none` | 缺字段 → `Option::None` |
| `method_null_yaml_parses_as_none` | `method: null` → `Option::None` |
| `method_m1_yaml_roundtrips` | `method: m1` → `Some(M1)` → serialize 回 `method: m1` |
| `method_m2_yaml_roundtrips` | 同上 |
| `method_invalid_value_errors` | `method: m3` → `serde_yaml::Error` |
| `none_omitted_on_serialize` | `None` 序列化后 yaml 不含 `method` 行 |

新增 `src-tauri/src/commands/projects.rs` 集成测试：
| 用例 | 期望 |
|---|---|
| `load_save_roundtrip_with_method_m1` | 写入临时目录 → load → method=Some(M1) → save → reload 仍然 M1 |
| `load_legacy_yaml_without_method` | 用 `examples/curved-flat/project.yaml` → load 成功，method=None |

### 11.2 Frontend unit tests (vitest)

| 文件 | 覆盖 |
|---|---|
| `src/stores/__tests__/currentProject.test.ts` | (+) `setMethod("m1")` 写 yaml + dirty=true；load 缺字段时 `config.project.method` 是 undefined |
| `src/composables/__tests__/useSurveyMethod.spec.ts` | (新) project mismatch race → method 为 null；正常 sync → 返回当前 method |
| `src/components/shell/__tests__/LmtSidebar.spec.ts` | (新) method=null/m1/m2 三态下 SURVEY 组渲染；首页态 Recent Projects 渲染 |
| `src/views/__tests__/Method.spec.ts` | (新) 两张卡渲染、点击触发 setMethod、confirm dialog 弹出/取消/确认 |
| `src/components/primitives/__tests__/LmtBanner.spec.ts` | (新) tone class、dismiss-key 存到 ui store |
| `src/components/primitives/__tests__/LmtConfirmDialog.spec.ts` | (新) open/close、confirm 触发事件、cancel 不触发 |

### 11.3 Manual integration

执行清单（dev build 起来后用浏览器验证）：

1. `pnpm tauri dev` 起来，无 console error
2. 首页 sidebar 出现 `RECENT PROJECTS` 列表，最后打开的项目标 PINNED
3. 项目数 = 0 时 sidebar 仅 `WORKSPACE / Home`，主区空态正常
4. 进入 `method` 缺省的项目（新建 curved-flat）：
   - sidebar 看不到 SURVEY 组
   - Method 项右侧有 `◆`
   - Design 页顶部出现 banner 引导
   - 关闭 banner 后在本 session 不再出现；reload 重新出现
5. 进 Method 选 M1：sidebar 立刻长出 `Import`，Design banner 消失
6. 切到 M2：confirm dialog 弹出 → 确认 → sidebar 切到 `Charuco + Photoplan`，`measurements/measured.yaml` 仍在磁盘
7. Output 区顺序：`Preview → Instruct → Runs`
8. method=m2 时访问 `/projects/:id/instruct` → 顶部 mismatch banner 出现
9. method=m1 时访问 `/projects/:id/charuco` → mismatch banner 出现
10. `/projects/:id/export` URL 手敲访问 → 被 catch-all 重定向到 `/`
11. Preview 页 toolbar 右侧 `EXPORT OBJ` 三按钮正常导出

### 11.4 Type / lint
- `pnpm typecheck` 全过
- `pnpm test --run` 全过
- `cargo test` 全过
- `cargo check` 全过

## 12. Out of Scope

- 不重画 CabinetGrid / MeshPreview 等画布内部
- 不动 Charuco / Photoplan 页面本体（仍是 M2 stub）
- 不重构 Instruct 让它支持 M2（独立项目）
- 不做 method 切换时的数据迁移
- 不做 `get_project_artifacts` Tauri 命令（Method 卡片暂不显示产物）
- 不做 catch-all 之外的精细 404 页面
- 不加 Export 高级功能（批量、预设）

## 13. Implementation Order (preview)

详细 plan 由 `superpowers:writing-plans` 生成，预计任务顺序（按 Codex 建议）：

1. **DTO 层**：`SurveyMethod` enum + `ProjectMeta.method` + Rust serde tests + `load/save_project_yaml` 集成测试
2. **共享 primitives**：`LmtBanner`、`LmtConfirmDialog` + 单测
3. **Composable**：`useSurveyMethod` + 单测
4. **Store action**：`currentProject.setMethod` + 单测
5. **Method 页**：`/projects/:id/method` 路由 + `Method.vue` 视图（用上面所有依赖）
6. **LmtSidebar 重写**：项目内 method-driven SURVEY 组 + 首页 Recent Projects（订阅 stores、防 stale）
7. **Method mismatch banner**：`LmtMethodMismatchBanner` 包装 + 嵌入各 view 顶部
8. **Design banner**：`design.banner.methodPending` 嵌入 Design.vue 顶部，ui store dismiss 状态
9. **Output 清理**：删 `Export.vue` / route / i18n key；改 LmtSidebar Output 顺序；加 catch-all 路由
10. **i18n 补全**：en.json + zh.json 完整 keys
11. **Manual checklist 跑完**
