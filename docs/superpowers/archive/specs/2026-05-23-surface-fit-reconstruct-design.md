# Surface-Fit Reconstruct — 设计文档

- 日期：2026-05-23（2026-05-24 经两轮 review 修订：Codex adversarial + code-review max）
- 分支：`worktree-feat+surface-fit-reconstruct`（基线已 rebase 到本地 main `0970689`，含 lmt CLI 重构）
- 状态：设计已逐节确认 + 两轮 review 修订，待 spec 复审 → writing-plans

## 1. 背景与动机

现状重建管线（`core::reconstruct` 的 `direct_link → radial_basis → boundary_interp →
nominal` 四级序列）有一个共同前提：**每个测量点必须带 `<screen>_V<col>_R<row>` 网格编号**。
编号在 import 阶段由 `geometric_naming` 用最近邻匹配理论网格点（±50mm）赋予。它处理的是
"按指令卡 SOP 在网格点采样、点测得不全或有偏差"的场景。

但现场常拿到**非网格采样**数据：沿屏面轮廓线密集采点、加若干特征/边界点，点不对应任何
网格顶点编号。这类数据连命名那一关都过不去，四个方法谁都接不住——尽管数据本身的几何
信息完全够（实测案例：57 点圆弧拟合残差 0.14%，曲面完全确定）。

本功能新增一条**曲面拟合 + 网格重采样**范式：用散点 robust 拟合出参数化曲面（平面 /
圆柱面），在曲面上按 cabinet 网格重新生成规则顶点，产出与现状一致的
`ReconstructedSurface`，下游 export 零改动复用。

### 能力边界（明确写出，避免误用）

- 仅支持**形状参数化可拟合**的屏：平面、圆柱面。自由曲面屏不在范围内，仍需密集网格测量。
- **scatter 模式仍需要完整 `project.yaml`**：`cabinet_array`（cols/rows/cabinet_size_mm）与
  `shape_prior` 跟 grid 模式一样从 project.yaml 读（见 §5）。本功能省掉的是"按网格点逐点测量"，
  不是"省掉屏体配置"。光有一个 CSV 不够。

## 2. 已确认的设计决策

| # | 决策点 | 选定方案 |
|---|---|---|
| 1 | 形状范围 | 平面 + 圆柱面（球面/自由曲面不做）|
| 2 | 散点入口 | 复用 `import → reconstruct` 两步；import 加 scatter 模式，走**独立路径**跳过 SOP 坐标系校验与网格命名 |
| 3 | 网格定位 | inlier 投影取覆盖范围当屏边界，**且与 `cabinet_array.total_size_mm()` 做单位换算后的一致性校验**（见 §9.1）|
| 4 | 屏面点筛选 | 全用上 + robust fitting（RANSAC）自动剔除离群杂点 |
| 5 | 代码落地 | measured.yaml 加 `sampling_mode`，reconstruct 顶层显式分流（不进 auto 序列）|
| 6 | 朝向控制 | 第一版：拟合几何**自动导出 frame** + `FrameDerivation` 写进 report + 朝向不确定 warning。frame hints + 强制 export gate **推迟到后续**（见 §9.2、§13）|
| 7 | 点身份 | scatter 点保留"行号+原始 label"稳定唯一 id，供 outlier 追溯（见 §5）|
| 8 | 圆柱拟合策略 | 第一版默认"**固定竖直轴 + 投影平面内 RANSAC 圆拟合**"（已验证可行）；通用轴向 RANSAC 留 backlog（见 §4）|
| 9 | 错误码 | **新增 `surface_fit_failed`**，三处同步（现状无 `reconstruction` 码，见 §8）|

## 3. 端到端数据流

