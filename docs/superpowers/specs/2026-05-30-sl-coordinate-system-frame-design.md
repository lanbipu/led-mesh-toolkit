# 视觉/结构光重建落入项目坐标系 — 设计 spec

- **日期**: 2026-05-30
- **状态**: 设计已确认，待实施计划（writing-plans）
- **作者**: lanbipu + Claude
- **关联**: `docs/agents-cli.md`（CLI 契约）、`crates/lmt-app/src/total_station_mapper.rs`（coordinate_system 现有唯一消费者）

---

## 1. 问题

视觉/结构光链路 `reconstruct-structured-light → export pose-obj → disguise` 导出的 LED 墙模型，**落在一个浮动/猜出来的坐标系里**，跟 disguise 里的设计源模型差约 1°（2.5m 高的墙顶端 ~5cm 前后错位），无法直接 drop-in 对位替换源模型。

根因（已读源码核实）：

1. **重建端**（Python sidecar `reconstruct.py`）把根箱体 `V000_R000` 钉死在 `R=I, t=0`，整墙坐标只是"相对根箱体"的局部帧。sidecar docstring 明说 `"screen-local reconstruction, no anchors / world datum"`（reconstruct.py:5-8）。**绝对朝向是未定义的**。
2. **导出端**（`run_export_pose_obj`，默认 disguise、无 `--root`）用 `apply_canonical_frame`（export.rs:458-502）**猜**一个摆法：取中心列平均法向绕 +Y 转 yaw 到 +Z（只修 yaw）、`flipY`、贴地、水平居中。`roll/pitch` 刻意保留。
   - 纠正一个常见误解：默认情况下原点**不是** `V000_R000`，而是"全墙水平质心 + 最低点"；只有传 `--root V000_R000` 才把原点钉到该箱体。"上 = 垂直"硬编码成世界 +Y，不是从任何箱体推的。
3. 项目早有 `coordinate_system`（三点定帧：`origin_point` / `x_axis_point` / `xy_plane_point`），但**只有全站仪路径在用**（`total_station_mapper.rs`）；视觉/结构光路径 `visual.rs` 加载同一个 `project.yaml`，却只读 `cfg.screens`，**从不读 `cfg.coordinate_system`**。

> 关键历史发现：sidecar 里 `FrameSpec.gauge_strategy="align_to_nominal"`（ipc.py:402）、`procrustes.py`、`nominal.py`、`procrustes_align_rms_m` 字段、`LmtError::ProcrustesFailed`（exit 15）**全部现成但休眠**——"对齐到 nominal" 项目以前实现过，后来为简化换成"根箱体固定 identity + 导出时猜"，把它废成了 identity（reconstruct.py:583-609 现在写死 `fix_root_cabinet` + `procrustes_align_rms_m=0.0`）。**现在导出阶段要靠猜补回来的那段精度，正是当年被砍掉的那段。** 本设计本质是"把它请回来并做对"。

---

## 2. 目标 / 非目标

### 目标
- 视觉/结构光导出的模型落在**项目定义的设计坐标系**里，可直接 drop-in 替换 disguise 源模型。
- 朝向误差从 ~1° 降到重建噪声 + 内参系统误差的本底水平（远小于 5cm/顶端）。
- 复用既有 `coordinate_system` 机制（用户已知的接口），不新造一套坐标定义。
- 满足 `CLAUDE.md` 的 CLI 维护契约：业务在 `lmt-app`、CLI 子命令同步、E2E、文档、schema。

### 非目标
- **不做** full joint bundle adjustment / 相机内参联合优化（内参误差是系统性本底，本任务不修，列为前提假设）。
- **不做** Folded（折屏）的精确支持（见 §7 范围）。
- **不改** 全站仪路径的对外行为（只在内部抽共享 helper 时顺带重构，行为不变）。
- **不引入** 真实世界绝对锚点（ArUco/survey）——视觉单独给不了绝对世界定位，超出本任务。

---

## 3. 核心技术决策（已与用户确认）

