# 精准模式（Precision Screen Scan）设计 · 压系统误差的箱体位姿精修

> 日期：2026-06-04
> 范围：在现有结构光 Path B 之上，新增"精准模式"——**同一刚性箱体模型、同一 BA、同一
> gauge，把每块箱体的位姿精度往系统误差的极限再压一档**。不解箱体内部形变、不出稠密 UV、
> 不上 Gray+Phase。

---

## 1. 背景与定位

### 1.1 精准模式不是什么

源头是一份外部双模式文档（`快速模式` = 稀疏编码点；`精准模式` = Gray Code + Phase Shift
稠密 UV + 子箱体形变）。经核验，该文档的"快速模式"项目早已实现且更优（时序闪码绕开 ArUco
容量墙），"统一观测 + 统一 solver"也早是现状（`reconstruct.py` 与 `sl_reconstruct.py`
共用 `Observation` + `solve_and_emit` + `model_constrained_ba`）。文档真正补的空白只有
"稠密 + 子箱体形变"那一块。

**但本次明确不做那一块。** 用户拍板：精准模式停在**刚性箱体级**——每块箱体仍当规则平板/
弧面（`shape_prior` flat/curved），只把它的**整体位姿（在哪/朝哪/接缝）算得更准**；
**不**还原箱体内部的鼓包/扭曲/垂坠，**不**输出逐像素 UV→XYZ。

### 1.2 核心论点：刚性箱体的精度本底是系统误差

对刚性箱体，位姿（6-DOF）用现在每箱约 64 个点（8×8 闪码点阵）已经超定。再加密点阵只能
按 √N 压**随机噪声**（64→1024 约 4 倍），但位姿精度的**本底是系统误差**——相机内参
（focal ≲2%、主点须锁死）、镜头畸变、屏幕是否真按设计 pitch 做 1:1 映射。这些**再多点也
抵消不掉**（见 `2026-05-30-sl-coordinate-system-frame-design.md` §中"√N 只压随机噪声；
内参是系统性误差"）。

**因此精准模式的"更准"必须攻系统误差，不是攻密度。** 这条决定了下面所有设计：不上稠密图案
（对刚性位姿无增益且引入新 codec / 帧同步成本），改为收紧相机模型与采集几何。

### 1.3 精准模式 = 配方，不是开关

精度杠杆分散在 4 个独立子命令（`plan-capture` / `decode-structured-light` /
`calibrate-structured-light` / `reconstruct-structured-light`），没有单一命令能让一个
`--mode` 级联。所以精准模式是**一串可组合命令的推荐配方 + 严格更好的项设为默认**，符合项目
"暴露可组合命令"的哲学。将来真要一键封装是个薄 wrapper（YAGNI，本期不做）。

---

## 2. 目标与成功判据

**目标**：在合成 known-good 上，精准配方的**逐箱位姿误差可测地低于快速模式**，且增益**可归因
到 focal/畸变误差下降**（非过拟合）；全程不改几何模型 / solver / gauge。

成功判据（全部以合成 known-good 验，符合项目"synthetic 是最好情况"）：

- **P1 内参收紧**：精准配方解出的 focal 误差 < 1%（参照：方向2 PoC 自标定 focal < 0.12%
  端到端过 nominal；2% 是 nominal 通过的地板）。主点协方差在门槛内。
- **P2 位姿更准**：同一合成场景，精准配方逐箱位姿误差 **< 快速模式**，差距可统计显著。
- **P3 增益可归因**：把精准配方的内参替换成"真值内参"后，位姿误差不再进一步明显下降——证明
  精准配方已逼近内参允许的地板，剩余误差是 pitch/1:1 等**不修只验**的底，不是图案密度问题。
