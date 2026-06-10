# 核心问题修复执行清单

> 日期：2026-06-10（基于 main @ 6d20d18 的全仓算法审查，完整报告：`_walkthrough/algo-review-2026-06-10.md`）
> 用途：供后续 Claude Code session 直接领任务执行修复。每个 FIX 自包含：证据位置、根因、修复方案、验收标准、陷阱。
> **行号声明**：所有 file:line 为 2026-06-10 时点，执行时以符号名/grep 重新定位为准，不要盲信行号。
> **核验等级**：[已核验] = 审查时主审亲自读码或数值复算确认；[机制核验] = 代码机制确认、数值幅度来自子 agent 仿真——执行前先花十分钟复跑确认再动手。

## 全局执行规范（每个 FIX 都适用）

1. **顺序**：FIX-1 + FIX-2 必须同一批次（同一 API 改造）→ FIX-3 → FIX-4 → FIX-9/10a（为前面的修复提供验收手段）→ 其余可并行。补充组（五～九，FIX-15 起）整体后置于一～四组，例外：FIX-19/20 建议随相邻修复顺手做；FIX-15～17 在 capture planner 被实际投入使用前必须完成（否则它输出错误指导）。
2. **遵守仓库 CLAUDE.md 契约**：业务逻辑只写 `crates/lmt-app` / `python-sidecar`；凡触及 DTO/CLI 行为的，六件套同步（lmt-app helper / Tauri shim / CLI 子命令 / E2E / `docs/agents-cli.md` / schemars）。
3. **Surgical changes**：只改与该 FIX 直接相关的行，不顺手重构。
4. **自检**：`cargo test --workspace` + `python-sidecar/.venv/bin/python -m pytest python-sidecar/tests`。
5. **合成验证铁律**（历史教训，违反必产生假绿）：
   - 渲染用 proper 3D 投影，**禁止图像 warp**（手性 artifact 前车之鉴）；
   - 相机必须在 y-up 局部帧的 **+z 一侧**（screen_mapping.py 自身有此警告）；
   - 弯墙/多行用例必须 **rows ≥ 2**（现 17 处测试 fixture 全 rows=1，是 FIX-2 长期隐身的原因）；
   - 网格重建测试必须**断言非锚点顶点**（现测试只验锚点复现，是 FIX-11 长期隐身的原因）。

---

## 一、最致命的问题群：坐标系账本（P0，先修）

### FIX-1 曲面墙名义法线是镜像的 [已核验 · critical]

- **位置**：`python-sidecar/src/lmt_vba_sidecar/nominal.py` `_cabinet_normal_model`（~:92-120）返回 `[sin a, 0, cos a]`。
- **根因**：弧线 x=R·sin a, z=R(1−cos a) 的切线是 [cos a, 0, sin a]；该"法线"与切线点积 = sin 2a ≠ 0，不垂直于自己的弧面。正确法线 = R_y(−a)·ẑ = **[−sin a, 0, cos a]**——同文件 `_cabinet_R_y_model`（~:162-188）已实现正确约定并在 docstring 自证（R_y(+a) 会让相邻 tile 开缝）。
- **受影响消费者**（全部继承镜像）：
  1. `reconstruct.py` `estimate_nonroot_cabinet_init` 的 IPPE 凹凸两支消歧（~:1349-1351）——Δ≥4°（margin 8°）时**系统性选中镜像支**，更小弧度 hard-abort "undecidable"；
  2. `reconstruct.py` `_nominal_world_corners`（~:315-333）——从法线 atan2 反推出 R_y(+a)，SL align_to_nominal 的 Procrustes 目标角点全部倾错；
  3. `capture_planner/geometry.py`(~:65) / `visibility.py` / `scoring.py`——弯墙可见性判反（正对箱体 0/16 可见点，镜像掠射位 16/16）。
- **修复方案**：单一真源。把 `_cabinet_R_y_model` 升级为公共 API（如 `nominal_cabinet_poses_model_frame` → dict[(col,row)] → (R, t)），法线一律由 `R @ [0,0,1]` 派生；删除 `_cabinet_normal_model` 的独立公式；`_nominal_world_corners` 直接消费 R，不再从法线反推。
- **验收**：
  - 数学不变量测试：任意 curved prior 下 `normal · tangent == 0`；相邻 tile 共享边重合（无缝）；
  - 弯墙 IPPE 消歧 e2e：proper projection 渲染的凹墙（相机在 +z 侧），消歧选中真支而非镜像支。
- **陷阱**：现有消歧测试用"R_y(+35°) 真值 + 镜像合成世界"钉死了错误约定——**改完代码这些测试会红，属预期；改测试，不要改回代码**。

### FIX-2 y-down 中心网格与 y-up 局部帧混用 [已核验 · critical]

