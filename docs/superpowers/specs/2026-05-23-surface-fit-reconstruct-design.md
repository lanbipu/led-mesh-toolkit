# Surface-Fit Reconstruct — 设计文档

- 日期：2026-05-23（2026-05-24 经 Codex adversarial review 修订）
- 分支：`worktree-feat+surface-fit-reconstruct`（基线已 rebase 到本地 main `0970689`，含 lmt CLI 重构）
- 状态：设计已逐节确认 + adversarial review 修订，待 spec 复审 → writing-plans

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

仅支持**形状参数化可拟合**的屏：平面、圆柱面。完全不规则的自由曲面屏不在范围内，仍需
密集网格测量。本功能是"扩大适用范围"，不是"取代网格测量"。

## 2. 已确认的设计决策

| # | 决策点 | 选定方案 |
|---|---|---|
| 1 | 形状范围 | 平面 + 圆柱面（球面/自由曲面不做）|
| 2 | 散点入口 | 复用 `import → reconstruct` 两步；import 加散点模式跳过网格命名 |
| 3 | 网格定位 | inlier 投影取覆盖范围当屏边界，**且与 cabinet 物理尺寸做一致性校验**（见 §9）|
| 4 | 屏面点筛选 | 全用上 + robust fitting（RANSAC）自动剔除离群杂点 |
| 5 | 代码落地 | measured.yaml 加 `sampling_mode`，reconstruct 顶层显式分流 |
| 6 | 朝向控制 | 默认拟合几何自动导出 frame；可选 frame hints 覆盖；全程写入 report（见 §9）|
| 7 | 点身份 | scatter 点保留"行号+原始 label"稳定唯一 id，供 outlier 追溯（见 §5）|

## 3. 端到端数据流

```
散点 CSV（可含杂点 CD/A/BZ）
  │ lmt total-station import --mode scatter [--columns x=3,y=4,z=5]
  │   ① 宽松解析：取 (x,y,z) + 生成稳定唯一 id（行号+原始 label）；不建 SOP 坐标系、不做网格命名
  ▼
measured.yaml  (sampling_mode: scatter, points = 裸坐标 + id, 无 V_R 编号)
  │ lmt reconstruct surface [--frame-hints ...]
  │   ② reconstruct 顶层按 sampling_mode 分流
  ▼  ─ grid    → 现状四级序列（direct_link…nominal）  ← 不动
     └ scatter → 新模块 surface_fit
core::reconstruct::surface_fit
  ③ robust 拟合（RANSAC）：依 shape_prior 选 平面/圆柱；杂点落为 outlier（记 id+行+残差）
  ④ inlier 投影到曲面参数空间（圆柱: 角度θ + 高度h；平面: u,v）
  ⑤ 取 θ/h 覆盖范围当屏边界 → **与 cabinet_count×cabinet_size_mm 期望尺寸一致性校验**
  ⑥ 按 cabinet (cols+1)×(rows+1) 均匀撒网格，映回 3D；导出 M0.1 IR 坐标系
  ⑦ frame/unwrap/origin/边界校验结果全写入 report；朝向不确定时 warning
  ▼
ReconstructedSurface（标准结构，填 shape_fit_rms_mm + outliers + ScatterFitInfo）
  │ lmt export obj <run_id> <target>   ← 完全复用，零改动
  ▼
OBJ（disguise / unreal / neutral）
```

## 4. 核心组件：`surface_fit` 模块

新增 `crates/core/src/reconstruct/surface_fit/`，纯几何、无 IO，五个小单元各管一件事：