- **P4 不退步**：所有现有 SL / charuco / decode / calibrate 测试保持绿（默认项升级不破回归）。
- **P5 可观测分两类信号**（修订）：pitch/1:1 误差按"能否被 K 吸收"分两条暴露路径——
  ① **刚体不可吸收类**（全局各向同性 pitch scale、整体 shape 偏差）→ 现有 `align_to_nominal`
  的 `procrustes_align_rms_m` 超阈值即报；② **可被 K 吸收类**（各向异性 sx≠sy、平滑 remap）→
  靠 §A.1.3 的**独立内参交叉校验**报（事后残差 warning 对这类**没信号**，因为吸收把 residual
  压低了）。focal/主点协方差出现在结果 envelope。
- **P6 反吸收硬门（no silent wrong，三类 × 各自的 guard·对应 Codex critical）**：合成台**注入三类
  pitch/1:1 误差**，每类由**对应的** guard 在导出前抓住——
  - **(a) 各向同性 scale** → 重建端 `procrustes_align_rms_m` 超阈 `nominal_misfit` warning
    （测试在 **Plan 3**，`test_nominal_misfit_warns_on_global_scale`）；
  - **(b) 各向异性 sx≠sy** → 交叉校验 **aspect** 项（有锚点 → refuse）；
  - **(c) 平滑非线性 remap** → 交叉校验 **畸变量级**项（**不是 focal/aspect**，remap 的非线性落在
    k1,k2,k3）（有锚点 → refuse）。(b)(c) 测试在 **Plan 1** `test_pitch_absorption_guard`。
  - 每类断言**注入量超过对应阈值** + guard ON 在写出前 refuse/warning（无文件）+ control（不注入）放行,
    证明 refuse 是误差引起、不是别的。**注入量必须高于交叉校验阈值**（aspect>1%、focal>2%、畸变量级>容差），
    否则护栏不该触发、测试不成立（Codex high "#2"）。这是 spec 的 no-ship 红线。

---

## 3. 架构

**一句话**：精准模式**不是新管线、不是新 solver**，而是在现有 SL 重建路径上叠一套高保险配置
+ 一项小新建（拍屏自标定内联）。`Observation`（`model_constrained_ba.py`）、
`model_constrained_ba`、`solve_and_emit`（`reconstruct.py`）、`align_to_nominal` gauge
（`sl_reconstruct.py`）、两阶段离群剔除、凹凸消歧、`export pose-obj`——**全部不动**。精度
全部来自喂给这个不变的 solver 一个**更好的相机模型 + 更干净的观测**。

**关键利好（核实）**：拍屏自标定的引擎**已存在且测过**——`calibrate-structured-light`
（`calibrate_sl.py` → `lmt_app::visual::run_calibrate_structured_light`，用 nominal 设计墙
当 3D 靶反解内参，`test_calibrate_sl` 守住）。精准模式 v1 不是从零造自标定，而是把它**升级一档
+ 保证它用的就是这次拍屏的帧**。风险大头落在"扩展已测代码 + 配置"。

### 3.1 杠杆地图

| # | 杠杆 | 落点 | 性质 | 期 |
| --- | --- | --- | --- | --- |
| L1 | 内参精修（更全畸变 + 帧匹配自标定 + **独立交叉校验防吸收**） | `calibrate-structured-light` / `reconstruct-structured-light` | 扩展 + 内联 + 护栏 | P1 |
| L2 | 亚像素升级（强度加权质心）— **独立变更，精准不依赖** | `decode-structured-light` `_seed_dots` | 独立 gate | P1 |
| L3 | 采集精准档（更严覆盖/视差，使 L1 可观测） | `plan-capture` | 配置 | P1 |
| L4 | 验收与系统误差可观测（pitch/1:1 两类信号） | `reconstruct` 结果 + `compare-known` | 配置/可观测 | P1 |
| L5 | BA 内联合精修畸变 | `reconstruct-structured-light` | 合成台 gate 后实验 | P2 |

> 注：L1 的"更全畸变"与"防吸收交叉校验"是**绑定**的一件事，不是两件——放开高阶畸变的前提是
> 独立锚点在场（§A.1.1 反吸收约束 + §A.1.3）。