- **位置**：`nominal.py` 的中心网格 `(row+0.5)·ch` 是 +y-DOWN（其 docstring 自述），标定靶组装处显式取负 local y（~:227, :273）；但 `reconstruct.py` 三处**不翻转**直接消费同一批 centers：
  1. 非桥接 init seed `t = nominal_m[cr] − nominal_m[root]`（~:876-877）；
  2. `_pnp_camera` fallback 相机初值合成（~:1399-1401）；
  3. `_nominal_world_corners`（~:333）把 y-up 的 corners_local 与 y-down centers 相加 → rows>1 时 align_to_nominal 的 Procrustes 目标**非刚性**（跨箱体要求翻 y、箱体内要求不翻）。
  另：`reconstruct.py:326` docstring 声称两边 "Y=rows-up" 与 `nominal.py` 的 "+y DOWN" 直接矛盾。
- **修复方案**：与 FIX-1 同一批次做。统一模型帧约定（建议 y-up），nominal 的 SE(3) API 输出即为统一帧，消灭所有散落的手写 ±y；**同时裁决 row-0 语义**（rust core `boundary_interp.rs`/`uv.rs` 注释说 row 0 = 屏底，`pattern.py` 把 row 0 摆 canvas 顶——全链一处声明、处处遵守，不一致处改注释或改代码并写明）。
- **验收**：rows≥2 的多行平墙 + 多行弯墙合成 e2e（proper projection），align_to_nominal 残差 < 0.1mm；init seed 与真值 y 同号断言。
- **陷阱**：17 处测试 fixture 全 rows=1，必须先加 rows≥2 用例再改，否则修没修对不可见。

### FIX-3 无传递桥接 + fallback 初值 identity 旋转且未旋入 root 帧 [已核验 · critical@目标场景]

- **位置**：`reconstruct.py` `estimate_nonroot_cabinet_init`（~:1260-1292，docstring 自认 "only direct root↔non-root bridging…Current target is the monitor bench"）；fallback `init = (np.eye(3), t_mm)`（~:869-879）；帧修正只给了 bridge 路径的法线（~:1300-1314），fallback 没享受。
- **后果**：几十米墙分段拍摄时几乎无箱体与角落 root (0,0) 共视 → 全部落入"错帧 nominal 平移 + identity 旋转"初值；长弧远端差至 ~90°，BA 发散或落镜像局部极小。三条路径（charuco/vpqsp/SL）共享此代码。
- **修复方案**：
  1. fallback 旋转改为 `R_root_nominalᵀ @ R_y(−a_cab)`（用 FIX-1 的 SE(3) API），平移同样旋入 root 帧；
  2. 桥接加传递组合：以共视关系建图（任意两 cabinet 同帧共视即连边），BFS 从 root 链式组合 `world_from_cab`，不再要求与 root 直连；
  3. 建议（低成本高收益）：gauge/root 改选**墙中心**箱体而非 (0,0)（减半链长与漂移）——注意 ROOT_CABINET 常量被 report/frame 语义引用，需同步 FrameSpec 与文档。
  4. 落 fallback 的箱体发 WarningEvent（现在是静默的）。
- **验收**：合成长弧（如 24×3 cabinets、弧跨 ~90°）+ 分段相机（每机只见 4–6 箱体），BA 收敛且**逐角点 3D 误差**达标（用逐点误差，不用会撒谎的聚合指标——见 FIX-9）。

### FIX-4 converged 硬编码 + 验收门双条件 [已核验 · major]

- **位置**：`reconstruct.py` ResultEvent 中 `converged=True` 硬编码（~:1064）；fatal 门 `if not result.converged and result.rms > 2.0`（~:898）——必须同时满足才拒绝。
- **修复方案**：`converged=bool(result.converged)` 如实上报；门改为：not converged → fatal；converged 但 rms > 阈值（默认 2.0px）→ fatal（如需逃生门再加显式 flag，默认严格）。
- **验收**：e2e 用 max_nfev=1 强制不收敛 → 非零退出 + `ba_diverged`；高 RMS 收敛解 → 拒绝。错误码若新增需三处同步（envelope/exit_codes/agents-cli.md）。

---

## 二、作用域缺口

### FIX-5 标定旋转多样性门把 roll 计入 [机制核验 · critical —— 执行前先复跑仿真]

- **位置**：`intrinsics_solve.py` `_max_pairwise_rot_deg`（~:56-64）用 trace 求总相对转角，含光轴 roll；平面靶下纯 roll 视图组是 Zhang 退化（焦距不可观测）却能过 5° 门。子 agent 仿真：横移+roll 捕获过全部门，fx 偏 107% 而 formal stddev 仅 0.09%。VP-QSP `--intrinsics auto`（默认路径）依赖此门。
- **同组**：legacy `calibrate.py` `_has_pose_diversity`（~:37-64）量的是**像素平移**不是旋转（仿真 fx+541% 穿透），且无任何协方差门——它还是 crosscheck anchor 的天然来源。
- **修复方案**：
  1. 门改测**视轴夹角**：`arccos(Rrel[2,2])`（剔除 roll 分量），阈值仍 5°（或实测后定）；
  2. legacy calibrate 二选一：并入共享 `solve_sl_intrinsics`（获得全部协方差门），或显式退役其 anchor 资格（文档 + 代码注释）。