| 单元 | 职责 | 输入 → 输出 |
|---|---|---|
| `fit.rs` | RANSAC 鲁棒拟合 `fit_plane` / `fit_cylinder`：迭代取最小子集拟合候选曲面，按点到面距离阈值统计 inlier，取 inlier 最多者，再用全体 inlier 最小二乘精修 | 散点 + shape_prior → 曲面参数 + inlier/outlier 划分 |
| `project.rs` | inlier 投影到曲面参数空间，定屏边界范围。圆柱：θ=绕轴角、h=沿轴高；平面：平面内两正交基 u,v | 曲面 + inlier → (θ,h) 或 (u,v) 的 min/max 范围 |
| `boundary.rs` | **边界一致性校验**：投影范围导出的弧长/高 vs `cabinet_count×cabinet_size_mm` 期望值，偏差分级 → ok / warning / reject（见 §9）| 范围 + cabinet_array → 校验结论 + 偏差量 |
| `resample.rs` | 在校验过的参数范围内按 cabinet 列×行均匀撒 (cols+1)×(rows+1) 网格点，映回 3D 模型坐标 | 范围 + cabinet_array → `Vec<Vector3>` |
| `frame.rs` | 导出 M0.1 IR 坐标系（圆柱轴→+Z 行向上、周向→+X 列、径向朝外→+Y 法向；平面→法向 +Y）；接收可选 frame hints 覆盖 sign/origin/unwrap，并产出可序列化的 `FrameDerivation` 记录 | 曲面 + 可选 hints → CoordinateFrame + FrameDerivation |

入口 `surface_fit::reconstruct(points, frame_hints) -> Result<ReconstructedSurface>`，
实现 `Reconstructor` trait 保持接口统一，但**由 reconstruct 顶层按 `sampling_mode` 显式
调用，不进 `auto_reconstruct` 序列**。

### 拟合数学要点

- **平面**：RANSAC 取 3 点定候选平面（法向 n、距离 d），inlier = 点到平面距离 < 阈值；
  全体 inlier 用 PCA / 最小二乘精修法向。
- **圆柱**：拟合 轴向 a、轴上一点 c、半径 r。**轴向估计是核心难点**（一条近平面的弧线
  做 PCA 会估出弧的展开方向而非圆柱轴，不可靠），实现阶段二选一：(a) 专用圆柱 RANSAC，
  取子集联立解轴向+半径；(b) 利用"屏面竖直"先验固定轴向近竖直，再在垂直于轴的平面内做
  圆拟合（Kåsa 代数法）。inlier = |点到轴距离 − r| < 阈值，全体 inlier 精修。崩铁数据
  已用"固定竖直轴 + Kåsa"验证可行（残差 0.14%）——但通用性靠 (a)，需在实现阶段评测。

## 5. 数据模型变更

| 改动 | 位置 | 说明 |
|---|---|---|
| 加 `sampling_mode: SamplingMode`（`Grid`\|`Scatter`）| `core` `MeasuredPoints` | serde `#[serde(default)]` = `Grid`，旧 measured.yaml 无此字段自动当 Grid，**向后兼容** |
| scatter 点带**稳定唯一 id** | `MeasuredPoint.name` | scatter 模式 name 存 `行号 + 原始 CSV label`（如 `row6_LEDB-1`），**不留空、拒绝重复**。不用于网格匹配，仅供 outlier 追溯（修订自 Codex Finding 4）|
| scatter import 不建 SOP 坐标系 | `coordinate_frame` | 存 identity；真实坐标系由 `surface_fit::frame` 拟合后算 |
| scatter import 报告简化 | import 输出 | 只报"存入 N 个散点"；inlier/outlier 划分在 reconstruct 拟合阶段产生 |

`SamplingMode` 作为 core 域类型；被 measured.yaml DTO 引用的部分进 `schema::dump_all()`
的 `incomplete` 列表并注明"引用 core 域类型"。

## 6. DTO / schemars（`lmt-shared`）

- 新增 `ScatterFitInfo`：拟合形状（plane/cylinder）、半径 or 法向、inlier_count、
  outlier 明细列表（每条含 `point_id`、`source_row`、`coordinates`、`residual_mm`）、
  参数覆盖范围（θ/h 或 u/v）、边界校验结论、`FrameDerivation`（导出的轴/origin/unwrap）。
  派生 `Serialize + Deserialize + JsonSchema`，加进 `schema::dump_all()`。
- 拟合细节随 run 的 `report.json` 落盘；`quality_metrics` 填
  `method = surface_fit_cylinder | surface_fit_plane`、`shape_fit_rms_mm`、`outliers`。

## 7. CLI 六件套契约（项目硬要求）