| 决策点 | 选择 | 理由 |
| --- | --- | --- |
| 目标帧定义 | **复用 coordinate_system 三点帧** | 跟全站仪/设计模型同一套约定；不需要额外输入 |
| 落点架构 | **方案 1 + 共享 helper** | 业务在 lmt-app/lmt-core；帧定义全站仪与导出共用同一段代码 |
| 帧精度算法 | **B 全局拟合（Procrustes 对齐到 nominal）** | 用全部点 √N 压噪；3 点定帧在短基线下噪声本身就 ~0.5°，不可靠 |
| 全局拟合落点 | **混合**：重建端做稳健配准，导出端做 coordinate_system 重锚 | 重建端 Procrustes/nominal 机器现成（复活休眠）；导出端只做无噪声的重锚 + target 适配 |

### 3.1 为什么是"全局拟合"而不是"裸三点定帧"

刚体三点定帧的角度误差 ≈ `点噪声 / 基线长度`（σ/L）。默认 `coordinate_system`（`V001_R001` / `V004_R001` / `V001_R002`）基线只有 1~3 个箱体：

- X 轴基线 ~1.5m，σ≈3mm → ~0.16°
- **上方向基线 ~0.5m（1 个箱体高）→ ~0.5°，与要修的 1° 同量级**

全站仪也是三点定帧（`build_frame_from_first_three`，reference_frame.rs:24-91），但它 σ 是亚毫米级所以扛得住；**视觉 σ 大一个数量级，不能照搬**。

全局拟合（Procrustes 把全部重建角点配准到已知 nominal）用所有点：随机噪声 √N 抵消（3 点 → 几百角点 ≈ 10× 改善），且最大化空间跨度。**关键：Procrustes 是纯刚体（无缩放），只借整体摆放，as-built 的真实形变全部保留。** coordinate_system 的三个点改成在**无噪声的 nominal** 上求值，所以那 3 个点永远不碰重建噪声。

精度本底（诚实声明）：√N 只压**随机**噪声；相机内参（focal/主点）/ 畸变残差是**系统性**误差，再多点也抵消不掉。内参质量仍是本底（本项目已知内参最敏感：focal ≲2%、主点须锁死）。

---

## 4. 端到端数据流

```
reconstruct-structured-light  (Python sidecar reconstruct.py / solve_and_emit)
  BA 解算 → 所有箱体角点在「根箱体 V000_R000 = identity」局部帧
  ↓ 复活 align_to_nominal（默认开启）:
  P @ 全部重建箱体角点                          # ① 显式定号置换：重建帧→M0.1 约定（确定性，不交给拟合）
  (R,t) = procrustes_rigid(P@src, dst_m01)      # ② 稳健摆放：同约定下近恒等，SVD 无缩放
  dst_m01 = M0.1 约定的 nominal 角点（见 §5.3）
  → corners_mm 写成 M0.1 帧；FrameSpec.gauge_strategy="align_to_nominal"（帧版本位）
  → ResultData.procrustes_align_rms_m = 实测对齐残差
                              │
                              ▼  <screen>_cabinet_pose_report.json （已在设计 nominal 帧，√N 稳健）
export pose-obj --coordinate-system <project.yaml>   (Rust run_export_pose_obj)
  读 project.yaml → coordinate_system 三个网格名 + 对应 screen 的 cabinet 配置
  在「无噪声 nominal」上解析 origin/x_axis/xy_plane 三个顶点的 3D 位置
  → 共享 helper: from_three_points + M0.1 排列 → 设计重锚帧 F
  → 对 report 每个角点 F.world_to_model(·) → 重锚到用户选的设计帧
  → adapt_to_target(disguise/unreal/neutral) 每顶点 + disguise winding/UV
  → OBJ（drop-in 设计源模型）
```

无 `--coordinate-system` 时：行为按 **report 帧版本**（`FrameSpec.gauge_strategy`）分支——**旧 `fix_root_cabinet` report 完全不变**（现有 canonical 猜测 / `--root` / `--ground`），**新 `align_to_nominal` report 跳过猜测**（几何已稳健摆进设计帧，只做约定→target 适配）。见 §5.2 / §5.5。

---

## 5. 详细设计

### 5.1 生产端改动（Python sidecar `reconstruct.py`，复活 align_to_nominal）

现状（reconstruct.py:583-609）：写死 `FrameSpec(gauge_strategy="fix_root_cabinet")`，`procrustes_align_rms_m=0.0`，**不跑** Procrustes。