- **验收**：先复跑仿真确认 107%/541% 场景 → 修后：纯 roll+平移捕获集 refuse（observability_failed）；含 ≥15° 俯仰/偏航多样性的捕获通过；原有合法捕获不被误拒（回归）。

### FIX-6 VP-QSP 渲染 blit 与 BA 标称坐标的半像素系统偏差 [已核验 · major]

- **位置**：`vpqsp_layout.py` 渲染 `x0 = int(round(cx)) − marker_px//2`（~:263-266）vs `marker_local_mm` 按连续 `(mc+0.5)·cell` 计算（~:146-180）。偏差 ≤0.83 LED px（round 误差 ±0.5 + odd marker_px 的常量 +0.5），折 0.5–1.6mm 系统误差。
- **修复方案**：方案 a（推荐，零渲染风险）：`marker_local_mm` 改为按**实际 blit 后中心**计算（复现渲染端的 round 逻辑）；渲染保持不变。注意 pattern_meta 变化 → `pattern_hash` 变化，旧 pattern 与新代码失配是**预期行为**（hash 门会拦住），CHANGELOG 写明。
- **同类顺手项**：`pattern.py` `_assemble_screen` 的 `(rw−tw)//2` floor 居中偏差（charuco 路径，≤0.5 LED px）同方式修。
- **验收**：新增 render→detect 端到端测试：理想光照下检测中心 vs `marker_local_mm` 推算中心的**系统偏差** < 0.05 LED px（现状 0.5–0.83）。现有 reconstruct 测试 monkeypatch 掉检测器，端到端用例是新增不是改造。

### FIX-7 默认方法拒绝 P2.3–P3.9 主流箱体 [已核验 · major · 决策项]

- **位置**：`vpqsp_layout.py` `MIN_CELL_PX=80`（~:57）+ `vpqsp_pattern.py` `MIN_MARKERS_PER_CABINET=8`（~:40, 94-103）。500mm 箱体：P3.9(128px)→1 marker、P2.9–P2.3(168–216px)→4，全拒；P1.9(≈264px) 起才可用。
- **事实**：生成门比 runtime 门严——`check_observability` 的 min_points=8 按**跨视图观测总数**计，4 markers × 2 views = 8 obs 已满足 runtime。
- **修复方案**（组合）：生成门与 runtime 对齐（≥4 markers 放行 + 输出"需 ≥2 视图"warning）；`docs/agents-cli.md` 与设计文档标注各 pitch 的适用边界；P3.9 以下（1–2 markers）维持拒绝并在错误信息中指引走 SL 路径。
- **验收**：P2.6@500mm 生成成功 + 2 视图 e2e 重建通过；P3.9 拒绝信息含 SL 指引。

### FIX-8 检测模糊悬崖未表征 + inverted 差分是死代码 [机制核验 · major]

- **位置**：`vpqsp_detect.py` 全局 Otsu + 外轮廓四边形前端（~:57-79, 178-201）；子 agent 实测 56px marker σ=3 模糊从 33/36 崩到 2/36。`detect_markers_image` 的 `inverted` 参数无任何生产调用方（生成器不输出 inverted 帧）。
- **修复方案**：
  1. 先加表征测试（σ=1..4 模糊 × marker 尺寸矩阵）入 CI，把包络钉下来并写进文档（最低相机像素/marker 需求）；
  2. 评估局部自适应阈值（CLAHE / adaptiveThreshold）替代全局 Otsu——以 1 的表征矩阵做 before/after；
  3. inverted 二选一：兑现（generate-pattern 输出 inverted 帧 + manifest 字段 + reconstruct 传入 + 六件套）或删除参数并从设计文档移除该声明。
- **验收**：表征矩阵入 CI；inverted 路径"有调用方"或"不存在"，不允许继续半死不活。

---

## 三、验证体系测不到的真问题

### FIX-9 holdout 指标接线（先于一切重建修复的验收依赖）[已核验 · critical]

- **位置**：`evaluate.py` `gauge_invariant_metrics`（~:26-57）只比 center/normal/size——箱体绕法线 roll 10°（角点偏 ~60mm）得分 0.0，全体法线共转同样 0。诚实的 `se3_aligned_holdout_rms`（同文件 ~:60-85）**只有单测调用**。
- **修复方案**：`eval_runner`/`eval_cmd` 增加逐角点 SE(3)-holdout RMS/p95 为 headline 指标（对齐集与打分集 disjoint，函数已约定）；`EvalResultEvent` schema 扩展 → lmt-shared DTO + schemars + adapter + CLI 六件套同步；顺手修 `seeds` 字段如实上报（现回显整个 seed_matrix 却只评单 seed）、补发被丢弃的 `rms_size_error_mm`。
- **验收**：注入 10° roll 的合成重建，新 headline 指标显著非零（旧指标 0.0 的场景成为回归用例）。

