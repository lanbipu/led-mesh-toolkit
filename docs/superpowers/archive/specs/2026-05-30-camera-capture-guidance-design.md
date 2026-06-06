# 相机采集指导规划器（Camera Capture Guidance Planner）— 设计 spec

- **日期**: 2026-05-30
- **状态**: 设计已确认，待实施计划（writing-plans）
- **作者**: lanbipu + Claude
- **修订**: v2 整合——可见性模型逐采样点化，并对齐 `reconstruct.py` 的真实观测闸门（替代 v1 的「箱体中心可见=整箱可见」捷径）
- **关联**:
  - `python-sidecar/src/lmt_vba_sidecar/sl_feasibility.py`（打分器基底，本设计要改造它）
  - `python-sidecar/src/lmt_vba_sidecar/nominal.py`（几何展开，复用）
  - `python-sidecar/src/lmt_vba_sidecar/reconstruct.py`（真实观测闸门常量来源：`MIN_PNP_CORNERS` / `check_observability` / `QUALITY_MIN_VIEWS`）
  - `crates/adapter-total-station/src/instruction_card/`（HTML 指导卡范式，对照物）
  - `docs/agents-cli.md`（CLI 契约）
  - 选型背景见记忆 `project_screen_backcalc_goal_and_omnical_scope`、`project_sl_curved_wall_bridging_blocker`

---

## 1. 问题

lmt 的结构光重建精度被**相机机位几何**卡住。现场用户不知道机位该摆在哪、朝哪拍、布多密。机位堆得太正（交会角小）→ 深度方向约束弱 → 弧端和顶底边缘箱体的重建误差明显偏大（合成台实测形状残差正集中在这些区域）。

需要一个**采集指导工具**：输入墙的基础几何（`project.yaml` 的弧半径/箱体网格/箱体尺寸）+ 相机内参（FOV、sensor），输出一份**带屏幕模型、标出每个推荐机位位置与拍摄朝向的可视化采集指导**，让现场用户照着拍就能采到满足重建精度要求的图像。

### 两个奠基事实（已读源码核实）

1. **「合格」已有现成定义**：`compare_known.py:17-21` 的 §10.3 容差 `{size_mm: 2.0, distance_mm: 3.0, angle_deg: 0.3}`。采集指导的精度目标直接挂这套，不另立标准。

2. **真实重建是「逐观测」的，且有硬性观测闸门**——这条决定了本工具的重心。
   - 真实链路逐箱体解 PnP，再做 model-constrained BA。`reconstruct.py` 的硬闸门 `check_observability(min_views=2, min_points=8)`（line 183/421）要求**每个箱体 ≥2 视图 + ≥8 观测点**；每视图 PnP 要 ≥`MIN_PNP_CORNERS=4` 角点（line 78）；`QUALITY_MIN_VIEWS=4`（line 92）以下虽能解但标「低观测」。
   - 现有打分器 `sl_feasibility.py::feasibility_rms_mm`（line 111-118）的内层循环是「每台相机 × 每个点」，**默认每台相机都能看到墙上每一个点**——没有视野、正背面（cheirality）、掠射角、弧面自遮挡、箱体间桥接链等任何检查。对一面 60m 弧墙，站左端的相机物理上看不到右端，但它当作看得到，因此**对大墙/弯墙给的分偏乐观**。

   含义：**这个工具最难的部分不是「优化站位」，而是「诚实建模——一面大弯墙上，每台相机的画幅里到底真看到了哪些采样点；这些点够不够喂出每个箱体的 PnP（≥4 点/视图、≥8 点/箱体、≥2 视图）；相邻箱体能不能靠共同覆盖串成桥接链」。** 优化只是搭在这个可见性模型之上的一层。因此本设计把**逐采样点的可见性模型**当作一等核心组件，并让它的覆盖判据**直接对齐 `reconstruct.py` 的真实闸门常量**。

   > 桥接的真实约束见记忆 `project_sl_curved_wall_bridging_blocker`：非桥接箱体旋转初值=identity，60m 弧远端 ~90° 必发散，全屏前须补 transitive bridging。本工具不修重建端的桥接，但**必须在打分时把「断链」如实标出来**，让规划器在压不下去时报告而不是假装达标。

---

## 2. 目标 / 非目标