---

## Part A · Phase 1 五杠杆详设（v1，现在交付）

### A.1 L1 内参精修

**A.1.1 更全畸变模型（`calibrate_sl.py`）**

现状（核实）：`calibrate_sl.py:183` 用 `flags = CALIB_USE_INTRINSIC_GUESS |
CALIB_ZERO_TANGENT_DIST | CALIB_FIX_K3`——**只解 radial k1,k2**，切向清零、k3 固定。

改：**按 observability 自适应放开**畸变——视角/视差足够时去掉这两个 flag、解
k1,k2,k3 + p1,p2；不够时退回 k1,k2（不硬解无观测支撑的高阶项，否则畸变 DOF 乱跑、比不加更差）。
判据走标定的协方差 / 条件数内部常量，**不开 CLI flag**（同项目"Otsu 自适应"做法）。BA 侧
`_undistort_obs`（`reconstruct.py:230`）走 `cv2.undistortPoints(pts, K, dist)`，dist
多长都吃——**重建侧零改动**。

**反吸收约束（对应 Codex high）**：放开 k3+切向**同时也放大了"屏侧平滑 remap 被畸变吸收"
的口子**（见 §A.1.3）。因此高阶畸变只在 `--intrinsics auto` **且独立交叉校验通过**时才放开；
纯 `auto`、无独立锚点、又是平面墙时**强制退回 k1,k2**（宁可畸变模型保守，也不让它替屏侧误差
背锅）。这条把"更全畸变"与"防吸收"绑在一起，不是两件独立的事。

**A.1.2 帧匹配自标定 `--intrinsics auto`（内联）**

现状：`reconstruct-structured-light --intrinsics <path>` 必填（`cli.rs`
`ReconstructStructuredLight.intrinsics: String`），需先单独跑 `calibrate-structured-light`
得 K 再传入。若标定用了**别的会话/棋盘格/别的帧**，会引入"标定时 vs 拍屏时镜头状态不一致"
（对焦/呼吸/温度漂移）这一隐藏系统误差。

改：`--intrinsics` 接受保留字 **`auto`**。`auto` 时 `run_reconstruct_structured_light`
**内联调用同一自标定引擎**（A.1.1 升级版），用**这次重建的同一组 `--corr`** 解内参 → 冻结
→ 喂现有 BA。构造上保证标定帧 == 重建帧，消掉镜头状态不一致。

**安全性 — 只覆盖一条吸收通道，另一条要 §A.1.3 兜**：自标定子问题把每箱体当 **nominal 平面靶**
解 K + per-pose 外参，**不碰箱体间 as-built 几何** → 焦距吸不进 **cabinet 形变**（这条通道堵住）。
但它**堵不住另一条通道**：屏侧的 pitch/1:1 误差（各向异性 sx≠sy、平滑 remap）可被 **K 本身**
（fx/fy 比、畸变系数）吸收，产出低 residual 但 metric 错的结果——因为标定靶（屏）和被测对象
（屏）是同一个，共用同一个 pitch/1:1 假设，假设错了会进 K 而不是暴露。**这条必须由 §A.1.3 的
独立交叉校验兜**，事后残差检查对它没信号。**注意**：单平面靶解不开 focal/主点二义，靠 L3 的
斜视角/视差破——故 L3 不是可选项。

**A.1.3 独立内参交叉校验（防 pitch/1:1 被吸进 K，对应 Codex high）**

`auto` 自标定的 K 是"屏侧假设 + 真实相机"耦合的解，单凭它分不开二者。引入一个**吸不进屏侧误差
的独立锚点**做交叉校验：
- **锚点来源**（取其一）：①现有棋盘格 `visual calibrate`（与屏无关的物理靶，最干净）；②一个
  **focal 先验 + 容差**（厂商标称 / 镜头档案）；③多视角的几何自洽冗余（cross-view consistency，
  屏侧系统误差在不同机位会留下不一致痕迹）。