### FIX-10 eval 覆盖生产方法与现实条件 [已核验 · major · 可拆三步]

- **位置**：`eval_runner.run_method`（~:68-73）只有 charuco/free_point；init 给近真值；`simulate.py` 无 FOV clipping（~:87-95）、"弯墙"是原地旋转的扇形非弧（~:44-71）、outlier 是真值上加零均值高斯（~:103-107）。
- **拆解**：
  - **10a（优先，FIX-3 的验收依赖）**：cold-init 模式（走生产 PnP+桥接路径而非真值 init）+ FOV clipping + 真弧形墙摆位 + 沿墙相机轨迹；
  - **10b**：vpqsp 图像级全链（render → detect → reconstruct）进 eval；
  - **10c**：SL 路径接入；outlier 注入改 mis-association 型（错 ID 落在别的 marker 真投影处）。
- **验收**：10a 能复现 FIX-3 修复前的发散（负对照）并验证修复后收敛（正对照）。

---

## 四、Rust 几何核心与导出链

### FIX-11 RadialBasisReconstructor 米级错误 + dispatch 遮蔽 [已核验·复算 · critical]

- **位置**：`crates/core/src/reconstruct/radial_basis.rs`（IMQ ε=1.5、无多项式项、直接插值绝对世界坐标）。主审复算：60×10 墙 top+bottom 两行+3 中点锚点 → 平均 2.25m / 最差 13.6m 顶点误差；4×4 纯上下行也 0.9m；却报 `max(input σ, 8mm)`。`mod.rs` dispatch（~:26-38）把纯 top+bottom 捕获路由进 RBF（任何非角点边锚点都算 "interior"），`boundary_interp` 生产不可达——与注释声称相反。
- **修复方案**：
  1. RBF 改为**插值 residual-from-nominal**（推荐：先按 nominal/shape_prior 生成基准面，RBF 只插残差——残差小且局部，衰减核无害）；或加线性多项式尾项（RBF + [1,c,r] affine，标准做法，需扩增线性系统）；
  2. dispatch 修 `applicable`："interior" 定义排除边行列，让纯 top+bottom 走 `boundary_interp`；
  3. 复算脚本场景固化为 Rust 测试：**非锚点顶点**误差断言（现有测试只验锚点复现）。
- **验收**：60×10 top+bottom+3mid 场景 max 顶点误差 < 5mm（现状 13.6m）；top+bottom-only 输入路由到 boundary_interp。

### FIX-12 质量指标真实化 [机制核验 · major]

- **位置**：`uncertainty.rs`（~:21-37）+ 各 reconstructor：`estimated_rms_mm` 是输入 σ 均值非拟合残差，再 clamp 任意常数；`estimated_p95_mm`/`shape_fit_rms_mm` 全仓库无赋值却持久化入 DB；`ScatterOutlier.residual_mm` 硬编码 0.0；scatter 路径用噪声 min/max 定屏幕范围（已知 `total_size_mm` 只做 ±1 箱体 sanity check）；圆柱拟合硬编码竖直轴 + 固定 50mm inlier 带。
- **修复方案**：① RMS/p95 改为对实际输入点的拟合残差统计；算不出的字段从 schema 删除而不是留 0（DTO 变更走六件套）；② scatter 范围改为"已知网格按 u/θ 最小二乘平移配准"替代 min/max；③ 圆柱轴自由化（或文档化限制 + 倾斜检测拒绝）；④ grid 路径加邻距/先验外点检查（相邻顶点间距 ≈ cabinet 尺寸的硬约束现成可用）。
- **验收**：注入单点 50mm 外点 → 报告非零 outlier/residual（现状全 0）；范围配准对边缘缺测稳健（删掉最外列测量点，total_size 误差 < 10mm）。

### FIX-13 导出链四处 [机制核验 · major]

- **位置**：`crates/lmt-app/src/export.rs`。
  1. `--split --target disguise` 跳过全部 disguise 补偿（~:237-294），而 fix_root 帧→disguise 含 det=−1 反射（自家注释 ~:499-502）——split 出的逐箱体 OBJ 镜像手性，disguise 内刚体摆位救不回；
  2. `target=unreal` 被 parse 放行但零适配（米级 neutral 帧直出，~:155-311）——core `export/adapt.rs` 已有 unreal 轴/单位定义，pose-obj 没接；
  3. pose-obj UV 假设均匀 cols×rows（~:554-595），与非均匀 `input_rect_px` 布局矛盾；
  4. `measured.yaml` 死端：`MAIN_` 前缀 + 0-based 箱体中心与 core 重建器 1-based 角点名永不兼容，却会备份后**覆盖 M1 全站仪数据**（`visual.rs` ~:198-211），BA 协方差只活在死端里。