### 目标
- 输入 `project.yaml` 几何 + 相机内参（FOV/sensor），输出一份结构化采集 plan + 自包含 HTML 指导卡，标出每个推荐机位的位置、架高、拍摄朝向、覆盖的箱体、预测残差。
- 选机位走**思路3（完整版）**：确定性「菜谱」种子布局当起点，自由精修优化器在其上加/换/删机位，把每个箱体的预测残差压到目标线（默认 p95 ≤ 3.0mm）以下。优化器可挪动甚至删掉种子机位，不是只贴补丁。
- 打分器**逐采样点视野感知**：每个点只有真正看到它本身的相机才参与三角化；箱体覆盖/可重建判据对齐 `reconstruct.py` 真实闸门（≥4 点/视图、≥8 点/箱体、≥2 视图）；相邻箱体不共同覆盖 = 断链。压不下去的区域如实报告「超出当前管线能力，建议切弧段或补桥接」，而不是给乐观数字。
- 满足 `CLAUDE.md` CLI 维护契约：业务在 `lmt-app` / `python-sidecar`、CLI 子命令同步、E2E、`docs/agents-cli.md`、DTO 进 schema dump。

### 非目标
- **不做 GUI three.js 机位叠加**（v1 砍掉）。CapturePlan DTO 设计成 GUI 可消费，留作后续；本 spec 不实现 GUI 消费者。
- **不做任意障碍物网格 / 现场遮挡建模**。v1 物理约束只做「可达壳层」= 后退距离区间 + 架高区间（YAGNI）。
- **不修重建端的桥接（transitive bridging）**。本工具只**诊断并标注**断链，不解决它。
- **不做相机标定 / 不联合优化内参**。内参当已知输入，畸变 `dist_coeffs` 在规划阶段假设为 0（规划是几何指导，非 metrology）。
- **不做 Folded（折屏）**。`shape_prior=folded` 沿用现有 fail-fast（`nominal.py:81-85`）。v1 支持 Flat + Curved。
- **不输出绝对世界坐标**。机位坐标用模型系（原点在屏幕，跟 `nominal.py` 一致）；HTML 卡给相对屏幕的人话描述。

---

## 3. 整体架构 / 管线

```
project.yaml 几何 ──┐
                    ├─▶ ① 几何展开 ─▶ ② 逐点可见性模型 ─▶ ③ 视野感知打分器
相机内参(FOV/sensor)─┘                                          │
                                                               ▼
                        ⑤ 自由精修优化器 ◀── ④ 菜谱种子布局
                                  │
                                  ▼
                        ⑥ CapturePlan (结构化 plan)
                                  │
                                  ▼
                        ⑦ 自包含 HTML 指导卡 (CLI/agent 交付物)
```

代码落点（遵循双 transport 契约）：
- **规划引擎在 Python sidecar**：新 module `capture_planner.py`，复用 `nominal.py` + 改造后的 `sl_feasibility.py`。新子命令 `plan_capture`（`__main__.py` 三处表注册：parser / `SUBCOMMAND_MODULES` / `SUBCOMMAND_ENTRYPOINTS`）。
- **`lmt-app` 走 IPC 包一层**：新文件 `crates/lmt-app/src/capture_plan.rs`，`run_plan_capture(...)`，范式照 `visual.rs::run_reconstruct_structured_light`（构造 ipc 输入 → `block_on(sidecar_call)` → `map_vba_err`）。
- **HTML 渲染是纯函数，不碰 tauri**：放 `lmt-app`（或新建 `adapter-capture-guidance`），消费 CapturePlan DTO → HTML，对照 `adapter-total-station/instruction_card/html.rs`。
- **DTO 在 `lmt-shared`**：`CapturePlan` 等，派生 `serde + JsonSchema`，进 `schema::dump_all()`。

### 观测闸门常量（贯穿 ②③，单一真值）

| 常量 | 值 | 来源 | 语义 |
| --- | --- | --- | --- |
| `MIN_PNP_CORNERS` | 4 | `reconstruct.py:78` | 单台相机解一个箱体 PnP 的最少可见点 |
| `MIN_VIEWS` | 2 | `check_observability` | 一个箱体可重建的最少覆盖相机数（硬） |
| `MIN_POINTS_PER_CABINET` | 8 | `check_observability` | 一个箱体可重建的最少跨视图可见点总观测数（求和，硬） |
| `QUALITY_MIN_VIEWS` | 4 | `reconstruct.py:92` | 低于此（但 ≥2）标 `low_observation` |