**`solve_and_emit` 显式参数化（F3，关键）**：`solve_and_emit` 被 charuco（`visual reconstruct`）与 SL（`reconstruct-structured-light`）**共用**。新增显式参数 `gauge_strategy: Literal["fix_root_cabinet","align_to_nominal"]`，由调用方传：**SL 传 `align_to_nominal`，charuco 维持 `fix_root_cabinet`（输出不变）**。不靠"默认值"隐式改 charuco——必须有一个测试断言 charuco 的 report / measured.yaml 帧逐位不变。charuco 是否迁移是独立决策（见 §11），但默认**不**误伤。

改动（仅 `gauge_strategy == "align_to_nominal"` 时）：

1. BA 解出箱体局部位姿（**重建帧**）后，**在构建 report 与 measured.yaml 之前**：
   - **a) 换约定（显式定号置换，非 Procrustes，F1）**：对每箱体角点左乘固定矩阵 `P`，把**重建帧**（X=列, Y=上, Z=外法向）转成 **M0.1 约定**（X=列, Y=外法向, Z=上）。`P` 是确定性的有号置换，与 `reference_frame.rs:85-89` 的 `[b0, b2, -b1]` 构造对应（含让 det=+1 的那个负号）。**换约定绝不能交给 Procrustes 去拟**（见 §5.3 为什么）。
   - **b) 稳健摆放（Procrustes）**：`(R,t) = procrustes_rigid(src_m01, dst_m01)`（procrustes.py:23），`src` = 换约定后的全部角点，`dst` = M0.1 nominal 角点（`nominal_cabinet_corners_m01`，§5.3）。两边同约定 → 平墙时 R≈identity（良态，无平面翻面二义）；曲墙非平面（3D 展开）→ 良态。
   - 对每箱体 `corners_mm`、`position_mm`、`rotation_matrix`、`normal` 先 `P` 后 `(R,t)`（点变换；法向/旋转左乘）。
   - 残差 `rms` → `ResultData.procrustes_align_rms_m`（m）。
   - **时机**：必须在构建 `cabinet_poses`→report（reconstruct.py:562-594）**和** `measured.yaml`（reconstruct.py:597-602，同源派生）**之前**，两产物都落 M0.1。`MeasuredPoints.coordinate_frame` 取值见 §11。
2. `FrameSpec.gauge_strategy = "align_to_nominal"`（这是 report 的**帧版本位**，导出端据此分支，§5.5）。
3. **失败处理**：箱体 <3 / 全共线 / 退化 → `ProcrustesFailed`（procrustes.py 已 `raise ValueError` → `LmtError::ProcrustesFailed` / exit 15，envelope.rs:124 现成）。
4. **Folded fail-fast（F4，生产端已有）**：`nominal.py:81-119` 对 folded 已抛 ValueError；保留，确保 align 目标取不到 folded nominal 时硬失败而非退化成平面。

> 角点 vs 中心：现有 `nominal.py::nominal_cabinet_centers_model_frame` 给中心（N 点，重建约定，BA 种子用，**不动**）。本设计的对齐目标新增 **M0.1 约定的全部角点**（4N 点，§5.3）——多 4× 数据，每箱体 4 角额外约束法向/朝向，对"上/法向"估计更稳健。

### 5.2 消费端改动（Rust `run_export_pose_obj`，加 coordinate_system 重锚）

`crates/lmt-app/src/export.rs::run_export_pose_obj`（现签名 export.rs:153-159）新增入参 `coordinate_system: Option<&Path>`（指向 `project.yaml`）。导出端**先读 report 的 `FrameSpec.gauge_strategy`** 分支（见 §5.5），再按 `--coordinate-system` 决定重锚：

- **report 是 `align_to_nominal`（新，已在 M0.1 设计帧）**：
  1. **跳过** `apply_canonical_frame`（几何已稳健摆放，再猜会双重定向）。
  2. 给了 `--coordinate-system`：`load_project_yaml_from_path` → 三网格名 + screens；前缀匹配定 screen（复用 `total_station_mapper.rs:54-104`）；**Folded fail-fast（F4）**：该 screen 是 Folded → `invalid_input`（Rust `expected_grid_positions` 把 folded 当平面会无声出错，必须拒）；共享 helper（§5.4）在 M0.1 nominal 上解析 origin/x_axis/xy_plane → 设计帧 F；对每角点 `F.world_to_model`（coordinate.rs:143）重锚。
  3. 没给 `--coordinate-system`：几何已在 M0.1 nominal 默认锚，直接进 target 适配。
  4. `adapt_to_target(v, target)`（adapt.rs:22）每顶点 + disguise winding/UV（与 `build.rs::surface_to_mesh_output` 第 5 步一致）。
