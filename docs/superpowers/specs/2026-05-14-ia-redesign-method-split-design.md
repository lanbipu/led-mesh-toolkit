# IA Redesign — Method Path Split + Home Sidebar + Output Reorder

**Date:** 2026-05-14
**Status:** Design — ready for implementation plan
**Scope:** UI/IA only. No backend reconstruction logic changes.

---

## 1. Motivation

Five pieces of user feedback against the current mission-control UI, which collapse into three core decisions:

1. **路径分流**（feedback #2, #3）— 全站仪（M1）与视觉反算（M2）当前在 sidebar 同组并列，用户认为两种技术路径不该混排。
2. **首页 sidebar 空白**（feedback #1）— 项目未选时 sidebar 只剩一个 Home 项，浪费导航空间且无法快速回到最近项目。
3. **输出区结构**（feedback #4, #5）— Output 组顺序不直观，Export 视图当前是 stub（三张 tile 全部跳回 Preview），独立路由冗余。

## 2. Direction Decisions (user-approved)

| 决策点 | 选择 |
|---|---|
| 路径分流入口 | **独立 Method 页**（新增 `/projects/:id/method` 路由） |
| 首页 sidebar | **Recent Projects 快捷列表**（最多 5 个，pin 最后打开的） |
| Export 形态 | **合并进 Preview**（删除独立 `/export` 路由与视图） |
| Method 未选时 Output | **灰显可点**（不死锁导航） |
| project.yaml 字段命名 | `project.method: "m1" \| "m2" \| null` |
| 切换 method 行为 | **不清空数据**（M1 / M2 产物共存） |
| 旧 `/export` 重定向 | **不要**（直接删，无外链顾虑） |

## 3. Sidebar Structure

### 3.1 项目内（已选项目）

```
LMT · led-mesh-toolkit
─────────────────────────
WORKSPACE
  ⌂  Home
DESIGN
     Design
  ◆  Method          ← method=null 时高亮 ◆ 提示「待选择」
SURVEY  (M1 / M2)    ← method=null 时整个分组隐藏
     [子项随 method 而变]
OUTPUT
  ⊞  Preview         ← 合并了 Export 的 toolbar 按钮
  ⎙  Instruct
  ☰  Runs
```

**SURVEY 分组子项规则：**
- `method = null` → 分组不渲染
- `method = "m1"` → 显示 `Import` 一个子项
- `method = "m2"` → 显示 `Charuco` + `Photoplan` 两个子项（并列，无强制顺序）

**Method 项视觉提示：**
- `method = null` → 右侧 `◆` 图标（`status-critical` 色）+ tooltip「需要选择测量方式」
- `method ≠ null` → 右侧显示当前 method 文字（`M1` / `M2`）灰色 mono small

**Output 三项规则：**
- 任何 method 状态下都渲染，但 `method = null` 时整组 `opacity-50`，点击仍可进入（用户进入后看到的是各自的空 / 灰态）
- 顺序固定为 **Preview → Instruct → Runs**

### 3.2 首页（未选项目）

```
LMT · led-mesh-toolkit
─────────────────────────
WORKSPACE
  ⌂  Home
RECENT PROJECTS  (n)
  ◆  stage-east-wall    ← last_opened_at 最近的，加粗 + ◆ icon
     living-cube-2025
     showroom-v3
     truck-led-test
     arch-curve-r1
```

- 最多渲染 5 个；超出 5 个时 sidebar 不展开第 6 个，Home 主区显示完整列表（不动）。
- 排序：按 `last_opened_at` 倒序。
- 点击项目 → `router.push('/projects/${id}/design')`（默认进 Design，不预判 method）。
- 项目数 = 0 时整组不渲染。

## 4. Routes