这四个常量**镜像 `reconstruct.py` 的定义并注释「keep in sync」**，理想做法是 sidecar 内从同一处 import，避免两份漂移。它们**不作为 CLI/IPC 输入暴露**——改了就跟真实管线脱钩。

---

## 4. 组件详细设计

### ① 几何展开（复用 `nominal.py`）
- 输入 `ScreenConfig`（`cabinet_count` 网格 + `cabinet_size_mm` + `shape_prior`）。
- 用 `nominal_cabinet_centers_model_frame` + `nominal_cabinet_normals_model_frame` 拿每个箱体的 3D 中心 + 单位法线（模型系，米）。
- 每个箱体采一个 **`sample_grid`（默认 4×4 = 16 点）** 铺满整个活动面，作为可见性 + 残差评估点。采样点由箱体中心沿局部切向（切向 = 法线绕 +Y 的正交基）按网格偏移得到。
  - **采样密度由真实闸门倒推**：可见性 + 覆盖判据要拿 §3 闸门常量当真值，每箱体采样点数必须 ≥ 能表达「8 点/箱体、4 点/视图」的判定，所以不能只用 4 角 + 中心 5 点。`sample_grid` 是真实 SL dot 密度的**保守代理**（实际 dot 比这密 → 代理偏保守 = 不会高估覆盖）。
- 输出：`cabinets: [{col,row,center_m,normal,sample_points_m[K]}]` + 弧参数（radius / total_width / arc_span_deg，供 HTML 卡画弧）。

### ② 逐点可见性模型（新，核心）— `capture_planner.visibility`
给定机位 `(R,t)`（world→cam）、内参 `K`、画幅 `(W,H)`、一个采样点 `p`（及其所属箱体的单位法线 `normal`），判定该相机是否**有效看到该点**。逐点过四关，全过才算这个点可见：

| 关 | 检查 | 判据 |
| --- | --- | --- |
| (a) 前方 | cheirality | `(R @ p + t).z > 0` |
| (b) 入画幅 | 投影在传感器内（留边距 `margin_frac`，默认 0.05） | `project(p)` 落在 `[margin·W, (1-margin)·W] × [margin·H, (1-margin)·H]` |
| (c) 不掠射 | 法线对视线夹角 ≤ `incidence_max_deg`（默认 60°） | `angle(normal_cam, -view_dir) ≤ θ_max`，`view_dir = (p_cam)/‖‖`（同箱体法线恒定，但逐点视线不同 → 边缘点先掠射出局） |
| (d) 不自遮挡 | 弧面近端不挡远端（仅 Curved） | 沿相机→点 `p` 射线，检查是否被弧面近端遮挡；Flat 跳过 |

(d) 用轻量解析判据，不做完整光线-网格求交：Curved 屏是单调凸/凹圆柱段，「相机能否看到弧角 θ 的点」等价于「该点法线与视线夹角 < 90°（关 c 的强化）+ 视线不被弧面切线挡」。实现上用「点在相机视角下的极角单调性」筛掉被前缘遮住的远端点。**实现时先写逐点 a/b/c，(d) 作为弧墙专项在测试驱动下补**，避免一上来过度工程。

**从「点可见」聚合到「相机覆盖箱体」与「箱体可重建」（对齐 §3 闸门）**：
- `vis_count(cam, cab)` = 相机 `cam` 看到的箱体 `cab` 的采样点数。
- **相机 `cam` 覆盖箱体 `cab`** ⟺ `vis_count(cam, cab) ≥ MIN_PNP_CORNERS(4)`（这一台才解得出该箱体的 PnP）。
- **箱体 `cab` 可重建（硬闸门）** ⟺ 覆盖它的相机数 `≥ MIN_VIEWS(2)` **且** 跨覆盖相机的可见点**总观测数**（求和，非并集）`≥ MIN_POINTS_PER_CABINET(8)`。
- **质量降级标志** ⟺ 覆盖相机数 `< QUALITY_MIN_VIEWS(4)`（但 ≥2）→ 标 `low_observation`（对应 `reconstruct.py:108` 的语义）。