- **report 是 `fix_root_cabinet`（旧/root-local）**：走**现有分支**（canonical 猜测 / `--root` / `--ground`），**零行为变化**——保旧产物兼容。若此时**传了 `--coordinate-system`**：拒（`invalid_input`，"该 report 未对齐 nominal，coordinate_system 重锚需要 align_to_nominal report"），不静默出错。

`check_pose_obj_inputs`（export.rs:291）同步加预检（dry-run/execute 对齐）：读 report frame 版本；`--coordinate-system` 时 project.yaml 可读、screen 可解析、非 Folded、三网格名合法且能解析到 nominal 顶点；`fix_root_cabinet` + `--coordinate-system` 组合拒绝。

> 设计帧通常接近"nominal 默认锚"（origin=V001_R001、X 沿底行、上沿首列）：此时 F ≈ identity，输出 ≈ 已对齐的 report；非默认 coordinate_system 时 F 把模型重锚到用户选的设计原点/朝向。
>
> 曲面墙注意：`from_three_points` 用的是 3 个顶点，得到的是**弦/割线帧**（两参考点连线方向当 +X），不是某点的切线帧——这与全站仪 `build_frame_from_first_three` 完全一致，正是要的"与全站仪/设计同帧"。

### 5.3 跨语言 nominal 帧统一（**硬约束**）

两个帧约定在用（两边网格原点**都**在 V001_R001 左下顶点 = V000_R000 的 BL 角，已核实——初稿"中心 vs 顶点差半箱体"源自一次传输损坏的 grep，作废）：

- **重建帧**（pose report `corners_mm` 所在；根箱体 V000_R000 局部帧）：`X=列, Y=行向上, Z=外法向`。`reconstruct_cabinet_geometry`（normal=R@[0,0,1]）+ export.rs 注释证实——这正是现有 pose-obj 当作 "disguise-native +Y up/+Z outward" 的帧。Python `nominal.py`（`_cabinet_center_model_m`：中心 `(col+0.5)*cw, (row+0.5)*ch, z`，法向 `[sin a,0,cos a]`）也是这个约定（Y=行, Z=法向），BA 内部当种子/消歧用。
- **M0.1 / measured.yaml / 设计帧**（全站仪 + `adapt_to_target` 输入；adapt.rs:9 "Model frame: +Z up, +Y outward normal"）：`X=列, Y=外法向, Z=行向上`。即 Rust `expected_grid_positions`（shape_grid.rs:38-39,70-71，Y 承法向、Z 承行）+ `from_three_points` 的 M0.1 排列。

两者差一个 **Y↔Z 轴约定**（不是平移）。生产端落帧与消费端解析必须落在**同一个 M0.1 帧**。

**换约定与稳健摆放必须分开做（F1，关键）**：
- 换约定（重建帧→M0.1）是**确定性的有号置换 `P`**，用一个固定矩阵显式做（对应 `reference_frame.rs:85-89` 的 `[b0,b2,-b1]`，那个负号保证 det=+1 的正常旋转）。
- **绝不能把换约定丢给 Procrustes 去拟**：两组**右手系**之间本是正常旋转，但平墙的角点**共面**，Procrustes 对共面点集有"法向翻面"二义性——det=+1 会选一个解，可能恰好把法向翻到反面，而角点 RMS 仍≈0、`procrustes_align_rms_m` **抓不到**。先 `P` 再 Procrustes，则 Procrustes 在同约定下只做近恒等的摆放（平墙 R≈I，曲墙非平面良态），不进危险的大旋转区。
- 验证靠 **golden 测试断言法向方向 + winding**（flat 与 curved 各一），不只比顶点位置。

落地：
- **新增** `nominal_cabinet_corners_m01(mapping) -> {(col,row): (4,3) corners_mm}`（Python），按 M0.1 约定（镜像 Rust `expected_grid_positions` 的 Flat/Curved 顶点公式 + 角点布局，shape_grid.rs:34-100）。**不动**现有 `nominal_cabinet_centers_model_frame` / `nominal_cabinet_normals_model_frame`（BA 仍用重建约定）。
- 单位：sidecar mm / Rust m，换算点在 helper 边界明确。
- **跨语言 golden 测试**：同一 screen 配置下，Python `nominal_cabinet_corners_m01` 与 Rust `expected_grid_positions` 顶点位置逐项一致（< 1e-6 m），且法向/winding 方向一致。