```
散点 CSV（可含杂点 CD/A/BZ）+ 已配好的 project.yaml（cabinet_array + shape_prior）
  │ lmt total-station import --mode scatter [--columns x=3,y=4,z=5]
  │   scatter 独立路径：① 新 scatter CSV parser 取 (x,y,z) + 生成稳定 id（行号+label）
  │                     ② 从 project.yaml 读 cabinet_array + shape_prior（同 grid）
  │                     ③ 不建 SOP 坐标系、不做网格命名、不经 map_to_adapter 的 V_R 校验
  ▼
measured.yaml  (sampling_mode: scatter, points = 裸坐标 + 字符串 id, 无 V_R 编号, coordinate_frame=identity)
  │ lmt reconstruct surface
  │   reconstruct 顶层（lmt-app/reconstruct.rs）按 sampling_mode 分流
  ▼  ─ grid    → 现状 auto_reconstruct 四级序列（direct_link…nominal）  ← 不动
     └ scatter → SurfaceFitReconstructor（不进 auto 序列）
core::reconstruct::surface_fit
  ③ robust 拟合（RANSAC）：依 shape_prior 选 平面/圆柱；杂点落为 outlier（记 id+行+坐标+残差）
  ④ inlier 投影到曲面参数空间（圆柱: 角度θ + 高度h；平面: u,v）
  ⑤ 取覆盖范围当屏边界 → 与 cabinet_array.total_size_mm() 单位换算后一致性校验（§9.1）
  ⑥ 按 (cols+1)×(rows+1) 均匀撒网格，映回 3D；uv 用 compute_grid_uv(topology)
  ⑦ 自动导出 M0.1 IR 坐标系；FrameDerivation + 边界校验 + outlier 明细写进 ScatterFitInfo
  ▼
ReconstructedSurface（标准结构）+ ReconstructionReport.scatter_fit = Some(ScatterFitInfo)
  │ lmt export obj <run_id> <target>   ← 完全复用，零改动
  ▼
OBJ（disguise / unreal / neutral）
```

## 4. 核心组件：`surface_fit` 模块

新增 `crates/core/src/reconstruct/surface_fit/`，纯几何、无 IO。`SurfaceFitReconstructor`
是 **unit struct**（同 `DirectLinkReconstructor`），`impl Reconstructor`，方法签名
`reconstruct(&self, points: &MeasuredPoints) -> Result<ReconstructedSurface, CoreError>`
**与 trait 完全一致**（frame hints 第一版不做，不引入第二参数）。拟合 + frame 导出全在
方法内部自动完成。模块内五个小单元：

| 单元 | 职责 |
|---|---|
| `fit.rs` | RANSAC 鲁棒拟合 `fit_plane` / `fit_cylinder`，返回曲面参数 + inlier/outlier 划分（带每点残差）|
| `project.rs` | inlier 投影到曲面参数空间。圆柱：θ=绕轴角、h=沿轴高；平面：见下方平面定向 |
| `boundary.rs` | 边界一致性校验：投影范围导出尺寸（米）×1000 vs `cabinet_array.total_size_mm()`（mm），分级 ok/warning/reject（§9.1）|
| `resample.rs` | 在校验过的参数范围内按 (cols+1)×(rows+1) 撒网格、映回 3D；uv 调 `crate::uv::compute_grid_uv(topology)` |
| `frame.rs` | 导出 M0.1 IR 坐标系，产出可序列化 `FrameDerivation`（轴/origin/unwrap 方向）|

reconstruct 顶层分流：`crates/lmt-app/src/reconstruct.rs` 在调 `auto_reconstruct` 前判
`measured.sampling_mode`，`Scatter` 走 `SurfaceFitReconstructor.reconstruct(&points)`。

### 拟合数学要点

- **平面**：RANSAC 取 3 点定候选平面（法向 n、距离 d），inlier = 点到平面距离 < 阈值；
  全体 inlier PCA 精修法向。
  - **平面定向（消歧，应对 code-review）**：平面内 u/v 基不能任取，否则网格会旋转/镜像。
    定向规则：origin 取投影 (u,v) 范围中对应 cabinet (col=0,row=0) 的角；u 基取使
    Δu : Δv 最接近 `cols : rows`（按 cabinet 物理长宽比）的方向，v 基由 u 与法向叉乘定，
    符号使 +Z 朝上、构成右手系。
- **圆柱（决策 8 拍板默认）**：固定轴向近竖直（LED 屏常态），把 inlier 投影到水平面，在
  其中跑 **RANSAC 圆拟合**（候选圆用 3 点 Kåsa 解，inlier = |点到圆心距 − r| < 阈值），
  全体 inlier 精修圆心/半径；轴向高度直接取 z。崩铁数据验证可行（残差 0.14%）。
  通用任意轴向的圆柱 RANSAC 留 backlog（§13）。