- **修复方案**：① split 与合并路径共用同一变换源（帧变换收敛到 `adapt_to_target` 单一出口）；② unreal 接 adapt 或 CLI 显式拒绝（择一，不许假出口）；③ pose-obj 接受可选 screen_mapping 生成非均匀 UV；④ visual 持久化停止覆盖 M1 文件（独立文件名或删除 M2 写出），逐箱体协方差迁移到 pose report schema。均涉 CLI 行为 → 六件套 + `docs/agents-cli.md` 的 `--split`/target 行为说明（现完全缺失）。
- **验收**：split 出的单箱体 OBJ 与合并导出中对应箱体逐顶点一致（容差内）；unreal 要么出正确 cm/左手系文件要么 exit 2；M1 measured.yaml 在 visual 重建后保持不变（e2e）。

### FIX-14 DPX 10-bit ingest 截断 [机制核验 · 现 minor / 精密模式前置 major]

- **位置**：`dpx.py` `>>2` 截到 8-bit（截断非舍入，引入 ~3/1023 向下偏置）；transfer characteristic 头字节（offset 801）不校验。
- **修复方案**：保留 10-bit（输出 uint16 或 float 路径，下游 sl_decode 的质心/Otsu 适配位深）；transfer characteristic ≠ linear 时按本文件"不支持变体必须 ValueError"哲学拒绝。
- **验收**：合成 10-bit DPX 渐变帧 → 解码强度无 4× 量化台阶；log 编码 DPX → 干净报错。

---

## 五、采集规划器（capture planner —— 投入使用前必修，否则输出错误指导）

### FIX-15 候选机位全部瞄墙中心：宽墙边缘箱体结构性不可覆盖 [已核验 · critical@planner]

- **位置**：`capture_planner/optimize.py` `candidate_cameras`（~:22-37）——pool 只按 (standoff, height, azimuth) 参数化，所有候选 `look_at_camera(K, pos, center)` 一律瞄墙中心；`seed.py`（~:60-74）fan/top/bottom 同样只瞄 x=cx。
- **后果**：任何超出"中心视锥足迹"的箱体对**所有**候选都出画，与机位预算无关；planner 随后把自己的候选空间退化误报为物理不可达（`unreachable_regions` + "raise shell / split arc" 误导性补救文案）。子 agent 实测：15m 墙全部 49 个候选 0 可见点，而一个平凡的对准相机 16/16。
- **修复方案**：候选按 (position, aim-target) 双参数化——aim-target 在墙面分区采样（如每 N 列一个区中心 + 全墙中心）；seed 的 fan 同步。pool 规模上升 → 配合 FIX-18 的向量化。
- **验收**：15–30m 平墙 + 标准 FOV，所有箱体可被至少一个候选覆盖；unreachable 报告仅在物理遮挡/shell 约束下出现。

### FIX-16 自遮挡判定：绝对 t-epsilon 撞 mm 级矢高 + 无限圆柱无高度范围 [机制核验 · major]

- **位置**：`capture_planner/visibility.py` `_arc_occludes`——交点判据 `1e-4 < t < 1−1e-3`（段长比例），而箱体外侧采样点天然落在弧圆柱后方 u²/2R（500mm 箱体 @R=8m ≈ 3.9mm），命中 t ≈ 1−(0.0005..0.003)，**正好骑在 1e-3 阈值上**——是否"自遮挡"取决于站距而非几何。遮挡体还是无限高圆柱（无 y 范围），从墙上/墙下越过的视线被误判遮挡。
- **修复方案**：判据改为**米制容差**（交点到目标点的 3D 距离 > 例如 20mm 才算遮挡），并给圆柱加墙体 y 范围裁剪。
- **验收**：浅弧墙正面站位在不同 standoff（3m/5m/8m）下遮挡判定结果一致；越顶/越底视线不再误报。

### FIX-17 贪心目标无连续进度信号 + MC 误差模型与生产估计器不符 [机制核验 · major]

- **位置**：`optimize.py` `_score` = (failing_count, view_deficit) 二元组——对 fail_reason=low_parallax 的箱体，一个显著改善 p95 但未跨线的候选**两个分量都不变**，无候选严格改进 → 循环 break，预算还剩也报 unreachable；`scoring.py`（~:37-103）的 p95 预测用 i.i.d. 逐点噪声 + SQPnP+DLT，而真实误差是箱体/弧级系统性偏差、生产估计器是 IPPE+模型约束 Huber BA——预测的是没人运行的估计器。
- **修复方案**：① 目标加第三分量：连续 p95-excess（Σ max(0, p95−target)），死停消失；② 噪声模型加箱体级相关扰动项（整箱平移/旋转抖动）；③（可选，成本高收益大）小规模合成上直接跑生产 BA 替代 proxy 估计器。
- **验收**：构造"可达但需 3 个宽基线机位"的合成场景，优化器不再死停于 unreachable；预测 p95 与生产 BA 实测 p95 的偏差有界（±50% 内即可，方向一致最重要）。