- **判据（三项都比，缺一漏类·对应 Codex critical）**：`auto` 解出的 K 与独立锚点比**三项**——
  ① **focal**（`|fx−afx|/afx`，抓各向同性 scale 被吸进焦距）；② **fx/fy aspect**（抓各向异性
  sx≠sy）；③ **畸变量级**（比 `dist_coeffs` 向量 / 到画面角的径向位移曲线，抓**平滑非线性 remap**——
  这类的非线性部分落在 k1,k2,k3、**根本不动 focal/aspect**，只比前两项必漏 remap 类）。任一项超
  容差 → `WarningEvent` + 在写出前 `observability_failed`。**平面墙 + 无任何独立锚点**时，`auto`
  **拒绝声称精准**（不静默给可能 metric 错的结果）。
- **弯墙更稳、平面墙最危**：弯墙 nominal 是非共面靶，本就压住 focal/scale 二义，吸收空间小；
  平面墙共面、最易把屏侧 scale 混进 focal/畸变——所以护栏对平面墙最严。

**A.1.4 实测发现：共面（平面）墙自标定的畸变不可观测（2026-06-04 执行 Plan 1 时实证）**

落地 `--intrinsics auto` 时实测确认了一个 A.1.3 已预警的现象，需要写明现场含义与设计：

- **现象**：**共面（平面）靶解不出畸变**——畸变与平面单应纠缠。平面墙 + 像素噪声下，自标定会把
  k1,k2 **过拟合到噪声**（实测：0.1px 噪声 → 画面角等效位移 ~15px）。此时交叉校验的"畸变量级"项
  **正确地拒绝**了它（验证了护栏设计有效）。无噪声合成靶不过拟合（畸变 ≈ 0），故 Plan 1 的 auto
  测试用平面墙 + **noise-free** + anchor 通过。
- **现场含义（安全但受限）**：现场平面墙 + `--intrinsics auto` 很可能**经常被拒**——这是**安全**的
  （拒绝胜于交付 metric 错的墙），但意味着**平面墙要精准，不能纯靠 auto**。
- **设计（推荐·把"固定畸变为零"改正为"固定为 anchor 畸变"）**：共面靶**根本观测不到**畸变，所以
  "把畸变固定为零"是错的（真镜头有畸变，固定为零会把镜头畸变留成系统误差）。正确做法是
  **共面靶 + anchor 时，把畸变固定为 anchor 的 `dist_coeffs`**（anchor 本就是畸变权威），自标定只解
  focal/pp。这样：① 平面墙 auto **能用而非总被拒**；② 不再把畸变过拟合到噪声；③ 交叉校验仍管
  focal/aspect（畸变项因构造相等而恒过，但共面靶下畸变本就不可独立校验，由 anchor 兜）。
  - **落地**：`solve_sl_intrinsics` 加可选 `fixed_dist`（用 `CALIB_FIX_K1..K3|FIX_TANGENT` + `dist0=fixed_dist`
    只解 K）；calibrate / auto 在 `coplanar_ratio < 阈值` **且** anchor 在场时传 `fixed_dist=anchor_dist`。
  - **范围**：**这是 Plan 1 的推荐增量（一个小 TDD 任务），不阻塞 v1**——当前"平面墙噪声自标定→拒绝"
    的行为安全，可先交付；该增量把平面墙 auto 从"总被拒"提升到"可用"。弯墙路径不受影响（非共面，
    畸变可观测）。

### A.2 L2 亚像素升级（独立变更·精准不依赖它）

**定位（修订，对应 Codex medium）**：这条**不在精准模式关键路径上**——精准精度靠 L1 压系统
误差，这点亚像素只压随机噪声。且它改的是**共享 decode 默认**，快速模式一并受影响。故按**独立
变更、独立 gate** 处理，精准模式的成功判据不依赖它。