> **保守性说明**：真实 `check_observability` 的 views 是「≥1 观测的相机数」；本工具更严——要求每个计入的覆盖相机自身 `≥MIN_PNP_CORNERS` 个可见点（每个贡献视图都能独立 seed PnP）。比裸闸门保守 = 指导工具不会把临界配置判为达标，宁可让现场多架一台。

输出：
- `per_camera: {cam_idx: {visible_points: {(col,row): [point_idx...]}, cabinet_vis_count: {(col,row): int}}}`
- `per_cabinet: {(col,row): {covering_cams: [...], total_visible_points: int, reconstructable: bool, low_observation: bool}}`

### ③ 视野感知打分器 — 改造 `sl_feasibility.py`
保留现有 Monte-Carlo 真实链路骨架（投影→质心噪声→PnP-against-nominal→三角化→对真值算 3D 误差），插入逐点可见性：

1. **逐点筛相机（无箱体继承）**：采样点 `X` 只让「②判定看得到 `X` 这个点本身」的相机参与三角化。**不让点继承所属箱体的可见性**——否则中心入画幅、角点被裁/掠射的箱体会被当作整箱可见。
2. **点级覆盖判据**：看到 `X` 的相机 < 2 台 → `X` 标 `uncovered`，残差记 `inf`（不进 RMS，单列报告）。
3. **箱体级可重建闸门（对齐 `reconstruct.py`）**：箱体 `cab` 须 `reconstructable`（②的定义：覆盖相机 ≥2 且并集可见点 ≥8），否则整箱标 `under_observed`，其所有点不进 RMS。
4. **桥接判据**（箱体级）：构造箱体邻接图（网格 4-邻接）。两相邻箱体要「连得起来」，须**有 ≥1 台相机同时覆盖二者**（覆盖 = ②的 `≥MIN_PNP_CORNERS` 定义，不是「中心都入画幅」）。否则该边「断链」；整墙被断链切成多连通分量 → 报告每个分量 + 断边。
5. **PnP per view**：每台相机的 PnP 只用它**逐点可见**的那些点；某相机对某箱体可见点 `<MIN_PNP_CORNERS` 时它对该箱体不贡献 pose。
6. **输出逐箱体聚合**：`per_cabinet: {(col,row): {p95_mm, median_mm, n_views, reconstructable: bool, low_observation: bool, bridged: bool, pass: bool}}`，`pass = reconstructable and bridged and (p95_mm ≤ target)`；`low_observation` 不直接判 fail，但在卡片标黄警告。

> 噪声参数沿用 `feasibility_rms_mm` 的语义：`pixel_sigma`（默认 0.3px）、`nominal_deviation_mm`（默认 2.0）、`focal_err_frac`（默认 0.0）、`trials`（默认 30-50）。这些是「假设的现场条件」，作为输入暴露，echo 进 plan 的 settings。

### ④ 菜谱种子布局（思路1部分）— `capture_planner.seed`
确定性规则产出**人能照着站**的初始机位，全部落在可达壳层内：

- **后退距离**（standoff）：由 FOV 填充定——整面墙高（或宽，取约束更紧者）塞进画幅并留 `fill_margin`（默认画幅的 80%）。`standoff = (screen_extent/2) / tan(fov/2) / fill`。再 clamp 到 `[standoff_min, standoff_max]`。
- **扇形弧**：墙前拉一道由 `N_fan` 台相机组成的扇面（绕墙中心、半径 = standoff），张角让中段箱体的交会角 ≥ `min_triangulation_deg`（默认 ~15-20°）。`N_fan` 由墙宽 / 单机位可覆盖宽度估算。
- **弯墙顺弧排**：Curved 时机位排在与屏同心、半径 = `screen_radius + standoff` 的弧上，使每台都近似正对它负责的弧段（直接压低关 c 的掠射，也压低自遮挡）。
- **顶/底边专项**：为顶行、底行各加一圈俯/仰机位（架高取壳层上/下沿），专门救顶底边缘——这正是现场残差偏大的区域。

### ⑤ 自由精修优化器（思路2部分，种子热启动）— `capture_planner.optimize`
- **候选池**：在可达壳层（standoff 区间 × 架高区间 × 沿墙/弧的方位采样）上离散撒候选机位，每个候选朝向 = look-at 它正前方的墙段。
- **目标**：每个箱体 `pass`（reconstructable ∧ bridged ∧ p95 ≤ target）。
- **贪心**：从种子布局起，迭代——
  - 找当前**最差未达标箱体**（或断链边）；
  - 在候选池里选「能让全局未达标集下降最多」的机位**加入**；
  - 周期性尝试**删/换**：去掉某机位若仍全达标则删（精简），或用更优候选替换。