- **inlier 距离阈值**：默认 50mm（与几何命名同量级），常量可调。

## 5. 数据模型变更

| 改动 | 位置 | 说明 |
|---|---|---|
| 加 `sampling_mode: SamplingMode`（`Grid`\|`Scatter`）| `core` `MeasuredPoints` | serde `#[serde(default)]` = `Grid`，旧 measured.yaml 向后兼容（`MeasuredPoints` 是普通 derive Deserialize，可加）|
| scatter 点带**稳定唯一 id** | `MeasuredPoint.name` | 存 `行号 + 原始 CSV label`（如 `row6_LEDB-1`），不留空、拒绝重复。仅供 outlier 追溯，不用于网格匹配 |
| scatter import 走**独立路径** | `lmt-app/total_station.rs` | 不经 `map_to_adapter`（它会校验 `coordinate_system.{origin,x_axis,xy_plane}_point` 为 V_R 格式）；scatter 模式 `coordinate_system` 块可缺省/忽略 |
| `cabinet_array` + `shape_prior` 来源 | project.yaml | **与 grid 一样从 project.yaml 读**，不是从 CSV 推。scatter import 仍要求项目配好这两项 |
| `coordinate_frame` = identity | scatter measured.yaml | 真实坐标系由 `surface_fit::frame` 拟合后算；identity frame 通过 `CoordinateFrame` 的 basis 校验 |
| `ReconstructionReport` 加 `scatter_fit: Option<ScatterFitInfo>` | `lmt-shared/dto.rs` | **列为交付项**：grid 模式为 `None`，scatter 模式 `Some(...)`。`ReconstructionReport` 已在 schema `incomplete` 列表，加字段不改变这点 |
| import 报告**复用** `TotalStationImportResult` | dto.rs | scatter 模式：`measured_count` = 存入点数，`fabricated_count`/`outlier_count`/`missing_count` = 0（这些 grid 概念在 import 阶段不适用，outlier 在 reconstruct 阶段算并进 ScatterFitInfo）；`warnings` 带 scatter 说明。不新增 import DTO |

## 6. DTO / schemars

- 新增 `lmt-shared` 的 `ScatterFitInfo`（纯标量/数组，**坐标用 `[f64;3]` 不用 `nalgebra::Vector3`**，
  因 lmt-shared 不依赖 nalgebra 且 schemars 0.8 无 Vector3 impl）：
  - `shape`: `"plane"` \| `"cylinder"`
  - `radius_mm: Option<f64>`（圆柱）/ `plane_normal: Option<[f64;3]>`（平面）
  - `inlier_count: usize`
  - `outliers: Vec<ScatterOutlier>`，`ScatterOutlier { point_id: String, source_row: usize, coordinates: [f64;3], residual_mm: f64 }`
  - `param_range`: 覆盖范围（θ/h 或 u/v 的 min/max）
  - `boundary_check`: 校验结论 + 投影尺寸 vs 期望尺寸（都换算成 mm）
  - `frame_derivation: FrameDerivation { axis: [f64;3], origin: [f64;3], unwrap_dir: String }`
- `ScatterFitInfo`、`FrameDerivation`、`ScatterOutlier`、`SamplingMode` 派生
  `Serialize + Deserialize + JsonSchema`，在 **`crates/lmt-shared/src/schema.rs`** 的
  `dump_all()` `add!()` 宏块注册（**不是 data/schema.rs，那是 DB migration**）。
- `quality_metrics.outliers`（`Vec<String>`，core 既有字段）仅放 outlier 的 id 列表（字符串）；
  **结构化明细放 `ScatterFitInfo.outliers`**，避免把结构体硬塞 `Vec<String>`。

## 7. CLI 六件套契约