```
/                                → Home (project list)
/projects/:id/design             → 平体设计
/projects/:id/method             → ★ 新增：路径选择
/projects/:id/import             → M1: 全站仪 CSV
/projects/:id/charuco            → M2: Charuco 校准
/projects/:id/photoplan          → M2: 拍摄计划
/projects/:id/preview            → 预览 + 导出（合并 export）
/projects/:id/instruct           → 施工指令
/projects/:id/runs               → 运行历史
─ 删除 ─
/projects/:id/export             × 删
```

**Route guard（最低限度）：**
- `/import` 仅 `method=m1` 时在 sidebar 显示；URL 手敲访问仍可达，顶部出现 `<LmtBanner>` 提示「当前 method 为 m2，建议先回 Method 切换」
- `/charuco` `/photoplan` 同理仅 m2
- `/preview` `/instruct` `/runs` 不限 method

## 5. Method 页设计

### 5.1 布局

两张大对比卡 + 共存说明栏 + 切换按钮。

```
┌──────────────────────────────────────────────────────┐
│ MEASUREMENT METHOD                                   │
│ 测量方式选择                                          │
│ ──────────────────────────────────────────────────── │
│  ┌──────────────────┐    ┌──────────────────┐        │
│  │ [icon-radio]      │    │ [icon-camera]     │      │
│  │  M1 · 全站仪      │    │  M2 · 视觉反算    │       │
│  │  ●  CURRENT      │    │  ○  AVAILABLE    │       │
│  │  ──────────────  │    │  ──────────────  │       │
│  │  • CSV 导入       │    │  • ArUco/Charuco │       │
│  │  • 高精度毫米级    │    │  • 手机/相机拍照  │       │
│  │  • 需要专业设备    │    │  • 设备门槛低     │       │
│  │  ──────────────  │    │  ──────────────  │       │
│  │  当前产物：       │    │  当前产物：       │        │
│  │  ✓ measured.yaml  │    │  — 无            │       │
│  │  ✓ 84 vertices    │    │                  │       │
│  │  ──────────────  │    │  ──────────────  │       │
│  │  [ 继续使用 M1 ]  │    │  [ 切换到 M2 ]   │       │
│  └──────────────────┘    └──────────────────┘        │
│                                                      │
│  ⓘ 切换 method 不会删除已有产物；measurements/ 与     │
│    aruco/ 目录共存，可随时切回。                       │
└──────────────────────────────────────────────────────┘
```

**关键状态语义：**
- `●  CURRENT` — 当前选中（卡片整体加 `border-primary` 强调）
- `○  AVAILABLE` — 可切换
- `method = null` 时两张卡都是 `○`，按钮文字变 `[ 使用 M1 ]` / `[ 使用 M2 ]`

**图标选择：**
- M1 → lucide `radio-tower`（全站仪信号塔）
- M2 → lucide `scan-eye`（视觉扫描）
- Method 自身 sidebar icon → lucide `compass`

### 5.2 切换流程

点击 `[ 切换到 M2 ]` 触发 confirm dialog（用 `<LmtConfirmDialog>` 复用现有 Dialog primitive，无则新增）：

```
切换到 M2 · 视觉反算
──────────────────────
已有 M1 产物（measured.yaml · 84 vertices）将保留，
但不会用于 M2 流程。你可以随时切回 M1。

[ 取消 ]  [ 确认切换 ]
```

确认后：
1. `proj.config.project.method = "m2"`
2. `proj.save()` 写入 yaml
3. sidebar 立即刷新（SURVEY 区子项切换）
4. 停留在 `/method` 页面，**不强制跳转**——用户自己决定下一步

### 5.3 初次进入引导

`project.method = null` 时，Design 页顶部插入 banner（不是 toast）：

```
┌──────────────────────────────────────────────────┐
│ ⓘ 平体设计就绪，下一步去选择测量方式              │
│                            [ 去选择 → ]          │
└──────────────────────────────────────────────────┘
```

- banner 用 `bg-status-info/10 border-status-info/30 text-status-info` 配色
- 关闭按钮 `×` 仅 session 级隐藏（reload 会再出现，直到 method 被选）
- 点击 `[ 去选择 → ]` → `router.push('/projects/${id}/method')`