- **停机**：全达标 → 输出；或达到 `max_stations` / 候选池榨干仍有未达标 → 输出**带 `unreachable_regions` 的 plan**，明确标注「这些区域在给定壳层 + 内参下任何站位都不达标」并附建议（增大壳层 / 切弧段 / 补桥接）。
- 贪心 + 视野感知打分每步都要跑 Monte-Carlo，**性能注意**：种子已是好起点 → 迭代次数有限；打分 `trials` 在优化内循环用低值（如 15），最终 plan 用高值（如 50）复算一遍写进报告。

### ⑥ CapturePlan DTO（`lmt-shared`，派生 `JsonSchema`）
```
CapturePlan {
  screen: { screen_id, cabinet_count, cabinet_size_mm, shape_prior, arc: {radius_mm, span_deg}? }
  stations: [ CaptureStation {
    id: string,                    // "S01"...
    position_mm: [f64;3],          // 模型系
    look_at_mm: [f64;3],           // 朝向（看向墙上一点）
    standoff_mm: f64,              // 到墙面的法向距离
    height_mm: f64,                // 架高（模型系 Y）
    covers_cabinets: [[u32;2]],    // (col,row)，覆盖 = ②的 ≥MIN_PNP_CORNERS 定义
    note: string,                  // 人话："从墙中心后退 12.5m、架高 1.6m、对准左 1/3"
  }],
  coverage: CaptureCoverage {
    per_cabinet: [ CabinetCoverage {
      col,row, p95_residual_mm: f64, n_views: u32, visible_points: u32,
      reconstructable: bool, low_observation: bool, bridged: bool, pass: bool
    }],
    unreachable_regions: [ UnreachableRegion { cabinets: [[u32;2]], reason: string } ],
    all_pass: bool,
    target_p95_residual_mm: f64,
  },
  settings: CapturePlanSettings {  // echo 输入，便于复现
    intrinsics: {image_size, hfov_deg|vfov_deg, derived_K},
    shell: {standoff_min_mm, standoff_max_mm, height_min_mm, height_max_mm},
    sample_grid: [int,int],
    pixel_sigma_px, nominal_deviation_mm, focal_err_frac, incidence_max_deg, seed,
  }
}
```
引用 `lmt-core` 域类型的字段（若有）放 `schema::dump_all()` 的 `incomplete` 列表并写明原因；纯类型直接进 schema dump。

### ⑦ 自包含 HTML 指导卡（CLI/agent 交付物）
对照 `adapter-total-station/instruction_card/html.rs` 的纯函数范式（`CapturePlan → String`），**无外部依赖、无 CDN**（agent 可直接渲染、可打印）。内含三块：

1. **俯视平面图（inline SVG）**：墙的弧/直线轮廓 + 每个机位的位置点 + 朝向箭头 + standoff 标注。
2. **正视立面图（inline SVG）**：箱体网格，按 `per_cabinet` 状态热力着色（达标绿、`low_observation` 黄、`under_observed`/断链红）。
3. **逐机位清单（HTML 表）**：id / 后退距离 / 架高 / 朝向（人话）/ 覆盖箱体 / 该机位贡献。顶部放整体合格判定 + `unreachable_regions` 警告。

> **设计决策（待 review）**：v1 用 **2D 正交投影（俯视+立面）SVG** 表达 3D 布局，而非内嵌 WebGL 3D 场景——这样卡片自包含、零依赖、可打印，也是测量行业标准表达。交互式 3D 模型留给后续 GUI 叠加（已 deferred）消费同一份 DTO。**CJK 排版**遵循 `~/.claude/CLAUDE.md`：sans-serif 字体栈、行高放宽、`<strong>`/`<em>` 而非 markdown。

---

## 5. 数据契约（sidecar IPC）