现状：`sl_decode.py` `_seed_dots`（约 :177-198）用 Otsu 连通域**质心**。候选改为**强度加权
质心**（高斯拟合因饱和平顶偏置风险，留作 bench 候选不作默认）。

**回退路径 + 真实验收门（不加永久 CLI flag）**：
- Otsu 实现**保留在代码里**，默认值集中一处，field 回归时一行可切回——不删旧路径。
- **promotion 门升级**：只有当强度加权质心在 **field-like fixtures**（LED bloom、饱和平顶、
  glare、粘连连通域）+ **端到端逐箱位姿误差**上**≥ Otsu**，才设为默认；只在干净合成亚像素真值
  上更好**不够**。达不到 → 保持 Otsu 默认（或仅作显式实验路径），**不静默 default**。
- corr.json 格式不变（仍 `{id,u,v,x,y}`，仅 `x,y` 更准）。

### A.3 L3 采集精准档（复用 planner）

`plan-capture`（`run_plan_capture`，`cli.rs:457`）加精准参数：`--min-views <N>`（默认仍 2，
精准传 3）+ **更小 `--target-mm`**。**视差/斜视角约束不是新增硬门**（修订·对应 Codex high
"#4 部分"）——实测核验：planner 的 Monte-Carlo p95 残差**已经**按 ~1/sin(parallax) 惩罚
（20mm 基线机位对 → p95≈269mm → pass=False；只有宽基线才过 3mm 目标），候选池 azimuth 已铺
−70°..70°。所以精准档"要视差"靠**更小 target-mm 把 p95 门收紧** + min_views，**不另造冗余视差
门**（YAGNI）。唯一要补的是**可观测性**：给"按数量够（reconstructable）但因低视差/p95 失败"的
箱体加一条**诊断**，区分"视差/几何不足"还是"覆盖不足"，否则它落进 `unreachable_regions` 看不出
原因。**这仍是 L1 高阶畸变 + focal 可观测的前提**（平面墙正面拍解不稳，靠斜视角破）。

### A.4 L4 验收与系统误差可观测

- **协方差上浮**：`run_reconstruct_structured_light` 把自标定的 `focal_stddev_px` /
  `pp_stddev_px` + 选用的 `distortion_model` 填进 `VisualReconstructResult`（自标定够不够稳
  一眼可见）。
- **pitch/1:1 两类信号（只验不解；修订，对应 Codex high）**：分两条互补路径，**不能只靠事后
  残差**——
  - **刚体不可吸收类**（全局各向同性 scale、整体 shape 偏差）：现有 `align_to_nominal` 的
    `procrustes_align_rms_m` 超阈值即报（均匀缩放/形变进不了刚体配准，残差会顶高）。设阈值 +
    超阈发 `WarningEvent` 指明"疑似 pitch scale / shape 偏差"。
  - **可被 K 吸收类**（各向异性 sx≠sy、平滑 remap）：靠 §A.1.3 的**独立内参交叉校验**报——
    事后残差对这类**没信号**（吸收把它压低了）。这是与上一版的关键区别：原"残差结构 warning"
    单独**挡不住吸收类**。
  - 两类都**只验不解**——不去"解尺度"（与"屏是尺子"冲突），只把它**变成导出前的明确信号**。
- **`compare-known` 紧阈值**：现 size≤2.0/dist≤3.0/angle≤0.3° 固定，加可覆盖
  `--max-size-mm` / `--max-dist-mm` / `--max-angle-deg`，精准档传更严值。

---

## Part B · Phase 2 实验门（合成台 gate 后才做）

### B.1 L5 BA 内联合精修畸变

**假设**：在重建 BA 里联合精修畸变（用全部 as-built 观测，而非只用 nominal 靶标定）能把位姿
压过"冻结 K"的地板。