## 6. Data Model Changes

### 6.1 `project.yaml` schema

```yaml
project:
  name: Curved-Flat-Demo
  unit: mm
  method: m1          # ★ 新增。允许值: m1 | m2 | null（默认 null）
screens:
  ...
```

### 6.2 TypeScript (`src/services/tauri.ts`)

```ts
export interface ProjectMeta {
  name: string;
  unit: string;
  method?: "m1" | "m2" | null;   // ★ 新增
}
```

### 6.3 Rust (`src-tauri/.../config.rs` 或同等位置)

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProjectMeta {
    pub name: String,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<SurveyMethod>,   // ★ 新增
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SurveyMethod {
    M1,
    M2,
}
```

### 6.4 示例 `project.yaml`（curved-flat / curved-arc）

**不改**——保持 `method` 字段缺省，让用户首次进入示例项目时也走一次 Method 页（保留示例的「初始状态」体验）。

### 6.5 Store 接口（`src/stores/currentProject.ts`）

新增：
```ts
function setMethod(method: "m1" | "m2"): Promise<void> {
  // 写入 config.project.method、dirty=true、await save()
}
```

## 7. Output 区合并（Section 4 收尾）

**删除：**
- `src/views/Export.vue`
- `src/router/index.ts` 中 `name: "export"` 路由
- i18n `nav.export`、整个 `export.*` 命名空间（en.json + zh.json）
- LmtSidebar 中 `output` 组的 `export` 条目

**改：**
- LmtSidebar `output` 组顺序：`preview → instruct → runs`

**保留不动（PreviewToolbar.vue 已经是目标形态）：**
- 顶部 `Reconstruct` 主按钮
- Status badge
- 右侧 `EXPORT OBJ` micro-label + 三个 outline 按钮（Disguise / Unreal / Neutral）
- 三个按钮平铺不换 dropdown

## 8. i18n Changes

### 8.1 新增 keys

```
nav.group.survey            "Survey" / "测量"
nav.method                  "Method" / "测量方式"

method.eyebrow              "MEASUREMENT METHOD"
method.title                "Method"
                            / "测量方式选择"
method.description          "Pick how you'll measure cabinet vertices for this project."
                            / "选择本项目所用的测量方式。"
method.m1.title             "M1 · Total Station"
                            / "M1 · 全站仪"
method.m1.desc              "Use a total station to capture vertex coordinates as CSV."
                            / "用全站仪测量顶点坐标，导入 CSV。"
method.m1.bullets           i18n array of 3 strings (consumed via vue-i18n `tm()`):
                            ["CSV import",
                             "Millimeter-level precision",
                             "Requires pro hardware"]
                          / ["导入 CSV",
                             "毫米级精度",
                             "需要专业设备"]
method.m2.title             "M2 · Visual Back-Calc"
                            / "M2 · 视觉反算"
method.m2.desc              "Recover surface from ArUco/Charuco photos taken with any camera."
                            / "用 ArUco/Charuco 标记拍照，反算出顶点空间位置。"
method.m2.bullets           i18n array of 3 strings:
                            ["ArUco / Charuco markers",
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
method.confirmSwitch.body   Template string with {target} (target method label) and {artifacts}
                            (current artifact summary, e.g. "measured.yaml · 84 vertices"):
                            "Switching to {target}. Existing artifacts ({artifacts}) will be
                             kept but not used by the new flow. You can switch back any time."
                          / "切换到 {target}。已有产物（{artifacts}）将保留，但不会用于新流程。
                             可随时切回。"
method.confirmSwitch.ok     "Confirm switch" / "确认切换"
method.confirmSwitch.cancel "Cancel" / "取消"

home.recentProjects         "Recent Projects" / "最近项目"
home.pinned                 "PINNED"

design.banner.methodPending "Design ready · pick your measurement method"
                            / "平体设计就绪，下一步去选择测量方式"
design.banner.go            "Choose →" / "去选择 →"
```

### 8.2 删除 keys
- `nav.export`
- 全部 `export.*` 子树
- `home.actionsTitle` / `home.actionsDesc`（同步精简 Home 主区底部那张 actions 卡——保留按钮，删 eyebrow 文字）

## 9. Visual / Token Usage

- Method 卡片选中态：`border-primary` + `bg-primary/5`
- Method 卡片未选态：`border-border` + `bg-card`
- `CURRENT` badge：`bg-primary/10 text-primary border-primary/30`
- `AVAILABLE` badge：`bg-muted/30 text-muted-foreground border-border`
- Design banner：`bg-status-info/10 border-status-info/30 text-status-info`
- 沿用现有 mission-control flat-only 规则（无 shadow、无 gradient）

## 10. Testing Strategy

### 10.1 Unit tests (vitest)

| 文件 | 覆盖 |
|---|---|
| `src/stores/__tests__/currentProject.test.ts` | (+) `setMethod("m1")` 写 yaml + dirty=true；从 yaml 读 method 字段 |
| `src/components/shell/__tests__/LmtSidebar.spec.ts` | (新) method=null/m1/m2 三种状态下 SURVEY 分组渲染正确子项 |
| `src/views/__tests__/Method.spec.ts` | (新) 两张卡渲染、点击触发 setMethod、switch confirm dialog 行为 |
| `src/components/preview/__tests__/PreviewToolbar.test.ts` | (回归，如已有) 确保 export 按钮三个仍工作 |

### 10.2 Manual integration

执行清单（dev build 起来后用浏览器验证）：

1. `cargo tauri dev` 起来，无 console error
2. 首页 sidebar 出现 `RECENT PROJECTS` 列表，最后打开的项目 pin 标
3. 项目数 = 0 时 sidebar 仅 `WORKSPACE / Home`，主区空态正常
4. 进入一个 `method=null` 的项目（如新建的 curved-flat）：
   - sidebar 看不到 SURVEY 组
   - Method 项右侧有 `◆`
   - Design 页顶部出现 banner 引导
5. 进 Method 页选 M1：sidebar 立刻长出 `Import` 子项，Design banner 消失
6. 切到 M2：confirm dialog 弹出 → 确认 → sidebar 切到 `Charuco` + `Photoplan`，且 `measurements/measured.yaml` 仍在磁盘上未被删
7. Output 区顺序：`Preview → Instruct → Runs`
8. `/projects/:id/export` URL 手敲访问 → 404 或 router fallback（不重定向）
9. Preview 页 toolbar 右侧 `EXPORT OBJ` 三个按钮正常导出

### 10.3 Type / lint
- `pnpm typecheck` 全过
- `pnpm test --run` 全过
- `cargo check` 全过

## 11. Out of Scope

- 不重画 CabinetGrid / MeshPreview 等画布内部
- 不动 Charuco / Photoplan 页面本体（保持当前 M2 stub 占位态——sidebar 改完后这两页在 M2 模式下可访问就行）
- 不做 method 切换时的数据迁移逻辑（设计上承诺"共存"，靠 measurements/ 和 aruco/ 目录天然分离）
- 不加 Export 高级功能（批量、预设）——若未来需要，再开 Export modal

## 12. Implementation Order (preview)

详细 plan 将由 `superpowers:writing-plans` 生成，预计任务划分：

1. Data model — Rust enum + TS interface + yaml serde
2. Method 路由 + 页面骨架
3. LmtSidebar 重写（项目内 + 首页两态 + method-driven 分组）
4. Home view sidebar 内容（recent projects 子组件复用）
5. Design 页 method-pending banner
6. 删 Export.vue / 路由 / i18n
7. Method 切换 confirm dialog
8. i18n 全量补全（en + zh）
9. Tests（unit + manual checklist）