**新子命令 `plan_capture`**，输入 pydantic 模型（`ipc.py` 新增 `PlanCaptureInput`）：
```
PlanCaptureInput {
  screen: { cabinet_array: CabinetArray, shape_prior }    // 复用现有类型
  intrinsics: { image_size: [w,h], hfov_deg?: float, vfov_deg?: float }
       // K 内部推导：fx=fy=(w/2)/tan(hfov/2)（给 hfov）或 (h/2)/tan(vfov/2)（给 vfov）；cx=w/2, cy=h/2
  shell: { standoff_min_mm, standoff_max_mm, height_min_mm, height_max_mm }
  target_p95_residual_mm: float = 3.0
  pixel_sigma_px: float = 0.3
  nominal_deviation_mm: float = 2.0
  focal_err_frac: float = 0.0
  incidence_max_deg: float = 60.0
  sample_grid: [int, int] = [4, 4]   // 每箱体采样点网格（§4①）；点数须 ≥ 表达 8点/箱体、4点/视图 闸门
  max_stations: int = 24
  seed: int = 0
}
```
§3 的观测闸门常量（`MIN_VIEWS / MIN_POINTS_PER_CABINET / MIN_PNP_CORNERS / QUALITY_MIN_VIEWS`）**不在输入里**——镜像 `reconstruct.py`，由 `capture_planner` 内部持有。

输出 = CapturePlan 的 JSON（§4⑥ 结构），由 `lmt-app` 反序列化成 Rust DTO。

---

## 6. CLI 契约交付项（CLAUDE.md 全套）

1. **lmt-app helper** — `crates/lmt-app/src/capture_plan.rs::run_plan_capture(project_path, screen_id, intrinsics_spec, shell, options) -> LmtResult<CapturePlan>`；可选 `render_html(&CapturePlan) -> String`。纯文件读 + sidecar 调用，不涉及 DB。
2. **Tauri shim** — `src-tauri/src/commands/` 新增 thin wrapper，只做 transport 翻译。
3. **CLI 子命令** — `crates/lmt-cli/src/cli.rs` 加 clap `plan-capture`；`crates/lmt-cli/src/commands/capture.rs` 实现。形如：
   ```
   lmt plan-capture --project <dir> --screen <id> \
       --image-size 3840x2160 --hfov-deg 50 \
       --standoff 4000..15000 --height 1200..3000 \
       [--target-mm 3.0] [--out plan.json] [--html card.html] \
       [--dry-run] [--json]
   ```
   - 写 `--out`/`--html`：非破坏性（新产物，无数据丢失，类比 export pose-obj），**不需要 `gate_destructive`**；但若目标已存在且无 `--force` → refuse（`output_exists`）。
   - `--dry-run`：跑规划 + 打印覆盖摘要，**不写任何文件**。
   - `--json`：只走 `output.rs::ok`/`err` envelope，不裸 println。
4. **CLI E2E** — `crates/lmt-cli/tests/cli_e2e.rs` 至少四类：
   - happy：合法 Flat 小墙 → 写出 card.html + plan.json，退出 0；
   - refuse：`--out` 已存在无 `--force` → `output_exists` envelope；
   - dry-run：跑通但断言文件未生成；
   - error envelope：非法几何（curved radius 过小，触发 `nominal._validate_curved_radius`）→ `invalid_input` envelope。
5. **docs/agents-cli.md** — 命令表加一行、side_effect 标注（writes-file）、必要时错误码表加 `output_exists`（若新分类，三处同步：`error_codes` 常量 + `exit_codes` 退出码 + 文档表）。
6. **DTO schemars** — `CapturePlan` 及子类型加 `JsonSchema`，进 `schema::dump_all()`。

> **Not exposed in CLI**：GUI three.js 机位叠加（⑦b，已 deferred）。替代路径：CLI `plan-capture --html` 出自包含 HTML 卡；后续 GUI 直接消费 `--out` 的 CapturePlan JSON。

---

## 7. 测试策略