**合成台协议**（扩 `visual simulate` 注入真实畸变；用 `visual eval` 比对）：已知 as-built 几何
+ 已知带真实畸变的相机 → 跑两版：(a) 冻结自标定 K；(b) 冻结 K + BA 内精修畸变（**主点全程
锁死**，敏感度结论）。比逐箱位姿误差 vs 真值。

**纳入判据（双条件，缺一不可）**：
1. (b) 逐箱位姿误差 < (a)，统计显著；
2. **注入一个焦距误差，断言它没被吸成假的箱体鼓曲**（正是 `2026-05-24-camera-branch-rework`
   警告的"K 全自由 → 焦距/畸变被解释成 cabinet 变形"失效模式）——即 (b) 的箱体几何误差不被抬高。

过了 → 出 `reconstruct-structured-light --refine-distortion`（默认关、精准 recipe 里开），
按六步契约补齐。**不过 → 不建，并把这个地板老实写进 `agents-cli.md` + 本 spec。**

---

## 4. CLI 契约落地（Phase 1）

| 杠杆 | lmt-app helper | CLI | DTO/schema | E2E |
| --- | --- | --- | --- | --- |
| L1 畸变 | `run_calibrate_structured_light`（visual.rs:279）自适应畸变，**高阶项仅在交叉校验通过时放开** | 无新 flag | `CalibrateResult`（dto.rs:308）加 `distortion_model` + `focal_stddev_px`/`pp_stddev_px`（当前仅 `reproj_error_px`，stddev 只在写出的 JSON、未进 envelope DTO） | calibrate full vs fallback 两 case |
| L1 自标定 | `run_reconstruct_structured_light`（visual.rs:235）`intrinsics: Path\|"auto"` | `--intrinsics auto`（保留 `<path>`，`auto` 为保留字）；可选 `--intrinsics-crosscheck <path>`（独立锚点 K，或 focal 先验文件） | `VisualReconstructResult` 加 `intrinsics_source`(provided/auto) + `focal_stddev_px`/`pp_stddev_px`/`distortion_model` | `--intrinsics auto` happy/refuse/dry-run/envelope |
| **L1 防吸收（新·Codex high）** | `run_reconstruct_structured_light` 内：`auto` K 与独立锚点交叉校验；超容差发 warning；平面墙+无锚点 → 拒绝精准 | 见上 `--intrinsics-crosscheck`（缺省时走 focal 先验/cross-view 一致性） | `VisualReconstructResult` 加 `intrinsics_crosscheck`(`{source, focal_dev, pp_dev, passed}`) | `auto` 无锚点+平面墙 → `observability_failed`；注入屏侧误差 → warning |
| L2 亚像素（独立·精准不依赖） | `run_decode_structured_light`（visual.rs:602）`_seed_dots` 强度加权质心，**Otsu 保留为回退默认** | 无新 flag | 无（corr.json 格式不变） | 现有 decode 全绿（回归门）+ **field-like fixtures + 端到端位姿门** |
| L3 采集 | `run_plan_capture`（visual.rs:789）加 `min_views` + 视差约束 | `plan-capture --min-views <N>`（+ 复用 `--target-mm`） | `CapturePlan` coverage 加 min-views 达标标记 | 精准参数 → 断言更严覆盖 |
| L4 验收 | `run_reconstruct_structured_light` 上浮协方差 + **pitch/1:1 两类信号**（procrustes 阈值 + 交叉校验）；`run_compare_known`（visual.rs:703）阈值可覆盖 | `compare-known --max-size-mm/--max-dist-mm/--max-angle-deg` | 见 L1 自标定行的 stddev + crosscheck 字段；`CompareKnownResult` 不变 | 协方差在 envelope；紧阈值 pass/fail |

**错误码**：Phase 1 **不新增**。自适应畸变退化、自标定欠观测、**平面墙 `auto` 无独立锚点
（防吸收拒绝）** → 复用 `observability_failed`(17)；内参非法 → `intrinsics_invalid`(16)；
`auto` 与 `<path>` 冲突 / corr < 2 等 → `invalid_input`(2/3)。三处错误码常量无改动。

