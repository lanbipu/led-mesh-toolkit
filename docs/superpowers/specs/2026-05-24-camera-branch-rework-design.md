# 相机视觉分支重构 — Design Spec（最终版 v2）

**Date**: 2026-05-25
**Status**: Draft v2（吸收 ChatGPT spec review;待用户最终确认 → writing-plans）
**Builds on / supersedes(算法部分)**: `docs/superpowers/specs/2026-05-11-m2-visual-ba-design.md`
**保留**: 该 M2 spec 的 transport / IPC / 打包骨架。**PoC 闸门改为"合成台 + 真实显示器台架"**(现场不配全站仪 → 无全站仪真值 → 不设对全站仪真值的验货门槛);旧 `docs/poc/` 全站仪真值报告模板作废(实施阶段清理)。

---

## TL;DR（给非 CV 读者的大白话）

我们要让一台普通相机,拍 LED 屏(或显示器)上显示的特殊图案,反算出整面屏每块箱体的位置和朝向——**全程不用全站仪**。

- **为什么不用全站仪也能算出真实尺寸**:图案是我们生成、显示在已知像素间距的屏上,所以图案上每个点的真实毫米坐标一开始就知道 → 屏自己就是尺子。
- **现在代码错在哪**:它把图案的已知信息全扔了(只取每个标记的中心点当"未知散点"算)。
- **怎么修**:把每块箱体当"刚性平面板",板上每个角点该在哪我们已知,算法解的是"每块板摆在哪、每张照片从哪拍",而不是几百个散点。
- **怎么证明它准**(两层,都不用全站仪):
  1. **合成台**:电脑里造假屏、假拍照、对答案(算法验证);
  2. **真实显示器台架**:拿两台已知尺寸的显示器实拍,看算出的尺寸/间距/夹角对不对(现实验证)。
- **范围**:第一轮只做 ChArUco(标准棋盘图案);结构光(黑白条纹)设计好但放到后面 gated 再做。最终主要靠 `lmt` 命令行用,GUI 以后再说。

---

## 1. 旧实现的结构性缺陷（已对源码核实）

一句话:**渲染了高精度 ChArUco pattern,却把已知几何全扔了。**

| # | 问题 | 证据 |
|---|---|---|
| 1 | 自由 3D 点 BA | `ba.py` 变量 = 相机 6DOF + 自由点;`reconstruct.py` 把每 marker 中心当自由点 |
| 2 | 连角点都没用 | `detect.py:37` 只 `detectMarkers`,从不 `CharucoDetector`;`reconstruct.py:112-114` 把 4 角点 `mean` 成中心 |
| 3 | cabinet 中心 = 质心平均 | `_aggregate_ba_per_cabinet` → `positions.mean(axis=0)` |
| 4 | 尺度靠"借"先验 | `procrustes_rigid` 无 scale;尺度继承自 nominal seed |
| 5 | 正好 3 anchor 无冗余 | C 模式 `len(frame_anchors)!=3` 报错 |
| 6 | 标定门槛松 | `calibrate.py` `MAX_REPROJECTION_RMS_PX=2.0` |

另:整条视觉分支**未接入 `lmt` CLI**(`adapter-visual-ba` 不是 `lmt-app`/`lmt-cli`/`src-tauri` 依赖,只被自身测试 + `poc_compare` bin 调用)。

---

## 2. 核心认知 + 关键前提

LED 屏是**主动的、已知尺寸的标定物**:每个 pattern 角点有已知物理坐标(像素索引 × 像素间距)→ BA 解里**尺度被固定**,无需全站仪。

但"像素索引 × 间距 = 物理坐标"成立有**硬前提**(ChatGPT §4,采纳):

```
1:1 pixel mapping、无 processor scaling / crop / overscan / 旋转 / 镜像 / remap 错误、
已知 active area 尺寸、已知 pixel pitch、已知 pattern origin 与 active surface origin 的关系
```

否则重建出的是"假设 1:1 下的几何",不可验收。**对策**:引入 `screen_mapping` 清单(§6)并在 `reconstruct` 前做 hash 校验。显示器测试天然满足(原生分辨率显示 + 规格已知像素间距)。