- **逐点可见性单测**（Python）：正对墙→全点可见；侧 90°→不可见（cheirality/掠射）；远端箱体出画幅→关 b 挡掉；弯墙近端遮远端→关 d 挡掉（弧墙专项）。
- **逐点 vs 整箱护栏测试（核心断言）**：构造一个「中心入画幅、但角点被裁/掠射」的箱体 → 断言它**不被判整箱可见**，`vis_count < MIN_PNP_CORNERS` → 该相机不覆盖该箱体、`reconstructable=false`。防 cabinet-center 捷径回潮。
- **观测闸门对齐单测**：箱体仅 1 台覆盖相机 → `reconstructable=false`（min_views）；2 台但并集可见点 <8 → `reconstructable=false`（min_points）；覆盖相机 2–3 台 → `low_observation=true`；≥4 台且点够 → `pass` 候选。常量与 `reconstruct.py` 一致（若改成 import 则断言同源）。
- **打分器单测**：明显欠覆盖的机位 → 断言 `under_observed`/`unbridged` 被标出；满覆盖机位（每点都可见时）→ p95 与改造前数值一致（回归不破旧路径）。
- **种子布局单测**：给定墙宽/FOV → standoff 落在解析预期内；顶底专项机位存在且架高在壳层边界。
- **优化器单测**：种子已全达标 → 优化器零改动（幂等）；人为制造一个未覆盖箱体 → 优化器补一台机位后全达标；壳层故意收死 → 输出非空 `unreachable_regions`。
- **HTML 卡测试**（Rust，对照 `instruction_html_test.rs`）：渲染含已知机位的 plan → 断言 SVG 含对应站位标记、立面热力色、合格判定文案；CJK 字体栈存在。
- **CLI E2E**：§6.4 四类。
- **端到端 sanity**：拿一个已知合成台配方（弧半径已知的弯墙），跑 `plan-capture` → 人工核对推荐机位是否顺弧、顶底有专项机位、弧远端若超限是否如实报 unreachable（对照记忆 `feedback_verify_artifacts_against_known_good` 的「跟已知正确版 diff」纪律）。

---

## 8. 里程碑（建议拆 PR）

1. **M1 几何 + 逐点可见性 + 打分器（仅 a/b/c）**：`capture_planner` 的几何展开（含 `sample_grid` 采样）、**逐采样点**可见性（前三关）、对齐 §3 观测闸门的覆盖聚合、视野感知打分器 + 单测。**不含弧面自遮挡、不含优化器**。验收：能对一组给定机位输出逐箱体「可重建/低观测/断链/残差」报告；护栏测试（中心入画幅但角点裁切的箱体不被判整箱可见）通过。
2. **M2 种子布局 + 优化器**：菜谱种子 + 贪心精修 + unreachable 报告 + 单测。验收：常规 Flat/Curved 墙能自动出全达标 plan。
3. **M3 CLI + DTO + HTML 卡**：sidecar 子命令、lmt-app helper、Tauri shim、CLI 子命令、E2E、HTML 卡、docs、schema。验收：`lmt plan-capture` 全链路 + CLAUDE.md 自检命令全过。
4. **M4 弧面自遮挡（关 d）+ 强弯曲大墙打磨**：补可见性关 d，针对 60m 强弧验证断链报告正确。验收：强弧远端如实报 unreachable 而非乐观达标。

---

## 9. 风险 / 已知限制

- **打分器是代理而非真值**：`feasibility` 是「逐点独立三角化」的可行性代理，**不含 model-constrained BA / transitive bridging**。它能诚实标「断链/欠覆盖」，但对「桥接后 BA 能救回多少」是保守的。plan 的残差是**指导性预测**，最终仍以真实重建 + Phase 3 BA 为准。plan 输出须注明这点。
- **`sample_grid` 是 SL dot 密度的保守代理**：4×4 比真实 dot 稀 → 倾向于低估覆盖（保守，可接受）；但对极细的局部裁切可能不够细 → 密度作为参数可调，弧墙专项（M4）若发现漏判再调密。
- **(d) 自遮挡判据是解析近似**：对单调圆柱段有效；不规则 mask（`irregular_mask`）/ 极端凹弧可能不准 → M4 重点测，必要时降级为「保守标 uncertain」。
- **优化器是贪心非全局最优**：可能给出比理论最优多一两台机位的 plan。可接受（现场多架一台机位成本远低于漏覆盖）。
- **内参假设零畸变**：广角强畸变镜头的真实可见边界会偏离 → 文档提示用户用畸变较小的镜头 / 中心区域，或后续接入 `dist_coeffs`。
- **坐标系是模型系相对屏幕**：现场要把「相对屏幕中心后退 X」落到真实地面，需用户自己找屏幕中心参照。这是指导工具（非 metrology）的固有边界，文档写清。

---

## 10. 开放问题（实施前确认）

- 无阻塞性开放问题。HTML 卡「2D 正交投影 vs 内嵌 3D」（§4⑦ 设计决策）请 review 时确认；其余细节按本 spec 自行拍板。