**Tauri shim**：`visual` 全组 CLI-only（`agents-cli.md` "Not exposed in the GUI"），精准模式
新增项**不加 shim**，在该段补一句说明即可。

**DTO schemars**：新字段加进 `VisualReconstructResult` / `CalibrateResult`（均已注册
`JsonSchema`，新字段自动进 dump），`lmt --json schema | jq` 验证。

---

## 5. 测试与验收

**sidecar pytest**
- `test_calibrate_sl`：加 full-distortion（够视角解 k3+切向）与 fallback（少视角退 k1,k2）两 case，断言 `distortion_model` 正确 + 协方差填充。
- `test_sl_decode`（亚像素，**门升级 · Codex medium**）：除干净合成真值外，加 **field-like fixtures**——LED bloom、饱和平顶、glare、粘连连通域——断言强度加权质心在这些上**端到端逐箱位姿误差 ≥ Otsu**（不只比 centroid 误差）；达不到则 Otsu 保持默认。现有 decode case 全绿（回归）。
- `test_reconstruct`（或新 `test_reconstruct_sl_auto`）：`--intrinsics auto` 内联自标定路径走通。
- **`test_pitch_absorption_guard`（P6 · Codex high · no-ship 红线）**：合成台**注入三类屏侧误差**——(a) 各向同性 pitch scale、(b) 各向异性 sx≠sy、(c) 平滑非线性 remap，`auto` 全程：
  - **开护栏**：断言 (a) 触发 `procrustes_align_rms_m` 超阈 warning；(b)(c) 触发独立交叉校验 warning / 平面墙无锚点时 `observability_failed`——**导出前都报出来**；
  - **关护栏对照**：断言 (b)(c) 被静默吸进 K（低 residual 但 metric 错）——证明护栏确实在挡、不是摆设。
- 改完 `build_exe.sh` 重建 binary。

**CLI E2E（`cli_e2e.rs`）**
- `reconstruct_sl_intrinsics_auto_happy` / `_refuse` / `_dry_run`：`--intrinsics auto` 四类。
- `reconstruct_sl_auto_flatwall_no_anchor_refuses`：平面墙 `auto` 无独立锚点 → `observability_failed`（防吸收拒绝）。
- `compare_known_tight_thresholds`：紧阈值 pass/fail。
- `plan_capture_precision_min_views`：`--min-views 3` 断言更严覆盖。
- 既有 SL/calibrate/decode E2E 不变（回归门）。

**合成台（`visual simulate` + `visual eval`）**
- 扩 `simulate` 注入真实畸变 **+ 三类屏侧 pitch/1:1 误差**；`eval` 比"快速 vs 精准配方"逐箱位姿误差 → 验 P1–P3，注入屏侧误差 → 验 P6。
- P3 关键：替换真值内参后位姿不再明显下降 = 已逼近内参地板。

**老实写地板**：pitch/1:1 假设误差 + 残余系统内参误差是精准模式**也修不掉**的底——但**必须可观测、不可静默吸收**（P5 两类信号 + P6 反吸收门）。L4 负责报，不假装能解。

---

## 6. 范围外（明确不做）

- **箱体内部形变**（鼓包/扭曲/垂坠/B-spline/per-module）——保持刚性箱体，形变是另一里程碑。
- **稠密 UV→XYZ 输出 / Gray Code / Phase Shift / 逐像素重建**——对刚性位姿无增益，引入新 codec
  + 帧同步成本，本期不做。
- **scale bar / 控制点 / 全站仪 / world datum / Sim(3) 校尺度**——与"屏是尺子"（pitch 定尺度）
  冲突；单相机绝对世界定位超范围。
- **主 BA 无约束放开 K**——项目明令的坑（焦距被解释成 cabinet 变形）；L5 仅在合成台双条件 gate
  通过后、且主点锁死下才引入畸变精修。