| # | 交付项 | 内容 |
|---|---|---|
| ① | lmt-app helper | `total_station.rs` import 加 scatter 分支；`reconstruct.rs` 顶层按 `sampling_mode` 分流到 `surface_fit`，透传 frame hints |
| ② | Tauri shim | `src-tauri/src/commands/total_station.rs` import command 加 `mode`/`columns` 参数；reconstruct command 加 `frame_hints`（thin wrapper，业务在 lmt-app）；GUI 的 UI 入口后续接，shim 先就位 |
| ③ | CLI 子命令 | `lmt total-station import --mode scatter [--columns x=3,y=4,z=5]`；`lmt reconstruct surface [--frame-hints origin=...,up=...,x=...]`（不传则自动导出）|
| ④ | CLI E2E | happy（scatter import→reconstruct→export 出 OBJ）/ refuse（无 --yes）/ dry-run / error envelope（拟合失败 / 边界校验拒绝）|
| ⑤ | docs/agents-cli.md | import 行补 `--mode`/`--columns`、reconstruct 行补 `--frame-hints`；如新增错误码补对照表 |
| ⑥ | DTO schemars | `ScatterFitInfo`、`SamplingMode`、`FrameDerivation` 进 schema dump |

## 8. 错误处理

- **优先复用**现有 `reconstruction` 错误码，message 说明拟合失败原因（inlier 比例太低 /
  数据不成形 / shape_prior 与数据不符 / 边界与物理尺寸严重不符），避免动错误码三处契约。
- 仅当语义确需区分时才新增 `surface_fit_failed`，那就三处同步（`error_codes` 常量 +
  `exit_codes` + docs 错误码表）。第一版倾向不新增。
- import 阶段：重复或空的 point id → `invalid_input` 拒绝。

## 9. 边界与朝向安全机制（应对 Codex Finding 2 / 3）

纯靠 inlier 投影 min/max 框定边界，有两个静默失效模式，且产出的错 OBJ 能通过 vertex
count / finite validation——下游难发现。两道防线：

### 9.1 边界一致性校验（Finding 2）

`boundary.rs` 把投影范围导出的物理尺寸（圆柱：弧长 = R×Δθ、高 = Δh；平面：Δu×Δv）跟
`cabinet_count × cabinet_size_mm` 的期望尺寸对比：

- 偏差 ≤ ±1 cabinet（或 ±2%，取大者）：ok。
- 偏差中等：产出 + warning，report 记两组尺寸。
- 偏差 > 阈值（暂定 ±10% 或 ±2 cabinet，实现阶段调）：reject，报错提示"测量未覆盖到屏边缘"
  或"混入屏外同曲面点"。

覆盖两个失效模式：① 同曲面屏外点撑大范围；② 缺失边缘点缩小范围。两者都会让投影范围偏离
cabinet 物理总尺寸而被抓住。

### 9.2 朝向可控 + 可追溯（Finding 3）

scatter 默认由拟合几何自动导出 frame，但：

- **全程记录**：导出的 `CoordinateFrame`、cylinder unwrap 方向、origin corner 选择写入
  report 的 `FrameDerivation`，下游可核对。
- **可选 frame hints**：`--frame-hints` 传 origin / up / x-direction / outward，覆盖自动
  结果的 sign / origin / unwrap 歧义。
- **无 hints 时**：产出但带 `orientation-uncertain` warning；可配置为仅允许 neutral
  target 导出（debug 用），disguise/unreal 需确认朝向后再导。

## 10. 拍板细节（实现时遵循）

- **坐标系自动导出**：grid 模式靠 CSV 前 3 个 SOP 点；scatter 模式无此约定，坐标系由拟合
  曲面几何自动导出，朝向歧义由 §9.2 的记录 + hints 兜底。
- **CSV 列映射**：scatter import 默认按"末尾 3 个数值列 = xyz"猜；猜不准用
  `--columns x=3,y=4,z=5`（1-based）显式指定。兼容用户真实的 `name,空,x,y,z` 非标格式。

## 11. 测试策略

- **core 单元测试**：合成已知半径圆柱 / 平面散点 + 注入离群点 → 验证 RANSAC 恢复
  半径·法向、正确剔除离群、重采样顶点数 = (cols+1)×(rows+1)、坐标系符合 M0.1 IR。
- **拒绝 / 边界测试（应对 Finding 2/3/4）**：
  - partial contour（缺边缘点）→ 边界校验应 warning/reject。
  - same-surface off-screen point（屏外同曲面点）→ 边界校验应抓出。
  - frame sign flip → `FrameDerivation` 记录正确、hints 能覆盖。
  - empty / duplicate labels → import 拒绝。