### 5.4 共享 helper

「同一段代码」保证全站仪与导出帧定义一致：

- **lmt-core**（纯几何）：抽 `CoordinateFrame::from_three_points_m01(origin, x_axis, xy_plane) -> CoordinateFrame` = `from_three_points`（coordinate.rs:91）+ M0.1 排列 `[b0, b2, -b1]`（现散在 reference_frame.rs:85-89）。`reference_frame.rs` 重构为调用它（全站仪行为不变，仅去重）。
- **lmt-app**（解析）：`design_frame_from_grid_names(project_cfg, screen_id) -> CoordinateFrame`：网格名解析（剥屏幕前缀 + 1-based→0-based + 顶点↔箱体角，复用 mapper.rs 前缀逻辑）→ 用 `expected_grid_positions` 取 nominal 顶点 3D 位置 → 调 lmt-core m01 builder。导出端 coordinate_system 分支调它。

### 5.5 report 帧版本化 + 兼容（F2，关键）

不能既"生产端默认出 M0.1"又"无 flag 导出行为不变"——这是初稿的真矛盾。修法是**让 report 自带帧版本，导出端据此分支**：

- **帧版本位** = `FrameSpec.gauge_strategy`（现成枚举 `"fix_root_cabinet" | "align_to_nominal"`，ipc.py:402）。`align_to_nominal` ⇒ 几何在 M0.1 设计帧、已稳健摆放；`fix_root_cabinet` ⇒ 旧重建/root-local 帧。
- **slim DTO 必须读到它**：`CabinetPoseReportFile`（dto.rs:338-342）现在**丢弃** `frame`；新增 `#[serde(default)] frame: PoseReportFrame { gauge_strategy: String }`（缺省 = `fix_root_cabinet`，老 report 自动归旧路径）。新 DTO 需派生 serde+JsonSchema 并进 `schema::dump_all()`。
- **导出分支**见 §5.2：旧 report 走原 canonical/`--root`/`--ground`（**逐位不变**）；新 report 跳过 `apply_canonical_frame`。`fix_root_cabinet` + `--coordinate-system` 组合 = `invalid_input`（不静默套错轴）。
- **兼容验证**：E2E 必须双覆盖——① 旧 `fix_root_cabinet` report 无 flag 导出，字节级对照既有快照；② 新 `align_to_nominal` report 导出（带/不带 `--coordinate-system`）。

### 5.6 命名/编号对账（已核实）

- coordinate_system 网格名：`{screen}_V{c+1:03}_R{r+1:03}`（shape_grid.rs:41），**网格顶点**、**1-based**、有屏幕前缀，共 `(cols+1)×(rows+1)`。
- pose report cabinet_id：`V{col:03}_R{row:03}`（reconstruct.py:249），**箱体**、**0-based**、无前缀，共 `cols×rows`。
- 对应：顶点 `(vc,vr)`（0-based）= 箱体 `(vc,vr)` 的 BL 角；边缘顶点（`vc==cols` 或 `vr==rows`）回退到相邻箱体对应角（BR/TL/TR）。
- 解析顺序：剥最长匹配屏幕前缀 → 解析 `V/R` → 减 1（顶点 1-based）→ 顶点→箱体角。

---

## 6. 错误处理

| 场景 | 端 | 错误码 | 退出码 | 现状 |
| --- | --- | --- | --- | --- |
| Procrustes 退化（<3 箱体 / 共线） | 生产 | `procrustes_failed` | 15 | 现成（envelope.rs:124） |
| coordinate_system 网格名格式非法 | 消费 | `invalid_input` | 2 | 现成 |
| 网格名解析到的顶点超出网格 | 消费 | `invalid_input` | 2 | 现成 |
| project.yaml / screen 找不到 | 消费 | `not_found` | 3 | 现成 |
| target 非法 | 消费 | `invalid_input` | 2 | 现成 |

新错误分类：暂无（全部复用现有码）。若新增需三处同步（`error_codes::*` / `exit_codes::*` / `docs/agents-cli.md`，CLAUDE.md 契约）。

---

## 7. 范围 / 已知限制（诚实声明）