| # | 交付项 | 内容 |
|---|---|---|
| ① | lmt-app helper | `total_station.rs` import 加 scatter 独立分支（新 parser + 从 project.yaml 取 cabinet/shape + 生成 id）；`reconstruct.rs` 顶层按 `sampling_mode` 分流到 `SurfaceFitReconstructor` |
| ② | Tauri shim | `src-tauri/src/commands/total_station.rs` import command 加 `mode`/`columns` 参数（thin wrapper）；reconstruct command 无新参数；GUI UI 后续接 |
| ③ | CLI 子命令 | `lmt total-station import --mode scatter [--columns x=3,y=4,z=5]`；`lmt reconstruct surface` 不变（自动分流）。**无 `--frame-hints`**（推迟，§13）|
| ④ | CLI E2E | happy（scatter import→reconstruct→export 出 OBJ）/ refuse（无 --yes）/ dry-run / error envelope（`surface_fit_failed`：inlier 太少 / 边界 reject）|
| ⑤ | docs/agents-cli.md | import 行补 `--mode`/`--columns`；错误码表加 `surface_fit_failed` |
| ⑥ | DTO schemars | `ScatterFitInfo`/`ScatterOutlier`/`FrameDerivation`/`SamplingMode` 进 `schema.rs` 的 dump_all |

## 8. 错误处理

现状事实（已核对）：`error_codes`（envelope.rs:96-117）**没有 `reconstruction` 码**；
`CoreError::Reconstruction` → `LmtError::Core` → `invalid_input`（envelope.rs:125）。

- **新增 `surface_fit_failed` 错误码，三处同步**（envelope.rs 注释明确要求）：
  ① `error_codes::SURFACE_FIT_FAILED` 常量；② `exit_codes` 加退出码映射；
  ③ `docs/agents-cli.md` 错误码表。并在 lmt-app 层把 surface_fit 的拟合失败/边界 reject
  映射到此码（与普通 `invalid_input` 区分，让 agent 能按 exit code 辨别"拟合数学失败"
  vs "传错参数"）。
- import 阶段：重复或空的 point id → `invalid_input` 拒绝。
- 触发 `surface_fit_failed` 的情形：inlier 比例低于阈值（暂定 50%）、边界校验 reject、
  RANSAC 无法收敛。

## 9. 边界与朝向安全机制

### 9.1 边界一致性校验（含单位换算，应对 code-review 单位 bug）

`boundary.rs` 把投影范围导出的物理尺寸**换算成 mm**后与 `cabinet_array.total_size_mm()`
对比（顶点是米，`total_size_mm()` 是 mm，必须 ×1000）：

- 圆柱：投影宽 = `R_m × Δθ × 1000` mm，高 = `Δh_m × 1000` mm。
- 平面：投影 `Δu_m × 1000`、`Δv_m × 1000` mm。
- 期望：`total_size_mm() = [cabinet_size_mm[0]×cols, cabinet_size_mm[1]×rows]`。
- 偏差 ≤ ±1 cabinet（或 ±2%，取大）：ok；中等：warning + report 记两组尺寸；
  > ±10% 或 ±2 cabinet（暂定，可调）：reject（`surface_fit_failed`），提示"未覆盖屏边缘"
  或"混入屏外同曲面点"。

覆盖两个失效模式：① 同曲面屏外点撑大范围；② 缺失边缘点缩小范围。平面同样校验（用上面
平面投影尺寸），配合 §4 的平面定向规则防止旋转/镜像。

### 9.2 朝向可追溯（第一版）+ hints（后续）

- **第一版**：拟合几何自动导出 frame；`FrameDerivation`（轴/origin/unwrap 方向）写进
  `ScatterFitInfo` → report，下游可核对；导出带 `orientation-uncertain` warning（在 CLI
  human 输出与 report warnings 里）。
- **推迟到后续**（§13）：`--frame-hints` 显式覆盖、以及"仅允许 neutral target 导出"的
  强制 gate。这两个需要定义 CLI hint 格式 + 在 surface/report 加标志位 + export 层加拦截，
  第一版不做；朝向可控性靠"记录 + warning + 用户看 report 核对"满足。

## 10. 拍板细节（实现时遵循）

- **scatter CSV parser（新增，不复用 `parse_csv`）**：现状 `parse_csv` 要表头且 `name`
  必须解析为非零 `u32`，拒绝 `LEDB-1` 这类标签。scatter 用独立 parser：无表头要求；按
  `--columns x=C,y=C,z=C`（1-based 列号，默认"末尾 3 个数值列"）取坐标；可选 label 列
  原样留存；point id = `row{行号}_{label或空}`，保证唯一。产出直接是带 string id 的散点
  集（不经 `RawPoint` 的 u32 instrument_id 体系）。