- **真实数据回归**：用崩铁 CSV 做 fixture，断言拟合出 R≈9523mm、张角≈165°、网格合理，
  且边界校验与 cabinet 物理尺寸自洽（呼应 27480 vs 55×500 的交叉验证）。
- **CLI E2E**：§7④ 四类。
- 合并前自检：`cargo test --workspace`、`lmt --json schema | jq`（新 DTO 进 dump）、
  `lmt total-station import --help`、`lmt reconstruct surface --help`（新参数注册）。

## 12. 文件清单（改 / 新增）

新增：
- `crates/core/src/reconstruct/surface_fit/{mod,fit,project,boundary,resample,frame}.rs`
- `crates/lmt-cli/tests/cli_e2e.rs` 的 scatter case
- core 测试 fixture（合成 + 崩铁真实 CSV）

修改：
- `crates/core/src/measured_points.rs`（`sampling_mode` + scatter 点 id）
- `crates/core/src/shape.rs` 或新增（`SamplingMode` enum）
- `crates/core/src/reconstruct/mod.rs`（如需暴露 surface_fit 入口）
- `crates/lmt-app/src/reconstruct.rs`（顶层分流 + frame hints 透传）
- `crates/lmt-app/src/total_station.rs`（scatter import 分支 + id 生成）
- `crates/adapter-total-station/src/csv_parser.rs`（宽松解析 + 列映射，或新增 scatter parser）
- `crates/lmt-shared/src/dto.rs`（`ScatterFitInfo`、`SamplingMode`、`FrameDerivation`）
- `crates/lmt-shared/src/data/schema.rs`（`dump_all`）
- `crates/lmt-cli/src/cli.rs`（`--mode`/`--columns`/`--frame-hints`）
- `crates/lmt-cli/src/commands/total_station.rs`、`crates/lmt-cli/src/commands/reconstruct.rs`
- `src-tauri/src/commands/total_station.rs`、`src-tauri/src/commands/reconstruct.rs`
- `docs/agents-cli.md`

## 13. 非目标（YAGNI）

- 球面 / 自由曲面拟合。
- scatter 模式的 GUI UI 入口（Tauri shim 就位，UI 后续）。
- scatter 模式的现场指令卡（指令卡服务于 grid SOP）。
- 多曲面 / 拼接屏的自动分段拟合。

## 14. 成功标准

1. 用崩铁 CSV：`import --mode scatter` → `reconstruct surface` → `export obj` 三步出 OBJ，
   拟合 R≈9523mm、张角≈165°，`shape_fit_rms` 小，杂点（CD/A/BZ）被剔除为 outlier 且
   report 能定位其行号。
2. 边界校验：投影范围与 cabinet 物理尺寸自洽；构造缺边缘 / 屏外点的 fixture 能被抓出。
3. `FrameDerivation` 写入 report；frame hints 能覆盖自动朝向。
4. CLI 四类 E2E 全过。
5. `cargo test --workspace` 全过。
6. `lmt --json schema` 含 `ScatterFitInfo` / `SamplingMode` / `FrameDerivation`。
7. 旧 grid 流程（现状四级序列）行为不变，回归测试不破。

## 15. 对抗性 review 的回应（Codex，2026-05-24）

| Finding | 处理 |
|---|---|
| [high] 引用不存在的 crates（lmt-app/lmt-cli/lmt-shared）| **非 spec 错误**：worktree 误建在过时的 origin/main（落后整个 lmt CLI 重构）。已 rebase 到本地 main，13 个引用路径全部核实存在。交付面引用不变。|
| [high] inlier min/max 边界静默产出错误几何 | 采纳：新增 §9.1 边界一致性校验（投影范围 vs cabinet 物理尺寸）+ 拒绝测试。|
| [high] 自动导出 frame 朝向不可控 | 采纳：新增 §9.2 frame hints + `FrameDerivation` 全程记录 + 无 hints 时 warning/限制。|
| [medium] 忽略 label 致 outlier 身份丢失 | 采纳：§5 scatter 点保留"行号+原始 label"稳定唯一 id；§6 outlier 明细带行号/坐标/残差。|