1. **Folded（折屏）不支持**：`expected_grid_positions` 现把折屏当平面（shape_grid.rs:84），把折叠 as-built 配准到平 nominal 会错位。**首版只保 Flat + Curved**（双弧/强弯曲属 Curved，nominal 忠实，OK）。折屏列 follow-up：要么补真实折叠 nominal，要么折屏走别的路。
2. **内参系统误差是本底**：√N 只压随机噪声；focal/主点/畸变有偏的话点再多也卡本底。前提假设，非本任务修。
3. **active-surface 角 vs 物理标记点**：视觉的发光区角点（数学）与全站仪棱镜实际贴点可能差几 mm，只影响原点（落"无所谓"档），方向上基本抵消。
4. **charuco 路径不动**：本设计聚焦 SL。`solve_and_emit` 共用，但靠显式 `gauge_strategy` 参数隔离——charuco 维持 `fix_root_cabinet`，有测试锁死输出逐位不变（F3）。charuco 迁移是独立 follow-up（§11）。

---

## 8. 风险

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 平墙 Procrustes 法向翻面二义（F1）：共面点集 det=+1 拟合可能翻法向，RMS≈0 抓不到 | 整面墙正反面翻转、winding 错 | §5.3 换约定用**显式定号置换 `P`**（非 Procrustes）；Procrustes 只做近恒等摆放；golden 断言法向+winding（flat/curved） |
| charuco 被 align_to_nominal 误伤（F3）：solve_and_emit 共用 | 既有 charuco 输出/measured.yaml 帧回归 | §5.1 显式 `gauge_strategy` 参数，charuco 维持 `fix_root_cabinet`；测试断言 charuco 逐位不变 |
| 帧版本兼容（F2）：默认改 M0.1 与"无 flag 不变"冲突 | 旧 reconstruct→export 工作流静默出错 | §5.5 `FrameSpec.gauge_strategy` 版本位 + slim DTO 读 `frame` + 导出分支；旧 report E2E 字节对照 |
| Folded 无声出错（F4）：Rust `expected_grid_positions` 把 folded 当平面 | folded 项目导出貌似合理实则错位的平面 OBJ | 生产端 `nominal.py` 已 fail-fast；导出 `--coordinate-system` 加 Folded `invalid_input` 守卫 + E2E 拒绝 |
| 命名不一致：visual measured.yaml 用 `MAIN_V{0-based}`（reconstruct.py:629），全站仪/coordinate_system 用 `{screen}_V{1-based}` | pose-obj 走 cabinet_id 不受影响；将来按 name 交叉引用会撞 | 记录，见 §11 |
| Procrustes 被离群点带偏 | 帧偏 | v1 用全角点最小二乘；若实测有离群，follow-up 上 robust（Huber/截尾），输出 `procrustes_align_rms_m` 供监控 |
| as-built 与 nominal 偏差大（错 shape_prior） | 拟合残差大 | 暴露 `procrustes_align_rms_m`，超阈值告警 |

---

## 9. CLI 契约交付项（CLAUDE.md）

### 生产端 `reconstruct-structured-light`
1. **sidecar**：`solve_and_emit` 加显式 `gauge_strategy` 参数（F3）；SL 调用传 `align_to_nominal`、charuco 传 `fix_root_cabinet`。`align_to_nominal` 分支 = 显式置换 `P`（F1）+ `procrustes_rigid` 摆放 + 新增 `nominal_cabinet_corners_m01`（§5.3）。
2. **lmt-app**：`run_reconstruct_structured_light` 不变签名（SL 默认 align_to_nominal 由 sidecar 调用方式决定）。
3. **DTO**：写 `FrameSpec.gauge_strategy="align_to_nominal"`；`procrustes_align_rms_m` 落到 `VisualReconstructResult`（dto.rs:242，新增字段保 serde+JsonSchema 并进 `schema::dump_all()`）。
4. **错误**：`ProcrustesFailed`（现成）；folded fail-fast（nominal.py 现成）。
5. **E2E / 单元**：SL 块（~2089-2159）补 align 后 happy + procrustes_failed；**charuco 逐位不变**断言（F3）；flat/curved 法向+winding golden（F1）。
6. **docs/agents-cli.md**：更新 SL 行 + side_effect。