### FIX-18 规划器性能：纯 Python 全量重算 [机制核验 · 规模到 1000+ 箱体时 major]

- **位置**：每轮贪心对每个候选 `score_screen` 全量重算（coverage 双重计算、已选相机的 MC 重复求解、`point_visible` 标量循环）。子 agent 实测 320 箱体单次 2.8s → 设计目标墙 + FIX-15 扩大的 pool ≈ 多小时。
- **修复方案**：可见性向量化（相机×点批量矩阵运算）；已选相机的观测/PnP 结果跨候选缓存；coverage 与 `sees` 合并为一次遍历。
- **验收**：320 箱体场景单候选评分 < 0.2s（≈15×）；优化结果与重构前一致（回归）。

---

## 六、BA 统计与数值

### FIX-19 协方差三连错 [已核验 · major]

- **位置**：`model_constrained_ba.py`（~:119-131）+ `reconstruct.py`（~:989-993, 1029-1032）。
  1. **huber 混算**：`cov = pinv(JᵀJ) · Σfun²/dof`——loss="huber" 时 scipy 的 `sol.jac` 是稳健加权后的 J，而 `sol.fun` 是原始残差，二者混算导致协方差缩放不自洽；
  2. **align 后未旋转**：`align_to_nominal` 路径 center/normal/corners 都乘了 `align_r`，但挂到同一 MeasuredPoint 的 3×3 平移协方差**没有做 R Σ Rᵀ**——不确定度椭球留在旧帧；
  3. **>2400 参数静默退化**：`MAX_COVARIANCE_PARAMS=2400`（≈400 相机+箱体）以上直接跳过，全部箱体退化为 5mm 各向同性占位——恰是大墙最需要真协方差的场景。
- **修复方案**：① σ² 用稳健加权残差（或对 inlier 子集做最终一次 linear-loss 求解再取协方差）；② align 分支补 `align_r @ cov @ align_r.T`；③ 大规模场景用稀疏求解（JᵀJ 的块对角逆 / Schur 补只取所需 3×3 块），去掉 dense pinv 上限。
- **验收**：合成已知噪声场景，报告协方差与蒙特卡洛经验协方差一致（迹比 0.5–2×）；align 旋转 90° 的用例下椭球主轴跟着转。

### FIX-20 VP-QSP 自标定假设全部视图同帧尺寸 [已核验 · major]

- **位置**：`reconstruct.py` `_first_image_size`（~:506-514）取第一张可读图定 image_size，`_self_calibrate_vpqsp` 全部视图共用——混入不同分辨率/横竖画幅时主点原点与 coverage 归一直接错，且静默。**SL 路径已有该校验**（`sl_reconstruct.py:90-92` 要求所有 corr 的 camera_image_size 一致），vpqsp 路径漏了。
- **修复方案**：照搬 SL 的校验——遍历全部视图首帧，尺寸不一致 → `invalid_input` 明确报错。
- **验收**：混合分辨率 manifest → exit 2 + 可读错误信息（e2e 用例）。

### FIX-21 自标定 pose 准入过松 + pose 数爆炸 [机制核验 · major]

- **位置**：`reconstruct.py` `_self_calibrate_vpqsp`（~:551-569）每 (view, cabinet) 一个 calibrateCamera pose、≥4 点即准入——4 点平面 pose 仅 8 约束对 6 外参 DOF，高方差；pose 数 = views × cabinets，40 视图 × 50 箱体 ≈ 2000 poses 的稠密 LM 进入分钟～小时级。
- **修复方案**：准入提至 ≥8–12 点；pose 数超阈值（如 200）时按"共面墙段"合并分组或均匀抽样。
- **验收**：现有自标定 e2e 不回归；合成 40×50 场景求解 < 60s。

### FIX-22 legacy ba.py 基线不公平 [已核验 · minor]

- **位置**：`ba.py:110-117` 的 `least_squares` 无 `loss`（线性，对注入外点零鲁棒）也无 `x_scale="jac"`（model_constrained_ba.py:108-110 自己写明缺它会"microscopic steps and stall"），max_nfev=100。free_point 基线"结构性更差"的 Phase-0 结论部分是**优化器配置差**而非模型差。
- **修复方案**：补齐 `loss="huber", x_scale="jac"`，重跑 eval 对比一次并存档结论（模型优势预计仍在，但数字会变）。
- **验收**：eval 重跑报告归档；若 free_point 差距显著缩小，在相关文档更新表述。

### FIX-23 求解性能：向量化 + 解析 Jacobian + 热启动 [已核验 · major · Phase 2 前置]