---

## 3. 坐标系与约定（RESOLVED — 消除歧义，ChatGPT §3/§16/§19A）

| 项 | 约定 |
|---|---|
| 相机外参 `T_cam_world` | **camera_from_world**:`x_cam = R·x_world + t`(OpenCV rvec/tvec 同向) |
| 箱体位姿 `T_world_cabinet` | **world_from_cabinet**:`x_world = R·p_local + t` |
| 重投影 | `project(K, T_cam_world · T_world_cabinet · p_local)` |
| 坐标系类型 | `screen_local`(屏幕自身),**唯一输出坐标系** |
| gauge 固定策略(默认) | `fix_root_cabinet`:`T_world_cabinet[root] = identity`,root = cabinet (0,0) → world frame ≡ 根箱体发光面 frame。备选 `align_to_nominal`(仅诊断,不作默认,因会重引入 nominal anchoring) |
| 箱体 local 原点 | **发光面(active emitting surface)中心**(非机械箱体中心);角点在 `±w/2, ±h/2` |
| 轴向 | x = 像素列方向,y = 像素行方向,z = 发光面外法向;右手系;mm |
| `MeasuredPoint.position` | `T_world_cabinet · [0,0,0]` = 发光面中心(model frame,转 m) |
| 机械中心(如需) | 单独记 `active_to_mechanical_offset_mm`,不混入 position |

屏在房间里的**绝对摆位(model→world)由下游既有步骤处理**,本分支不接 world、不用全站仪。

---

## 4. 目标 / 非目标

### 目标
- 自由点 BA → **model-constrained BA**(相机位姿 + 每箱体 SE3,残差用已知 local 角点)
- ChArUco 前端用 `CharucoDetector` 提 board 角点(带已知 local mm)
- 合成几何台(算法验证)+ 真实显示器台架(现实验证),全程零全站仪
- 全部接入 `lmt` CLI(`CLI_DESIGN_SPEC.md` v3.0 + 项目 CLI 契约)
- 默认 ChArUco MVP;结构光设计好但 gated 后做

### 非目标
- **全站仪 / anchor / world datum / N-anchor 评估 / Sim(3) 校尺度**(ChatGPT 多处建议,与"零全站仪"冲突,**全砍**;尺度来自 `screen_mapping`,评估用 gauge-invariant 指标)
- GUI / Tauri 视图(推迟;`lmt-app::visual` helper 现在就备好供后续复用)
- 修改 `core` 的 IR(`MeasuredPoint` 保持冻结;cabinet pose 走独立产物 §9)
- intra-cabinet 形变(单箱体内部非平面)→ 推迟 M3
- Linux 打包;MCP/HTTP adapter(CLI-only)

---

## 5. 架构

transport 全留(Rust adapter ↔ Python sidecar one-shot + NDJSON、IR 契约、atomic 输出)。**一个 BA 内核 + 两个对应关系前端**(让双方法工程量可控):

| 前端 | 对应关系来源 | 实施轮次 |
|---|---|---|
| ChArUco | `CharucoDetector` 提 board 角点,带已知 local mm | **MVP(首轮)** |
| 结构光 | Gray code 解码每像素屏坐标,稠密 | **gated 后做**(§16) |

两者都产出统一观测三元组 `(view_id, p_local_mm, observed_pixel)`,喂同一 model-constrained BA。

调用链(每层薄):
```
lmt visual <op> (lmt-cli, clap + envelope)
  → lmt_app::visual::run_<op> (编排)
    → adapter-visual-ba (Rust: spawn sidecar, 解析内部 NDJSON, IR 转换)
      → python sidecar
    ← 内部 NDJSON (progress/result/error)
  ← 翻译成 CLI envelope / ndjson 事件
```
新增依赖:`lmt-app` 依赖 `adapter-visual-ba`;新 DTO 进 `lmt-shared`(派生 `serde + schemars::JsonSchema`)。

---