- **lens/tracking/timing 联合标定（CameraGeoCal/TemporalCal）**——屏幕反算范围外。
- **pitch/1:1 尺度求解**——只验不解，但**验必须可观测、不可静默吸收**：两类信号
  （procrustes 阈值 + 独立交叉校验，§A.1.3 / §A.4 / P5 / P6），不是单靠事后残差 warning。
- **一键 `--mode precision` 封装**——精准模式是配方，YAGNI。

---

## 7. 交付清单（任一缺失视为未完成）

**Phase 1**
1. `calibrate_sl.py` 自适应畸变（observability 放开 k3+切向 / 退回 k1,k2）+ 协方差/条件数门；
   **高阶项仅在 §A.1.3 交叉校验在场时放开**，否则退 k1,k2（反吸收约束）。
2. `reconstruct-structured-light --intrinsics auto`：`cli.rs` 保留字解析（`auto` vs path，
   冲突 → `invalid_input`）+ `run_reconstruct_structured_light` 内联自标定（同 `--corr`）。
3. **防吸收交叉校验（Codex high）**：`--intrinsics-crosscheck <path>`（独立锚点 K / focal 先验），
   缺省走 cross-view 一致性；`auto` K 与锚点超容差 → `WarningEvent`；**平面墙 + 无任何锚点 →
   `observability_failed`，拒绝声称精准**（不静默给 metric 错的结果）。
4. `sl_decode.py` `_seed_dots` 强度加权质心，**Otsu 保留为回退默认**；只有过 field-like + 端到端
   位姿门才设默认（独立变更，精准不依赖）。
5. `plan-capture --min-views` + 视差约束（`run_plan_capture` + `cli.rs`）。
6. `compare-known --max-size-mm/--max-dist-mm/--max-angle-deg`（`run_compare_known` + `cli.rs`）。
7. `VisualReconstructResult` 加 `intrinsics_source` + `focal_stddev_px`/`pp_stddev_px`/
   `distortion_model` + **`intrinsics_crosscheck`**；`CalibrateResult` 加 `distortion_model` +
   stddev；进 `schema::dump_all()`。
8. pitch/1:1 **两类信号**：① procrustes 阈值 warning（刚体不可吸收类）；② 交叉校验 warning
   （可吸收类）。消息指明"pitch 非位姿"。
9. sidecar pytest（calibrate full/fallback、decode 亚像素 field fixtures、auto 路径、
   **`test_pitch_absorption_guard` 三类注入 P6**）+ 重建 binary。
10. CLI E2E（auto 四类、**平面墙无锚点拒绝**、compare-known 紧阈值、plan-capture min-views）。
11. `agents-cli.md`：`reconstruct-structured-light` 行加 `--intrinsics auto` / `--intrinsics-crosscheck`
    说明、`calibrate` / `compare-known` / `plan-capture` 行补新 flag、"Not exposed in GUI" 段补一句；
    新 DTO 字段写进文档。
12. 合成台 `simulate` 注畸变 **+ 三类屏侧 pitch/1:1 误差** + `eval` 验 P1–P3 + **P6 反吸收门**。

**Phase 2（合成台 gate 后）**
13. L5 合成台双条件实验（位姿更准 + 焦距不被吸成假鼓曲）；过 → `--refine-distortion`（默认关）
    全六步契约；不过 → 文档写明地板、不建。

---

## 自检命令（合并前）

```bash
cargo test --workspace                                          # 全测试(含 CLI E2E)
.venv/bin/python -m pytest python-sidecar/tests                 # sidecar(含合成台验收)
./target/debug/lmt --json schema | jq '.VisualReconstructResult'  # 新 DTO 字段进 dump
./target/debug/lmt visual reconstruct-structured-light --help   # --intrinsics auto 文档
./target/debug/lmt visual compare-known --help                  # 紧阈值 flag
./target/debug/lmt visual plan-capture --help                   # --min-views
```