- **位置**：`model_constrained_ba._residuals` 纯 Python 逐观测循环、无解析 jac（trf 有限差分）；`stage_b_robust_solve` 每轮修剪后从**同一冷初值**重解（~:236-239 重传 init_cameras/init_cabinets）；`_undistort_obs` 每角点一次 cv2 调用。
- **修复方案**：① 残差向量化（按相机/箱体分组批量矩阵运算）；② 该投影模型的解析 Jacobian 是标准推导，实现后供 `jac=` 参数；③ Stage B 各轮以上一轮解热启动；④ undistortPoints 按图像批量调用。精密模式（稠密观测）依赖此项，详见 `docs/precision-scan-improvement-plan.md` §3.3。
- **验收**：数值等价回归（与现实现的解在 1e-6 内一致）+ 100 箱体 × 20 视图合成场景端到端 < 30s。

---

## 七、检测与标定鲁棒性细项

### FIX-24 _order_corners 在 ~45° roll 处退化 [已核验 · minor]

- **位置**：`vpqsp_detect.py:46-54`——TL=argmin(x+y)、TR=argmin(y−x)：菱形朝向（45° roll）时两者数学上必选同一顶点（例 (0,−1),(1,0),(0,1),(−1,0)：sum 与 diff 的 argmin 都落在 (0,−1)），单应退化 → decode 失败带（子 agent 实测恰在 45° 从 36 掉到 22）。
- **修复方案**：改用质心极角排序（atan2 排序后以最高点起始旋转对齐），对任意 roll 稳定。
- **验收**：0–90° roll 扫描测试，检出率无凹陷。

### FIX-25 质心观测无逐点不确定度：异方差进 BA [机制核验 · major]

- **位置**：`vpqsp_detect.py` 质心 = 背景扣除后的强度矩，无饱和/bloom 检查、无斜视偏置模型；所有观测进 BA **等权**（`Observation` 无 confidence 字段），远/斜/小 marker 与近/正/大同权。检测器自家测试预算显示 20° 斜视已 ~0.8px 中值误差。
- **修复方案**：① 检测端输出逐点质量（blob SNR、尺度、饱和像素占比）；② `Observation` 加 optional `sigma_px`，残差按 1/σ 加权（同步落实 Part C §30 承诺的 confidence/covariance 字段——精密模式同样需要，见 roadmap 修订 §1.3）；③ 饱和占比超阈值的 dot 降权或剔除。
- **验收**：合成异方差场景（近 5 marker 清晰 + 远 50 marker 模糊）：加权后位姿误差显著优于等权（量化为 ≥30% 改善）。

### FIX-26 ChArUco 检测无重复 id / 假阳性过滤，参数全默认 [已核验(结构) · major]

- **位置**：`detect.py:102,152` 全默认 `DetectorParameters()`（为印刷板调的参数，不适配 LED 的摩尔纹/像素网格/bloom）；id→bucket 直接 append（~:120-122），重复 id / 背景假阳性 marker 一并喂给 `interpolateCornersCharuco`（其角点种子无 RANSAC），单个假 marker 可拉偏整箱角点。
- **修复方案**：① 同 (cabinet, id) 重复检出按角点质量留一或整组弃用；② marker 周长/面积界限 + 单应一致性预滤；③ DetectorParameters 针对发光面板调优（adaptive threshold 窗口、corner refinement=CORNER_REFINE_SUBPIX）并以 FIX-8 的表征矩阵验证。
- **验收**：注入背景 DICT_6X6 假阳性 + 重复 id 的合成图，角点输出无污染（与干净图差 < 0.1px）。

### FIX-27 单 pose 标定准入过松 + preflight(image_size) 概念错误 API [已核验(后者) · minor]

- **位置**：`intrinsics_solve.py`——coplanarity 比 >1e-3 即算"3D 靶"放行单 pose（10m 宽墙 10mm 浮雕就够），且旋转多样性门 `len(rvecs)>=2` 条件下单 pose 永不触发，接受边界对操作员不可解释；`screen_mapping.preflight(image_size=...)` 把相机帧尺寸与 LED 画布分辨率比较——概念错误，`reconstruct.py:391-395` 已刻意绕开但 API 留存，未来调用者按 docstring 用必踩坑。
- **修复方案**：① 单 pose 准入改为显式几何判据（JᵀJ 在焦距方向的条件数/可观测性检查，替代 coplanarity 比例 proxy），refuse 信息说明"需要什么样的额外 pose"；② preflight 删除 image_size 参数（或改为仅 warning + docstring 改写为真实语义）。
- **验收**：单 pose 浅浮雕墙 refuse 且错误信息含补拍指引；preflight 无误导参数。

---

## 八、IPC / 采集链鲁棒性

### FIX-28 NDJSON 协议脆弱：杂行废全程、fatal 标志被无视 [已核验 · major]