## 6. 输入契约：capture_manifest + screen_mapping（ChatGPT §4/§9，采纳）

`reconstruct` 主输入改为 **`--capture-manifest capture.json`**(`--images <dir>` 仅作 convenience,内部先生成 manifest)。

**capture_manifest**(ChArUco):
```json
{
  "method": "charuco",
  "intrinsics": "intrinsics.json",
  "pattern_meta": "patterns/pattern_meta.json",
  "screen_mapping": "screen_mapping.json",
  "views": [ { "view_id": "cam_001", "images": ["captures/cam_001.png"] } ]
}
```
(结构光的 manifest 是帧序列,见 §16。)

**screen_mapping**(尺度可信的根据;无它则重建不可验收):
```json
{
  "screen_id": "...",
  "cabinets": [{
    "cabinet_id": "V000_R000",
    "resolution_px": [w, h],
    "active_size_mm": [w, h],
    "pixel_pitch_mm": [x, y],
    "active_origin": "center",
    "input_rect_px": [...], "rotation": 0, "mirror_x": false, "mirror_y": false
  }],
  "expected_pattern_hash": "..."
}
```
**reconstruct 前 preflight 校验**:`pattern_meta` hash、`intrinsics` 来源、`screen_mapping` hash、图像分辨率、method 一致;不符 → `invalid_input`。

> 显示器场景:`resolution_px` = 原生分辨率,`active_size_mm` 从规格查(或对角线+分辨率算),`pixel_pitch` 随之确定;1:1 显示即满足前提。

---

## 7. BA 内核设计（ChatGPT §10/§11，采纳）

- **变量**:`{相机 i 的 SE3} ∪ {箱体 j 的 SE3}`,gauge 按 §3 固定根箱体。
- **参数化**:SE3 用 `(rvec, t)`(Rodrigues);稀疏 Jacobian(`scipy least_squares` `jac_sparsity`)。
- **畸变策略(明确)**:观测**上游 undistort**(复用 `_undistort_obs`),BA 用 pinhole;残差单位 = undistorted-pinhole 像素。(distortion-in-BA 作为后续精度 refinement,MVP 不做。)
- **robust loss**:Huber(`least_squares loss="huber"` 或显式),抗角点 outlier。
- **初值**:相机用单图 PnP(每图对单箱体解 PnP);箱体用 nominal(`nominal.py` 复用)。
- **协方差**:gauge 固定后从 Jacobian 提每箱体 pose 协方差;不可用 → fallback isotropic + warning。
- **观测权重**:ChArUco 等权起步(后续按 corner confidence 加权);结构光必须分层采样 + 加权(§16),不可百万点等权直喂。

---

## 8. ChArUco 前端 + ID 容量（ChatGPT §13，采纳）

- 检测:`CharucoDetector` 提 board 内角点 → 每角点 `(cabinet, p_local_mm)`(local 以发光面中心为原点,§3);marker 仅用于 ID→cabinet 路由。
- **ID 容量边界**(写明,不再隐含"每箱独立 board 且 marker ID 全局唯一"):
  - **MVP / 中小屏 + 显示器测试**:每箱体独立 ChArUco board,marker ID 分段(现 `pattern.py:_preflight_capacity` 已会在超 `DICT_6X6_1000` 容量时报错)。
  - **大屏(几十~上百箱体)**:超容量时,策略 = (a) 箱体 ID 用独立大号 marker / board header 标识、内部角点局部识别;或 (b) 大屏默认走结构光。Phase 起始按实际屏规模选;MVP 不实现大屏方案,但 preflight 必须明确报错给出路径。

---

## 9. 输出（IR 冻结 + 独立 pose 产物，ChatGPT §6，采纳）