### 消费端 `export pose-obj`
1. **lmt-app**：`run_export_pose_obj` / `check_pose_obj_inputs` 加 `coordinate_system: Option<&Path>`；**读 report `frame` 版本分支**（§5.5）；**Folded `invalid_input` 守卫**（F4）；`fix_root_cabinet` + `--coordinate-system` 拒绝（F2）。
2. **lmt-shared DTO**：`CabinetPoseReportFile` 加 `#[serde(default)] frame`（§5.5，新类型保 serde+JsonSchema 进 schema dump）。
3. **lmt-core / lmt-app**：共享 helper（§5.4）；显式置换 `P` 复用同一定义。
4. **CLI**：`cli.rs ExportCmd::PoseObj`（273-290）加 `--coordinate-system <PROJECT_YAML>`；`commands/export.rs::pose_obj`（137-213）穿到 dry-run payload + execute（**dry-run/execute 对齐**）。
5. **Tauri**：export 非 CLI-only，确认是否需同步 `#[tauri::command]`（agents-cli.md 核对）。
6. **E2E**：pose-obj 块（~1592-1799）补 happy（带 coordinate_system）/ refuse（无 `--yes`）/ dry-run / error（坏 yaml、解析不到顶点、Folded、`fix_root_cabinet`+flag）；**旧 report 字节对照**（F2）；**新 align report 带/不带 flag**（F2）。
7. **docs/agents-cli.md**：更新 pose-obj 行；错误码表；contract-manifest.json（export.pose_obj 条目 259-276）+ `manifest.rs:118`。
8. **schema**：DTO 改动 → `lmt --json schema` 校验。

### 自检（合并前）
```bash
cargo test --workspace
./target/debug/lmt --json schema | jq
./target/debug/lmt export pose-obj --help        # 新 flag 注册
./target/debug/lmt --help
# sidecar: cd python-sidecar && .venv/bin/pytest（procrustes / nominal / golden）
```

---

## 10. 测试计划

- **单元（Rust）**：`from_three_points_m01` 正交/右手/M0.1 轴向；置换 `P` 把重建帧→M0.1（F1）；`design_frame_from_grid_names` 命名解析（前缀/1-based/顶点↔角/边缘回退）；坏输入报错。
- **单元（Python）**：`procrustes_rigid`（已知 R,t 往返）；`nominal_cabinet_corners_m01` Flat/Curved 公式；`solve_and_emit(gauge_strategy)` 两分支。
- **法向/winding golden（F1）**：flat 与 curved 各一面已知墙，断言导出 OBJ 的法向方向 + winding 正确（不只比顶点位置）——专门抓平面翻面二义。
- **charuco 不变（F3）**：同一输入下 `gauge_strategy="fix_root_cabinet"` 的 report/measured.yaml 与改动前逐位一致。
- **帧版本兼容（F2）**：旧 `fix_root_cabinet` report 无 flag 导出字节对照；新 `align_to_nominal` report 带/不带 `--coordinate-system`。
- **Folded 拒绝（F4）**：folded 项目 export `--coordinate-system` → `invalid_input`。
- **跨语言 golden**：Python `nominal_cabinet_corners_m01` == Rust `expected_grid_positions`（§5.3）。
- **集成**：合成一面已知位姿的墙 → 加噪 → SL 重建 → align_to_nominal → export pose-obj --coordinate-system → 与已知设计模型逐点 3D 误差 < 阈值（验"全局拟合优于 3 点"：对比 3 点裸定帧的误差）。
- **E2E（CLI）**：见 §9，happy/refuse/dry-run/error 四类各覆盖。
- **验收**：与"已知正确"对账（memory: verify against known-good）——首选与同一面墙的全站仪导出 diff，或与设计模型 diff。

---

## 11. 待实施时确认（不阻塞设计）

- nominal 单位：sidecar mm vs Rust m，换算点明确（统一在 helper 边界）。
- `--coordinate-system` 多 screen 时屏幕识别：默认从网格名前缀推；是否需显式 `--screen`。
- **charuco 迁移到 align_to_nominal**：本任务**不做**（F3 已定：charuco 维持 `fix_root_cabinet`、显式参数隔离、测试锁死不变）。charuco 同样吃 root-local 任意朝向之苦，未来可单独评估迁移——独立决策，不在本 spec 范围。
- visual measured.yaml 命名（`MAIN_V{0-based}`）是否要与全站仪（`{screen}_V{1-based}`）统一——本任务不强求，但记一笔避免将来按名字对账撞车。
- `MeasuredPoints.coordinate_frame` 在 align_to_nominal 下取值（identity vs 设计帧）——实施时定，需与 measured.yaml 消费方一致。
