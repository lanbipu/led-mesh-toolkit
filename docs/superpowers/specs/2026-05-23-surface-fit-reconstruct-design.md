# Surface-Fit Reconstruct — 设计文档

- 日期：2026-05-23
- 分支：`worktree-feat+surface-fit-reconstruct`
- 状态：设计已与用户逐节确认，待 spec 复审 → writing-plans

## 1. 背景与动机

现状重建管线（`core::reconstruct` 的 `direct_link → radial_basis → boundary_interp →
nominal` 四级链）有一个共同前提：**每个测量点必须带 `<screen>_V<col>_R<row>` 网格编号**。
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
| 3 | 网格定位 | 测量点自动框定：inlier 投影到曲面取覆盖范围当屏边界 |
| 4 | 屏面点筛选 | 全用上 + robust fitting（RANSAC）自动剔除离群杂点 |
| 5 | 代码落地 | measured.yaml 加 `sampling_mode`，reconstruct 顶层显式分流 |

## 3. 端到端数据流

```
散点 CSV（可含杂点 CD/A/BZ）
  │ lmt total-station import --mode scatter [--columns x=3,y=4,z=5]
  │   ① 宽松解析：只取 (x,y,z)，name/note 忽略；不建 SOP 坐标系、不做网格命名
  ▼
measured.yaml  (sampling_mode: scatter, points = 裸坐标, 无 V_R 编号)
  │ lmt reconstruct surface
  │   ② reconstruct 顶层按 sampling_mode 分流
  ▼  ─ grid    → 现状四级链（direct_link…nominal）  ← 不动
     └ scatter → 新模块 surface_fit
core::reconstruct::surface_fit
  ③ robust 拟合（RANSAC）：依 shape_prior 选 平面/圆柱；杂点落为 outlier
  ④ inlier 投影到曲面参数空间（圆柱: 角度θ + 高度h；平面: u,v）
  ⑤ 取 θ/h 覆盖范围当屏边界，按 cabinet (cols+1)×(rows+1) 均匀撒网格
  ⑥ 网格点映回 3D，导出符合 M0.1 IR 约定的坐标系（+X=列, +Z=行向上, +Y=法向）
  ▼
ReconstructedSurface（标准结构，填 shape_fit_rms_mm + outliers）
  │ lmt export obj <run_id> <target>   ← 完全复用，零改动
  ▼
OBJ（disguise / unreal / neutral）
```

## 4. 核心组件：`surface_fit` 模块

新增 `crates/core/src/reconstruct/surface_fit/`，纯几何、无 IO，四个小单元各管一件事：

| 单元 | 职责 | 输入 → 输出 |
|---|---|---|
| `fit.rs` | RANSAC 鲁棒拟合 `fit_plane` / `fit_cylinder`：迭代取最小子集拟合候选曲面，按点到面距离阈值统计 inlier，取 inlier 最多者，再用全体 inlier 最小二乘精修 | 散点 + shape_prior → 曲面参数 + inlier/outlier 划分 |
| `project.rs` | inlier 投影到曲面参数空间，定屏边界范围。圆柱：θ=绕轴角、h=沿轴高；平面：平面内两正交基 u,v | 曲面 + inlier → (θ,h) 或 (u,v) 的 min/max 范围 |
| `resample.rs` | 在参数范围内按 cabinet 列×行均匀撒 (cols+1)×(rows+1) 网格点，映回 3D 模型坐标 | 范围 + cabinet_array → `Vec<Vector3>` |
| `frame.rs` | 从拟合曲面导出 M0.1 IR 坐标系：圆柱轴→+Z（行向上），周向→+X（列），径向朝外→+Y（法向）；平面→法向 +Y，平面内基 +X/+Z | 曲面 → CoordinateFrame |

入口 `surface_fit::reconstruct(points: &MeasuredPoints) -> Result<ReconstructedSurface>`，
实现 `Reconstructor` trait 保持接口统一，但**由 reconstruct 顶层按 `sampling_mode` 显式
调用，不进 `auto_reconstruct` 链**。

### 拟合数学要点

- **平面**：RANSAC 取 3 点定候选平面（法向 n、距离 d），inlier = 点到平面距离 < 阈值；
  全体 inlier 用 PCA / 最小二乘精修法向。
- **圆柱**：拟合 轴向 a、轴上一点 c、半径 r。**轴向估计是核心难点**（一条近平面的弧线
  做 PCA 会估出弧的展开方向而非圆柱轴，不可靠），实现阶段二选一：(a) 专用圆柱 RANSAC，
  取子集联立解轴向+半径；(b) 利用"屏面竖直"先验固定轴向近竖直，再在垂直于轴的平面内做
  圆拟合（Kåsa 代数法）。inlier = |点到轴距离 − r| < 阈值，全体 inlier 精修。崩铁数据
  已用"固定竖直轴 + Kåsa"验证可行（残差 0.14%）——但通用性靠 (a)，需在实现阶段评测。
- **inlier 距离阈值**：默认与几何命名同量级（如 50mm），可后续调参常量。

## 5. 数据模型变更

| 改动 | 位置 | 说明 |
|---|---|---|
| 加 `sampling_mode: SamplingMode`（`Grid`\|`Scatter`）| `core` `MeasuredPoints` | serde `#[serde(default)]` = `Grid`，旧 measured.yaml 无此字段自动当 Grid，**向后兼容** |
| scatter 点不带 `V_R` 编号 | `MeasuredPoint.name` | scatter 模式 name 存原始 CSV 标签或留空，不参与逻辑 |
| scatter import 不建 SOP 坐标系 | `coordinate_frame` | 存 identity；真实坐标系由 `surface_fit::frame` 拟合后算 |
| scatter import 报告简化 | import 输出 | 只报"存入 N 个散点"；inlier/outlier 划分在 reconstruct 拟合阶段产生，记入 run 的 quality_metrics |