- **scatter 模式 `coordinate_system`**：project.yaml 的 `coordinate_system` 块在 scatter
  模式可缺省；scatter import 不读它、不校验 V_R 格式。
- **坐标系自动导出**：见 §9.2；朝向歧义由 `FrameDerivation` 记录 + warning 兜底。

## 11. 测试策略

- **core 单元测试**：合成已知半径圆柱 / 平面散点 + 注入离群点 → 验证 RANSAC 恢复
  半径·法向、剔除离群、顶点数 = (cols+1)×(rows+1)、坐标系符合 M0.1 IR、uv 与 grid 一致。
- **回归 / 拒绝测试（应对两轮 review）**：
  - 单位换算：构造米级真实屏，断言边界校验**不会**因 mm/m 混用误判 reject。
  - 平面定向：旋转/镜像输入 → 网格行列与 cabinet 对齐，不产出转 90°/镜像的屏。
  - partial contour（缺边缘）/ same-surface off-screen point → 边界校验 warning/reject。
  - scatter parser：字符串 label（`LEDB-1`）能解析、空/重复 id 被拒。
  - 错误码：拟合失败走 `surface_fit_failed` 且 exit code 与 `invalid_input` 不同。
- **真实数据回归**：崩铁 CSV 做 fixture，断言 R≈9523mm、张角≈165°、边界校验与 cabinet
  物理尺寸自洽（27480 vs 55×500）。fixture 见 §12 隐私说明。
- **CLI E2E**：§7④ 四类。
- 合并前自检：`cargo test --workspace`、`lmt --json schema | jq`（新 DTO 进 dump）、
  `lmt total-station import --help`。

## 12. 文件清单（改 / 新增）

新增：
- `crates/core/src/reconstruct/surface_fit/{mod,fit,project,boundary,resample,frame}.rs`
- scatter CSV parser（`crates/adapter-total-station/src/` 新模块，或 lmt-app 内）
- `crates/lmt-cli/tests/cli_e2e.rs` 的 scatter case
- core 测试 fixture（合成 + 崩铁脱敏 CSV）

修改：
- `crates/core/src/measured_points.rs`（`sampling_mode`）
- `crates/core/src/shape.rs` 或新增（`SamplingMode` enum）
- `crates/core/src/reconstruct/mod.rs`（导出 surface_fit）
- `crates/lmt-app/src/reconstruct.rs`（顶层按 sampling_mode 分流）
- `crates/lmt-app/src/total_station.rs`（scatter 独立 import 分支 + id 生成）
- `crates/lmt-shared/src/dto.rs`（`ScatterFitInfo`/`ScatterOutlier`/`FrameDerivation`；
  `ReconstructionReport` 加 `scatter_fit` 字段；`SamplingMode` 暴露）
- **`crates/lmt-shared/src/schema.rs`**（dump_all 的 add! 宏 —— 不是 data/schema.rs）
- `crates/lmt-shared/src/envelope.rs`（`error_codes::SURFACE_FIT_FAILED`）
- `crates/lmt-shared/src/exit_codes.rs`（退出码映射）
- `crates/lmt-cli/src/cli.rs`（import `--mode`/`--columns`）
- `crates/lmt-cli/src/commands/total_station.rs`
- `src-tauri/src/commands/total_station.rs`（import shim 加参数）
- `docs/agents-cli.md`（import 参数 + `surface_fit_failed` 错误码表）

## 13. 非目标（YAGNI / 后续）

- 球面 / 自由曲面拟合。
- 通用任意轴向的圆柱 RANSAC（第一版固定竖直轴）。
- `--frame-hints` 显式朝向覆盖 + "仅 neutral 导出"的强制 export gate（第一版靠记录+warning）。
- scatter 模式的 GUI UI 入口（Tauri shim 就位，UI 后续）。
- scatter 模式的现场指令卡（指令卡服务于 grid SOP）。
- 多曲面 / 拼接屏的自动分段拟合。