- **`core` IR 不动**:仍输出 `MeasuredPoint { position(发光面中心,m), uncertainty, source: VisualBA }`,喂下游曲面拟合,兼容现有 pipeline。
- **额外正式产物 `cabinet_pose_report.json`**(不写 stderr):
```json
{
  "schema_version": "visual_pose_report.v1",
  "frame": { "type": "screen_local", "gauge_strategy": "fix_root_cabinet", "root_cabinet": [0,0], "units": "mm", "handedness": "right", "z_axis": "outward" },
  "cabinet_poses": [{
    "cabinet_id": "V000_R000",
    "position_mm": [x,y,z], "rotation_matrix": [[...],[...],[...]], "normal": [..],
    "corners_mm": [[..],[..],[..],[..]],
    "reprojection_rms_px": 0.42, "observed_views": 7, "observed_points": 128, "quality": "ok"
  }]
}
```
这样既不破坏 `core`,又保住本次重构最有价值的结果(pose / normal / corners)。

---

## 10. 合成验证台（ChatGPT §7/§17，采纳 + 改写）

### 10.1 分两层（防"自证正确"）
- **Level 0A — 几何模拟器(MVP 建)**:输入真值箱体位姿 + 相机位姿 → 直接生成对应关系 `(p_local, observed_pixel)`,加高斯噪声 / outlier / 可见性 mask / 像素间距误差。**只验 BA 数学正确性**。
- **Level 0B — 图像模拟器(可选,低优先)**:渲染 ChArUco/Gray code 图 + blur/bloom/曝光/卷帘/重采样,再跑真实 `detect.py`。**验前端**。
- **明确写死**:**Level 0A 过 ≠ 现场过**。因为有真实显示器台架(§11),0B 优先级降低——真显示器比渲染图更可信。

### 10.2 评估指标 = gauge-invariant 为主（解 ChatGPT §2）
不依赖坐标系/datum 的量,合成台与真实显示器测试**共用同一套**:
- **每箱体尺寸误差**(重建发光面尺寸 vs 已知)
- **箱体两两距离误差**(中心间距)
- **箱体两两夹角误差**(法向夹角)
- **(全屏)SE(3) 对齐后逐角点 RMS / p95**:对齐用一组点,打分用**互斥**的 holdout 点(防作弊);Umeyama 无 scale 对齐。

### 10.3 三档闸门 + 固定 seed 矩阵（ChatGPT §17，起步值，Phase 0 锁定）
| 档 | 条件 | 门槛(起步) |
|---|---|---|
| unit(零噪声) | — | 逐角点 RMS < 0.1mm;变换恢复在 tol 内 |
| nominal | 0.3px 噪声, ~20 视角, 80% 可见 | RMS ≤ 3mm, p95 ≤ 6mm, 尺寸误差 ≤ 2mm, 距离 ≤ 3mm, 夹角 ≤ 0.3° |
| stress | 1.0px 噪声, 8 视角, 50% 可见, 轻微 pitch 误差 | RMS ≤ 10mm, p95 ≤ 15mm, coverage ≥ 90% |
- 跑**固定 seed 矩阵**(≥ N seeds),报 failure rate;单 seed 不算数。
- 分别报告:shape_error / scale_error / per-cabinet_error / coverage / failure_rate。

---

## 11. 真实显示器台架验证（用户提供，零全站仪，首要现实验证）

**用户开发环境的两台显示器** = 不需要全站仪的真值来源:显示器物理尺寸(分辨率 × 像素间距,规格已知)、两台间距与夹角(可量)。

- **流程**:两台显示器(原生分辨率)显示 pattern → 用户相机多机位实拍 → `reconstruct` → 得两块平面板位姿。
- **验收(gauge-invariant,§10.2 同套指标)**:
  - 每台显示器重建尺寸 vs 规格 → **尺度对不对**;
  - 两台中心距离 vs 实测 → **相对平移对不对**;
  - 两台法向夹角 vs 实测 → **相对旋转对不对**。
- **同时验**:标定质量、ChArUco 检测率、capture_manifest / screen_mapping / CLI 端到端链路。
- **诚实边界**:显示器是平的、画面清晰,**验的是几何 + 尺度 + 前端(清晰显示下)**;真实 LED 的 bloom / 摩尔纹 / 远距离要等真 LED 测试才覆盖(列入风险 §18)。
- 合成台 Level 0A 要能模拟"2 块平面板 + 可配距离/夹角"场景,使**合成与真实显示器测的是同一件事**。