`SamplingMode` 作为 core 域类型；被 measured.yaml DTO 引用的部分进 `schema::dump_all()`
的 `incomplete` 列表并注明"引用 core 域类型"。

## 6. DTO / schemars（`lmt-shared`）

- 新增 `ScatterFitInfo`：拟合形状（plane/cylinder）、半径 or 法向、inlier_count、
  outlier_count、参数覆盖范围（θ/h 或 u/v）。派生 `Serialize + Deserialize + JsonSchema`，
  加进 `schema::dump_all()`。
- 拟合细节随 run 的 `report.json` 落盘；`quality_metrics` 填
  `method = surface_fit_cylinder | surface_fit_plane`、`shape_fit_rms_mm`、`outliers`。

## 7. CLI 六件套契约（项目硬要求）

| # | 交付项 | 内容 |
|---|---|---|
| ① | lmt-app helper | `total_station.rs` import 加 scatter 分支；`reconstruct.rs` 顶层按 `sampling_mode` 分流到 `surface_fit` |
| ② | Tauri shim | `src-tauri` import command 加 `mode`/`columns` 参数（thin wrapper，业务在 lmt-app）；GUI 的 UI 入口后续接，shim 先就位 |
| ③ | CLI 子命令 | `lmt total-station import --mode scatter [--columns x=3,y=4,z=5]`；`reconstruct surface` 不变（自动分流）|
| ④ | CLI E2E | happy（scatter import→reconstruct→export 出 OBJ）/ refuse（无 --yes）/ dry-run / error envelope（拟合失败）|
| ⑤ | docs/agents-cli.md | import 行补 `--mode`/`--columns` 说明；如新增错误码补对照表 |
| ⑥ | DTO schemars | `ScatterFitInfo`、`SamplingMode` 进 schema dump |

## 8. 错误处理

- **优先复用**现有 `reconstruction` 错误码，message 说明拟合失败原因（inlier 比例太低 /
  数据不成形 / shape_prior 与数据不符），避免动错误码三处契约。
- 仅当语义确需区分时才新增 `surface_fit_failed`，那就三处同步（`error_codes` 常量 +
  `exit_codes` + docs 错误码表）。第一版倾向不新增。
- 质量门槛：inlier 比例低于阈值（暂定 50%）→ 拒绝并报错；高但 `shape_fit_rms` 偏大 →
  产出但 warning。

## 9. 拍板细节（实现时遵循）

- **坐标系自动导出**：grid 模式靠 CSV 前 3 个 SOP 点；scatter 模式无此约定，坐标系由拟合
  曲面几何自动导出（圆柱轴定竖直、投影范围角点定原点）。代价：模型在世界空间的绝对朝向
  由数据决定，不可控——对"从历史数据复原几何"够用。
- **CSV 列映射**：scatter import 默认按"末尾 3 个数值列 = xyz"猜；猜不准用
  `--columns x=3,y=4,z=5`（1-based）显式指定。兼容用户真实的 `name,空,x,y,z` 非标格式。

## 10. 测试策略

- **core 单元测试**：合成已知半径圆柱 / 平面散点 + 注入离群点 → 验证 RANSAC 恢复
  半径·法向、正确剔除离群、重采样顶点数 = (cols+1)×(rows+1)、坐标系符合 M0.1 IR。
- **真实数据回归**：用崩铁 CSV 做 fixture，断言拟合出 R≈9523mm、张角≈165°、网格合理。
- **CLI E2E**：上述四类。
- 合并前自检：`cargo test --workspace`、`lmt --json schema | jq`（新 DTO 进 dump）、
  `lmt total-station import --help`（新参数注册）。

## 11. 文件清单（改 / 新增）

新增：
- `crates/core/src/reconstruct/surface_fit/{mod,fit,project,resample,frame}.rs`
- `crates/lmt-cli/tests/` 的 scatter E2E case
- core 测试 fixture（合成 + 崩铁真实 CSV）

修改：
- `crates/core/src/measured_points.rs`（`sampling_mode`）
- `crates/core/src/shape.rs` 或新增（`SamplingMode` enum）
- `crates/lmt-app/src/reconstruct.rs`（顶层分流）
- `crates/lmt-app/src/total_station.rs`（scatter import 分支）
- `crates/adapter-total-station/src/csv_parser.rs`（宽松解析 + 列映射，或新增 scatter parser）
- `crates/lmt-shared/src/dto.rs`（`ScatterFitInfo`、`SamplingMode` 暴露）
- `crates/lmt-shared/src/data/schema.rs`（`dump_all`）
- `crates/lmt-cli/src/cli.rs`（`--mode`/`--columns`）
- `crates/lmt-cli/src/commands/total_station.rs`
- `src-tauri/src/commands/`（import shim 加参数）
- `docs/agents-cli.md`

## 12. 非目标（YAGNI）

- 球面 / 自由曲面拟合。
- scatter 模式的 GUI UI 入口（Tauri shim 就位，UI 后续）。
- scatter 模式的现场指令卡（指令卡服务于 grid SOP）。
- 多曲面 / 拼接屏的自动分段拟合。

## 13. 成功标准

1. 用崩铁 CSV：`import --mode scatter` → `reconstruct surface` → `export obj` 三步出 OBJ，
   拟合 R≈9523mm、张角≈165°，`shape_fit_rms` 小，杂点（CD/A/BZ）被剔除为 outlier。
2. CLI 四类 E2E 全过。
3. `cargo test --workspace` 全过。
4. `lmt --json schema` 含 `ScatterFitInfo` / `SamplingMode`。
5. 旧 grid 流程（现状四级链）行为不变，回归测试不破。