## 14. 成功标准

1. 用崩铁 CSV（+ 配好 project.yaml）：`import --mode scatter` → `reconstruct surface` →
   `export obj` 三步出 OBJ，拟合 R≈9523mm、张角≈165°，杂点（CD/A/BZ）被剔除为 outlier，
   report 的 `scatter_fit.outliers` 能定位其行号/坐标/残差。
2. 边界校验：单位换算正确（米级屏不误判）；缺边缘 / 屏外点 fixture 被抓出。
3. 平面定向：旋转/镜像输入不产出错位网格。
4. `surface_fit_failed` 错误码三处同步，拟合失败 exit code 与 `invalid_input` 不同。
5. `FrameDerivation` 写入 report；朝向不确定有 warning。
6. CLI 四类 E2E 全过；`cargo test --workspace` 全过。
7. `lmt --json schema` 含 `ScatterFitInfo`/`SamplingMode`/`FrameDerivation`。
8. 旧 grid 流程行为不变，回归测试不破。

## 15. Codex adversarial review 回应（2026-05-24）

| Finding | 处理 |
|---|---|
| [high] 引用不存在的 crates | 非 spec 错误：worktree 误建在过时 origin/main。已 rebase 到本地 main，路径全部核实存在 |
| [high] inlier min/max 边界静默错误 | 采纳：§9.1 边界一致性校验 + 拒绝测试 |
| [high] 自动 frame 朝向不可控 | 采纳（调整）：第一版 `FrameDerivation` 记录 + warning；hints/gate 推迟（§9.2、§13）|
| [medium] outlier 身份丢失 | 采纳：§5 稳定 id；§6 `ScatterFitInfo.outliers` 带行号/坐标/残差 |

## 16. code-review (max) 回应（2026-05-24，15 findings）

| # | Finding | 处理 |
|---|---|---|
| 1 | scatter import 被 `map_to_adapter` 的 V_R 校验挡死 | §2/§5：scatter 走独立 import 路径，不经 map_to_adapter，`coordinate_system` 可缺省 |
| 2 | §9.1 单位 1000× + `cabinet_count` 字段名不存在 | §9.1：明确 ×1000 换算 + 用 `cols/rows` + `total_size_mm()` |
| 3 | `surface_fit::reconstruct(points, frame_hints)` 破坏 trait 签名 | §4：`SurfaceFitReconstructor` 退回 unit struct，无第二参，签名与 trait 一致（hints 推迟）|
| 4 | `parse_csv` 拒字符串 label / 位置映射 | §10：新增独立 scatter parser，无表头、`--columns` 位置映射、string id |
| 5 | `ScatterFitInfo` 无承载字段 | §5：`ReconstructionReport` 加 `scatter_fit: Option<ScatterFitInfo>`，列入 §12 |
| 6 | `outliers: Vec<String>` 装不下明细 + Vector3 类型 | §6：结构化明细进 `ScatterFitInfo.outliers`；坐标用 `[f64;3]` |
| 7 | scatter 的 cabinet_array/shape_prior 来源未说 | §1/§5：明确从 project.yaml 读，scatter 仍需完整 project.yaml |
| 8 | `reconstruction` 错误码不存在 | §8：修正事实，新增 `surface_fit_failed` 三处同步 |
| 9 | 平面分支边界/frame/uv 未定义 | §4 平面定向规则；§9.1 平面边界校验；uv 复用 compute_grid_uv |
| 10 | orientation-uncertain 无落地载体 | §9.2：第一版降级为记录+warning，hard gate 推迟（§13）|
| 11 | 圆柱拟合两候选未拍板 | 决策 8 + §4：拍板默认竖直轴+RANSAC 圆，通用轴向留 backlog |
| 12 | schema 路径写错 data/schema.rs | §6/§12：改为 `crates/lmt-shared/src/schema.rs` |
| 13 | `--frame-hints` 格式未定义 | §13：推迟，第一版不做 |
| 14 | scatter import 报告 DTO 未定义 | §5：复用 `TotalStationImportResult`，scatter 模式 grid 字段填 0 |
| 15 | uv 生成未提 | §4：复用 `crate::uv::compute_grid_uv(topology)` |