---

## 12. 可观测性 / 质量门（ChatGPT §12，采纳）

全局 RMS 不够,必须 per-cabinet 门(否则局部箱体静默失真):
- 每箱体被 ≥ N 个 view 看到、≥ M 个有效角点、发光面覆盖比 ≥ 阈值、视角 obliqueness 达标;
- per-cabinet reprojection RMS ≤ 阈值;协方差 / Hessian 条件数不异常;
- **观测图连通性**:相机节点 + 箱体节点构成 bipartite graph,每条观测是边 → 整图必须 connected,每箱体有冗余边;不满足 → 报错列出孤立箱体。

---

## 13. CLI 契约（CLI_DESIGN_SPEC §12 五件套）

适配器:CLI 必须;MCP/HTTP 不做;Tauri 推迟(helper 备好)。

| operation_id | command | side_effect |
|---|---|---|
| `visual.calibrate` | `lmt visual calibrate <project> <screen_id> <checkerboard_dir>` | destructive |
| `visual.generate_pattern` | `lmt visual generate-pattern <project> <screen_id> --method charuco`(`graycode` 后做) | destructive |
| `visual.reconstruct` | `lmt visual reconstruct <project> <screen_id> --capture-manifest <json>`(`--images <dir>` convenience) | destructive |
| `visual.simulate` | `lmt visual simulate <config> --out <dir>`(合成几何数据集) | destructive |
| `visual.eval` | `lmt visual eval --dataset <dir> --method <m> [--seed-matrix ...]` | write_safe |

- **exit code**:从 `lmt_shared::exit_codes` 的 app-specific(10–63)段分配,**对现有去重**(`surface_fit` 已占 12)。错误码:`invalid_input`(2)/`image_load_failed`/`detection_failed`/`ba_diverged`/`procrustes_failed`(对齐退化)/`intrinsics_invalid`/`observability_failed`(图不连通/覆盖不足)/`decode_failed`(结构光)/`internal_error`(1)。
- **dry-run**:destructive 操作支持 `--dry-run`(校验 + `data.dry_run_plan`,不写盘)。
- **输出**:长任务默认 `--output ndjson`;sidecar progress → CLI ndjson `type:progress`,末条 `final:true`;`--output json|ndjson` 下 stdout 仅 envelope,日志走 stderr。
- **每 operation 六件套**:lmt-app helper / clap subcommand + 错误映射 + dry-run / lmt-shared DTO(`JsonSchema` + 进 `schema::dump_all()`) / `cli_e2e.rs`(happy+refuse+dry-run+error) / `agents-cli.md` 更新 / `contract-manifest.json` 刷新。

---

## 14. 分阶段（calibration 前置到 Phase 1，ChatGPT §15/§18，采纳）

| Phase | 内容 | 现场? |
|---|---|---|
| **0** | 契约(DTO/schema)+ 合成几何台(simulate/eval)+ 旧自由点 baseline + **model-constrained BA 内核**(SE3/gauge/robust/sparse/per-cabinet 诊断/可观测性门) | 否 |
| **1** | ChArUco 前端(`CharucoDetector`→local mm)+ capture_manifest + screen_mapping preflight + **calibration 命令与门槛前置**(RMS<0.5px、覆盖、pose 多样性、与拍摄同焦同分辨率)+ `reconstruct --method charuco` + `cabinet_pose_report.json` + measured.yaml + 全套 CLI | 否 |
| **2** | **真实显示器台架验证**(§11):两显示器实拍 → 尺寸/距离/夹角对账 + 检测/标定质量自检 + CLI 端到端 | **是(用户)** |
| **3** | 鲁棒性收紧(像素间距误差建模、质量门完善、错误 + cancel 完整化)+ 生产化打包(PyInstaller/CI) | 否 |
| **OPT-SL** | 结构光(§16):仅当 Phase 1/2 未达标或需更高覆盖率;**独立 gated 实施轮,不在首轮** | 否 |