- **位置**：`adapter-visual-ba/src/sidecar.rs:65` `serde_json::from_str(&line)?`——任何第三方 Python 库往 stdout 打的一行、或未来新增的 event tag，都让已成功的多分钟 BA 整体报废；协议定义了 `fatal: bool`（Rust ipc.rs 与 Python ipc.py 两侧都有）但 Rust 把一切 error event 当 fatal 处理，非致命通道实际不存在。
- **修复方案**：① 非 JSON 行：跳过 + 记入 stderr-tail 式诊断（计数超限才失败）；② 未知 event tag：容忍并告警（serde untagged fallback 或先解 `{"event": …}` 再分发）；③ error event 按 `fatal` 字段分流，非致命转 warning 收集。
- **验收**：sidecar 故意混入打印行/未知 event/非致命 error 的 e2e：运行成功且诊断可见；fatal error 仍立刻失败。

### FIX-29 pose report 回读静默吞错 [机制核验 · minor]

- **位置**：`adapter-visual-ba/src/api.rs` `read_cabinet_summaries` 对损坏/缺失 report 一律返回空 Vec，整体仍 success——用户无从区分"report 没写出来"与"没有 summary"，而 report 是后续 pose-obj 导出的唯一输入。
- **修复方案**：读不到/解析失败 → 显式 warning（带路径与原因）入 result；重建成功但 report 缺失视为降级而非正常。
- **验收**：删掉/损坏 report 的 e2e → result 带 warning，CLI envelope 可见。

### FIX-30 帧目录 ingest 卫生 [已核验 · minor]

- **位置**：`sl_decode.py:31-45,72`——`_read_frame_file` 直接返回 `cv2.imread` 结果（损坏文件 → None 静默入列，远处 `np.stack` 才以无关 TypeError 崩）；`_IMG_EXTS` 与 .dpx 一起按文件名排序收集——目录里的缩略图/本工具自产 `*.debug.png` 会被当序列帧插入，错乱时序索引表现为难排查的 decode_failed。
- **修复方案**：① imread 返回 None 立即 ValueError（带文件名）；② 单目录内强制单一扩展名（混合 → 报错并列出意外文件），或显式排除 `*.debug.*` 等已知自产物。
- **验收**：损坏文件 → 指名报错；混入 debug.png 的 .dpx 目录 → 干净拒绝而非错位解码。

---

## 九、Rust 几何细项与文档

### FIX-31 平面帧方向退化无警告：方形屏可转置、近水平屏 up 不稳定 [已核验(机制) · major]

- **位置**：`crates/core/src/reconstruct/surface_fit/project.rs:77-89`——列/行轴按"投影范围比最接近 cols:rows"分配，方形屏（cols≈rows）近乎平票，噪声可使网格转置 90° 且无警告；up 钉定 `if v_dir.z < 0.0` 在 v_dir.z≈0（转置情形/地屏天幕）时由噪声决定。±normal 朝向歧义已有 warning 覆盖，这两个没有。
- **修复方案**：比值平票区间（如 ratio 差 <15%）→ 发 warning 并要求显式方向输入；v_dir.z 近零 → 改用更稳的参考（如重力先验或用户指定 up），同样告警。
- **验收**：10×8 方形屏 + 边缘缺测合成数据：要么方向正确要么显式警告，不允许静默转置。

### FIX-32 文档现实标注（纯文档任务）[minor]

- **内容**：① `docs/LED_ScreenModelCal_Dual_Mode_Scheme.md` 给 Part B（精密模式）、normal/inverted、multi-screen、live capture 加状态标记（aspiration / not implemented），避免据文档承诺不存在的能力；② VP-QSP 文档标注 pitch 适用范围（随 FIX-7 的决策结果）；③ `docs/agents-cli.md` 补 `--split` 与 target 行为说明（随 FIX-13）。
- **验收**：文档与代码能力一一对应，无未标注的空头承诺。

---

## 评估后未收录项（已审，明确不立项）

- `MeasuredPoints::find` O(N) 线性扫描 + 逐查询 String 分配——~4k 点规模才可感，当前墙体不构成瓶颈；
- capture planner 的高度/row-0 约定疑义——随 FIX-2 的 row-0 全链裁决自动消解；
- eval `seeds` 字段虚报与 `rms_size_error_mm` 丢弃——已并入 FIX-9 的 schema 修订；
- DPX transfer characteristic 校验——已并入 FIX-14。

## 附：明确不做（修复期间防跑偏）

- 不重写为通用 SfM / 引入 COLMAP（已知度量靶先验是本项目最大优势）；
- 不在 FIX-1/2/3 完成前做性能优化（解析 Jacobian 属 Phase 2 前置，见 `docs/precision-scan-improvement-plan.md` §3.3）；
- 不在旧指标（center/normal/size）上添加新评测场景——先做 FIX-9；
- 不顺手清理与本清单无关的 dead code / 注释 / 格式。

**Knowledge Sources**: `_walkthrough/algo-review-2026-06-10.md`（审查全文）；`docs/precision-scan-improvement-plan.md`、`docs/roadmap-phase2-3-revision.md`（关联规划）。
**External Inputs**: 无新增；所有结论以 2026-06-10 的代码核验为准。