Phase 0 全程零现场;Phase 2 是首个现实验证,**零全站仪**(显示器自带已知真值)。

---

## 15. 成功指标

- **合成台(Phase 0)**:§10.3 三档闸门 + seed 矩阵全过;model-constrained 在各档**显著低于**自由点 baseline(同条件 RMS 对比,差距写进报告)。
- **真实显示器(Phase 2)**:尺寸误差、两两距离误差、夹角误差达到 §10.3 nominal 档同量级(具体值 Phase 0 锁定后套用)。
- **CLI 一致性**:`lmt --json schema` 含新 DTO;`lmt manifest` 含新 operation_id;`lmt visual <op> --help` 人话;`cargo test --workspace` 全过(含 E2E)。

---

## 16. 结构光（设计保留，gated 后做，ChatGPT §8/§9/§11）

- **pattern**:`generate-pattern --method graycode` 输出时序帧(水平+垂直 Gray code,含 inverse frame + all-white/all-black reference)。
- **capture_manifest(结构光形态)**:每 view 是帧序列,需 `frame order / pattern_id / 是否 inverse / reference`:
```json
{ "method": "structured-light",
  "views": [{ "view_id": "cam_001",
    "frames": [{"pattern_id":"h_bit_00","path":"..._000.png"},{"pattern_id":"h_bit_00_inv","path":"..._001.png"}] }] }
```
- **解码**:`structured_light.py` 逐像素恢复屏坐标 + confidence map;阈值 / gamma / PWM / 卷帘 / 屏像素量化是误差源,需建模。
- **喂同一 BA**,但必须:**每 cabinet/每 view 分层采样 100–500 点**、按 confidence 过滤、空间网格去重、加权(防稠密点压制)。
- **同步**:屏幕帧序与相机帧序对齐(手动按序 vs 屏侧节拍)——Phase 起始先定 SOP。
- **gated 条件**:Phase 1 ChArUco 未达 10mm 或需更高覆盖率才启动。

---

## 17. 待解决 / 显式假设（writing-plans 前确认）

1. `screen_mapping` 的 `active_size_mm`:显示器从规格查;LED 屏从厂商规格 + 处理器配置确认(关系到尺度可信度)。
2. 标定 RMS 具体门槛:Phase 0 合成 + 一次真实标定锁定(起步 < 0.5px)。
3. exit code 数值:对 `lmt_shared::exit_codes` 去重后定。
4. 结构光屏侧同步 SOP:Phase OPT-SL 起始定。
5. 大屏 ID 容量方案选型(§8):按实际屏规模,MVP 不实现。
6. `core::auto_reconstruct` 接受 screen_local frame 的 MeasuredPoints:应接受(只做曲面拟合,坐标系无关),实施时确认。

---

## 18. 风险

| 风险 | 缓解 |
|---|---|
| 纯视觉精度达不到 10mm | Phase 0 合成台先证;不达标优先靠结构光提精度,仍不行停下重评估,**不引全站仪** |
| 合成台自证正确 / 与现实有 gap | 分 0A 几何 + 真实显示器台架两层;明说"0A 过≠现场过";gauge-invariant 指标 |
| 显示器测试覆盖不了 LED bloom/摩尔纹/远距 | 显示器验几何+尺度+前端;真 LED 的光度问题列为后续真 LED 测试(本 spec 不含) |
| LED 像素间距误差侵蚀尺度 | `screen_mapping` 显式声明 + preflight;误差是已接受的 10mm 级 tradeoff,**不引全站仪** |
| gauge 未固定致刚体漂移 | §3 明确 `fix_root_cabinet`;BA 单测验证 |
| 局部箱体静默失真 | §12 可观测性 + 观测图连通性门 |
| 结构光稠密点撑爆 BA / 压制 | §16 分层采样 + 加权;gated 才做 |
| CLI envelope 与 sidecar NDJSON 翻译错 | adapter 翻译单测 + E2E 覆盖 error/dry-run/ndjson |
| 范围过大 | ChArUco MVP 优先;结构光 gated;现场只用已知真值显示器 |
