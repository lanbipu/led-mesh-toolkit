# Path B 现场鲁棒性设计 · 两道防线（检测层时序前端 + 重建层几何剔除）

> 日期：2026-05-29
> 范围：让点阵结构光（Path B）从"只能在 disguise 灰底素材跑通"走到"现场实拍能成"。
> 两个互补的阻塞——**检测层误码** 与 **重建层离群点拖垮 BA**——合成一个里程碑、一份计划。

## 1. 背景：两个互补的现场阻塞

Path B：屏上每个白点按二进制 + 偶校验闪一个全局 id → 多机位拍摄 → 每机位 decode 出一批
对应点 `{id, 屏幕坐标(u,v), 相机像素(x,y)}` → reconstruct 用所有机位的对应点做
model-constrained BA，反求每相机位姿 + 每箱体 SE3（根箱体固定为世界原点，尺度来自像素 pitch）。

### 1.1 检测层问题（decode 前端）

`sl_decode.py` 前端是 naive 的、**假设黑底**：

| 环节 | 现状（核实过） | 现场为什么崩 |
| --- | --- | --- |
| 找哨兵 `segment_code_region` L45–75 | `frame.mean() > sentinel_threshold*255` 整帧均值 | 现场背景本就亮，整帧均值常年高，全屏白哨兵顶不出来 |
| 找点 `_centroids` L103–106 | `cv2.threshold(frame,128,…)` + 连通域 全局 128 | 背景像素远超 128 → 当成一坨假点；斜看暗点反丢 |
| 读位 `_read_bit_at` L109–114 | 3×3 patch 均值 `>128` 全局 128 | 同上，亮度判据现场失效 |

灰底素材（disguise，背景 ≈64 < 128）能解纯属巧合（背景恰好低于阈值），不是鲁棒性。

### 1.2 重建层问题（离群点拖垮 BA）

decode 会产生**误配对（离群点）**：闪码被读错（噪声 / 大斜角变暗 / 相邻点粘连 / ≥2-bit 错码
恰好过偶校验且落在合法 id）→ 贴错 id → 相机像素 (x,y) 被配到屏幕上**错误的 3D 位置**。这是
几何上不可能成立的配对。BA 要让所有配对同时成立，少数离群点把整个解拽偏 → 发散 / 解错。

**实证（lmt-test 9×5 弧，真实录像）**：两个 100% 干净机位 → BA 收敛 reproj **0.108px**；
第三个只解出 1677/2880（含错配对）加进去 → 发散 **228px**，去掉就好。当前 BA 只有 Huber
损失（`loss='huber', f_scale=2.0`，**降权不剔除**），离群点一多就压不住。

### 1.3 为什么两层都要做（互补，不重复）

检测层时序 / ROI / 逐点阈值 / 校验闸能**减少**误码，但挡不住两类漏网的：

1. **"自信地错"**：≥2-bit 错码过偶校验又落在合法 id 上 —— 检测层无从分辨。
2. **斜角粘连**：大斜角两点粘成一个 blob → 必然误码（空间分辨率问题），不是阈值能救的。

这些只能在**重建层用几何一致性**剔除（一个配对的 3D 位置与该机位真实成像不符 → 剔）。
所以：**检测层把误码降到少数，重建层把漏网的少数几何剔除**，两道防线缺一不可。

### 1.4 现场条件（已与用户确认）

1. **机位三脚架锁死全程不动** → 静止背景天然零变化，检测层**不需要帧间配准**。
2. **屏外有运动物体（人 / 车），但绝不遮挡屏幕** → 运动物体只在屏幕 ROI **之外**捣乱。

## 2. 统一目标与成功判据

**目标**：检测层换成"靠闪不靠亮 + 屏幕 ROI"；重建层在 BA 前 / 中自动识别并剔除几何不一致
对应点。使管线在任意亮度 / 纹理静止背景 + 屏外运动物体 + 少量误码下成立。

成功判据（全部以**合成 known-good** 验证——现场无真值，合成可控，符合"synthetic 是最好情况"）：

**检测层**
- **S1 回归**：现有灰底素材走新前端仍 100% 解，现有 SL E2E + sidecar pytest 全绿。
- **S2 亮背景**：合成亮 + 纹理背景 + 叠点 → 解出 ≥99%；同素材走 naive 前端会失败（对照断言）。
- **S3 屏外运动物体**：S2 + ROI 外叠移动亮块 → 解出率不降、不多假点、哨兵 / 切段不偏。
- **S4 暗点 / 斜看**：点亮度 < 背景 → 仍正确解（证明判据是"变化"不是"亮度"）。
- **S5 可视化**：`--emit-debug-image` 出"纯黑底 + 白点"检测掩膜，肉眼核对点 / 框。

**重建层**
- **S6 注入离群剔除**：合成多机位干净观测 + 注入错 id（**含随机远、近邻同箱体、单视图相干平移三类**）
  → 开剔除：BA 收敛低 rms，且被剔 ≈ 注入（每类各算 precision/recall）；关剔除：发散 / 高 rms（对照）。
- **S7 脏机位不拖垮**：复现实证场景——2 干净机位 + 1 含大量误码机位，开剔除后总解仍收敛
  （≈ 只用 2 干净机位的水平），不因第 3 机位发散。
- **S8 剔除可见（no silent caps）**：report / DTO 报告每机位 / 每箱体 / 全局的剔除数；
  剔除比例高的机位发 WarningEvent。
- **S9 凹凸不翻转**：合成 concave 弧 + 全正面斜视角机位 → 重建凹/凸方向 == 真值（不被深度镜像）；
  故意喂翻转初值也能被 init 消歧纠回；旧单解 ITERATIVE 作对照会翻。

---

# Part A · 检测层：靠闪不靠亮 + 屏幕 ROI

## A.1 核心原理与一处诚实边界

把判据从**亮度**换成**变没变**：逐像素时序极差 `range = max−min`。静止背景（哪怕 250 白墙）
`range≈0` 判黑；闪烁点（哪怕斜看 40↔90）`range` 大判点。与 128 阈值的毛病完全相反
（128 留白墙丢暗点；时序极差留暗点丢白墙），绝对亮度不再进入判断。

**诚实边界**：点中心精确定位（seeding）仍用 anchor 帧（all-on）——因为 id=0 点在所有 code 帧
从不亮，纯时序找不到它。但 anchor 的亮度判据**只在 ROI 内做**：ROI 内"背景"是屏幕自身的黑
（点间），不是屏外亮墙；屏外已被 ROI 排除。而 ROI 本身从时序活动图推出（靠闪）。链条自洽：
**靠闪定 ROI → ROI 内 anchor 自适应阈值定点 → 靠闪读每点的位**，没把"找亮矩形"偷塞回主路径。

## A.2 三遍管线（全部在 Python sidecar `sl_decode.py`）

**Pass 1 — 粗 ROI（全片活动图）**：整段逐像素求 `range`。屏幕矩形因哨兵刷过 + 点闪 → 高活动
**实心矩形**；屏外运动物体是分离、细长、不实心、与屏幕不重叠（不遮挡）的块。取最大实心矩形
活动连通域 bbox = 粗 ROI。`--screen-roi x,y,w,h` 手动覆盖兜底；自动失败 → `detection_failed`(13)
（消息提示手动指定）。（哨兵会把整屏刷亮、正好利于框屏；抠点在 Pass 3 用"仅 code 区"避开它。）

**Pass 2 — 同步（只在 ROI 内）**：`segment_code_region` 哨兵均值改 `frame[roi].mean()`、
`index_plateaus` 帧间变化像素数只在 ROI 内统计 → 屏外运动物体影响不到同步（最危险的不是多假点，
是凭空多切一段 / 哨兵错位 → 整段崩）。`sentinel_threshold`（现有 flag，默认 0.85）**语义不变、
计算域改 ROI**，不删 flag（避免动刚合进来的契约链）。

**Pass 3 — 抠点 + 读位 + 解码闸（只在 ROI 内）**：
1. **seeding**：anchor 帧、ROI 内**自适应阈值**（Otsu，非全局 128）+ 连通域 → 候选中心（亚像素）。
   anchor=all-on，所有点含 id=0 都被找到。
2. **形状 / 尺寸过滤**：按 `dot_radius_px`（sl_meta 有）滤圆形、合理大小，去掉大 / 不规则块。
3. **逐点读位**：每点 on/off 跟**它自己**在 code 区的 min/max 比（不用全局 128）→ 斜看暗点也对。
4. **解码闸（已有）**：`decode_bits` 偶校验 + id 落在 `uv_by_id` → 漏网假点闪不出合法码被丢。

**A.2.1 "纯黑底 + 白点"图**：`--emit-debug-image` 把 Pass 3 第 1 步的二值掩膜写成图存 corr.json
旁（`<out>.debug.png`）。既是中间产物、也是肉眼核对素材。v1 只出这张（YAGNI）。

## A.3 检测层接口契约改动

- **sidecar `ipc.py`**：`DecodeStructuredLightInput` 加 `screen_roi: tuple[int,int,int,int]|None`
  (默认 None=自动)、`emit_debug_image: bool=False`；`sentinel_threshold` 保留(语义改对 ROI-mean)；
  **不加** variance 阈值字段(Pass1/3 阈值走 Otsu 自动)。`run_decode_structured_light` 始终走新前端
  (无 legacy 开关，S1 回归守住灰底素材)。
- **CLI**：`cli.rs VisualCmd::DecodeStructuredLight` 加 `--screen-roi <X,Y,W,H>` (`Option<String>`)、
  `--emit-debug-image` (bool)；`commands/visual.rs::decode_structured_light` L383 解析 ROI 字符串
  (格式错 → `INVALID_INPUT`(2)，在 destructive gate **之前**校验，同 reconstruct 的 ≥2 corr 前置)；
  dry-run `would_write` 在 `--emit-debug-image` 时含 debug 图路径。
- **lmt-app / adapter**：`run_decode_structured_light` L537 + `DecodeStructuredLightArgs` L480
  各加 `screen_roi`、`emit_debug_image`；IPC JSON 按 `sentinel_threshold` 同款条件注入。
- **DTO `dto.rs`**：`DecodeStructuredLightResult` 加 `debug_image_path: Option<String>`、
  `screen_roi: Option<[u32;4]>`(实际用的 ROI，供核对)；已有 `JsonSchema`，自动进 `schema::dump_all()`。
- **错误码**：ROI 失败 / 点太少 → 复用 `detection_failed`(13)（仅消息具体化）；哨兵 / 切段失败
  → 复用 `decode_failed`(18)。**不新增错误码**。
- **manifest / agents-cli.md**：decode op CLI 串加 `[--screen-roi X,Y,W,H] [--emit-debug-image]`，
  exit_codes 仍 `[0,2,3,4,13,18]`；命令表第 44 行更新签名 + 说明；side_effect 仍 destructive；
  **不加 Tauri shim**（visual 全组 CLI-only）。

## A.4 检测层测试（TDD）

sidecar pytest（合成 fixture 测试内构造，不依赖现场照片）：`test_decode_gray_bg_regression`(S1)、
`test_decode_bright_textured_bg`(S2) + `..._fails_with_naive`(对照)、`test_decode_moving_object_outside_roi`(S3)、
`test_decode_dim_dots_below_bg`(S4)、`test_decode_finds_id0`、`test_roi_auto_vs_manual`；改完
`build_exe.sh` 重建 binary。CLI E2E：`decode_..._with_roi_and_debug_dry_run`、`decode_..._invalid_roi_format`(exit2)、
happy(复用 `LMT_VBA_SIDECAR_PATH` 跑灰底，S1)、refuse(沿用)。

---

# Part B · 重建层：几何离群剔除

## B.1 离群点本质 & 现有代码

一个 `Observation`（`model_constrained_ba.py`，dataclass: `camera_idx, cabinet_idx,
p_local: ndarray[3] mm, pixel: ndarray[2] 去畸变`）= 一个对应点。离群 = 该点 id 解错 →
`p_local`（按 id 从 sl_meta 取的屏幕 3D 点）与相机里真实位置不符 → 几何不可能。

现状（核实）：
- 装配在 `sl_reconstruct.py` L152–166，按 id 查 sl_meta 取 canonical `p_local`、去畸变 pixel；
  **`per_view_cab_corners: dict[(cam_idx,cab_idx)] → list[(p_local,pixel)]`（L149）装配时已按
  (机位,箱体)分组**——PnP 的天然落点。
- `check_observability`（`observability.py`，`min_views=2, min_points=8` + 二部图连通 BFS）在
  L175、装配后 solve 前调。
- `_solve_pnp`（`reconstruct.py` L495–516）用 `cv2.solvePnP(flags=SOLVEPNP_ITERATIVE)`，**无 RANSAC**；
  被 `estimate_nonroot_cabinet_init`（桥接初值）与 `_pnp_camera`（相机初值）复用。
- `model_constrained_ba`（`model_constrained_ba.py` L98–135）：`scipy.optimize.least_squares`,
  `loss='huber', f_scale=2.0`；`_residuals` 返回 flat `(2*n_obs,)`，**逐点残差可取**（reshape (n_obs,2)）。
- `BaStats`（`ipc.py`）只有 `rms_reprojection_px, iterations, converged` 三字段。

## B.2 两阶段几何剔除（Stage B 全局解是主权威，Stage A 只是廉价预清）

> 经对抗式 review 修正：**per-(机位,箱体) 单平面 PnP-RANSAC 不能当唯一 / 主要权威**。下面写清它
> 能 catch 什么、catch 不了什么，以及为什么主权威必须落在跨视图的全局解。

**Stage A — per-(机位,箱体) PnP-RANSAC 预清（廉价、砍粗大随机离群）**（装配后、`check_observability` 前）。
对 `per_view_cab_corners` 每个 `(cam,cab)` 组（同箱体点共面 z=0）跑 `cv2.solvePnPRansac` →
inlier mask → 按 mask 重建 observations、同步更新 `per_cabinet_views/points`。

它**能 catch**：
- 远配错（错 id 落到该箱体很远的点）：残差 ≫ 阈值 → 剔。实证里 228px 那种 gross 离群属此类，
  Stage A 直接解决。
- 独立的近邻配错（错 id 落到相邻点）：因为**可分辨的点在图像里的间距 > RANSAC 阈值(2–3px)**
  （间距小到 < 阈值时两点早粘成一个 blob、根本检测不出两个），单个近邻配错残差 ≈ 点间距 > 阈值 → 仍剔。

它**catch 不了**（盲区，必须由 Stage B 兜）：
- **相干 / 系统性配错**：一整片 id 被一致地平移（解码栅格整体错位、新 ROI/seeding 的栅格滑移）→
  这种一致偏移会被 PnP **吸收进一个错的平面位姿**，所有点残差都低 → 全判 inlier。
- 更糟：若相干错的点占该组**多数**，RANSAC 会锁定到**错的 consensus**、反把正确的少数判成 outlier。
  ⇒ 所以单平面 RANSAC 的 inlier 数 / 覆盖率**不可信作为权威**，只能当预清。

- 共面 PnP-RANSAC 平面退化时切 `SOLVEPNP_IPPE`（impl 调，S6/S7 验）；组内 < `MIN_PNP_CORNERS`(4)
  跳过、交给 Stage B；阈值（reprojErr≈2–3px、confidence 0.99、iters 100）走 sidecar 内部常量，**不开 CLI flag**。

**Stage B — 全局联合 BA 的鲁棒残差裁剪（主权威，提供跨视图一致性）**（包住 `model_constrained_ba`）。
跨视图冗余是揭穿相干单视图错的关键：一个箱体按 observability 至少被 2 个机位看到；若机位 1 对该箱体
有相干配错、被 Stage A 吸收成错位姿，进入全局联合解后，机位 1 的观测与**机位 2 的正确观测 + 桥接约束**
无法同时满足 → 机位 1 这组观测残差被顶高 → 被裁掉。流程：
1. **从跨视图一致的观测播种**：优先用"被 ≥2 机位看到、且这些视图互相吻合"的点做 clean seed，降低
   相干错污染初值的风险（Codex 建议的 leave-one-view / cross-view agreement 的落地形式）。
2. 跑 BA → 取逐点残差（`_residuals` reshape (n_obs,2)）→ 丢 `norm > max(k·MAD, 绝对px下限)`
   （k≈3，下限≈3px）→ 重解 → 重复至无可丢或封顶（≤3 iter）。
3. **整组相干守卫**：除逐点裁剪外，若某 `(机位,箱体)` 组**中位残差**在联合解后仍系统性偏高（不是
   个别点、是整组），整组降权 / 剔除——专治被 Stage A 吸收的相干错。
- 每轮残差**重算**、用**当轮** `cabinet_poses` 算 per-cabinet RMS；设地板不把任何箱体砍破 min_points
  （否则 L412 KeyError / BA 退化）。

**根本限度（老实写）**：若某箱体恰好只被**最少 2 个机位**看到、且其中 1 个是相干错，2 票 1:1 无法
多数表决 → 这是欠定的本质二义，Stage B 只能把它标成高残差 / 触发 `observability_failed` 暴露出来，
**不能**自动判对。缓解靠多机位冗余（现场 SOP 鼓励每箱体 ≥3 视图）+ report 警告，不在本里程碑求解。

**Stage A + B = 重建层 defense-in-depth**：A 廉价砍掉粗大随机离群（含实证的 gross 离群），让初值 / BA
不被海量垃圾淹没；B 是全局权威，靠跨视图一致性收掉 A 看不见的相干错。两者都复用现成机件（PnP、BA、
Huber），不引入新求解器。

## B.3 `_solve_pnp` 升级（一石三鸟：离群 + 鲁棒初值 + 凹凸消歧）

把 `_solve_pnp` 内部换成 `solvePnPRansac`：①**初始化也变鲁棒**——`estimate_nonroot_cabinet_init`
桥接 PnP、`_pnp_camera` 相机初值当前用裸 solvePnP，本身就被离群点带偏；换 RANSAC 后初值更稳
（初值偏了 BA 也救不回，见 Path B 桥接历史）。②顺带产出 inlier mask 供 Stage A 复用。
注意（gotcha）：`tvec` 仍需 `.reshape(3)`；返回 None 要照常判（共线退化）；inlier <4 时按原逻辑跳过。
③**同一处再叠 IPPE 两解 + 凹凸消歧（见 Part C）**——RANSAC 出 inlier、IPPE 出两分支、front-facing
消歧，合成一个例程，三件事一次改完，不要拆成两遍。

## B.4 顺序与 observability 交互（关键，否则会出 KeyError / 假发散）

- Stage A 在 `check_observability`(L175) **之前** → observability 在 inlier 上评估（脏机位不该
  计入覆盖）。但若 A 砍到二部图断连 / 某箱体 < min_points → `observability_failed`(17)，这是
  **正确行为**（问题真的欠约束），消息须说明"剔除 N 个离群后..."。
- Stage B 在 BA 之后，可能把某箱体砍到 < 8 → **每轮 trim 后设地板**：任何箱体将跌破 floor 就
  停止 trim（不过度裁剪进退化）；`_per_cabinet_reproj_rms` 用**当轮** `cabinet_poses` 算（别用上轮，
  否则 RMS 静默错）；绝不把箱体砍到 0（否则 L412 直接 KeyError）。
- 残差每轮**重算**（`sol.fun` 是上一次 least_squares 的，stale）。

## B.5 重建层接口契约改动

- **默认 ON、无新 CLI flag**：剔除是"应该默认就对"的鲁棒估计，不让用户调（同 Part A 的 Otsu 决策、
  YAGNI）。`reconstruct-structured-light` 的 CLI 签名 / flag **不变**，manifest CLI 串 / exit_codes
  `[0,2,3,4,13,14,16,17]` **不变**。要调再加 flag。
- **不新增错误码**：`ba_diverged`(14)、`observability_failed`(17) 已覆盖硬停。剔除后仍发散 → 14；
  剔除后欠约束 → 17（消息说明因剔除）。
- **报告剔除数（no silent caps，必须做）**——唯一要动的 DTO：
  - Python `BaStats`（`ipc.py`）加 `n_observations_total: int`、`n_observations_used: int`、
    `n_rejected: int`。Python `CabinetPose`（`ipc.py`）加 `rejected_points: int`（与 observed_points 并列）。
  - Rust `VisualReconstructResult`（`dto.rs`）只加全局 `ba_observations_total/used/rejected: usize`
    （已注册 `JsonSchema` 结构上的新字段，自动进 schema dump，**无新类型**）；adapter ba_stats 解包
    同步加字段。**per-cabinet 剔除数只留在 Python `CabinetPose`/pose_report.json**——Rust
    `CabinetPoseSummary` 当前连 observed_points 都没有，给它加 rejected_points 没分母、意义弱，
    不动它；要细看每箱体剔除走 pose_report.json。
  - 高剔除比机位 / 箱体（如 >30% 被剔）发 `WarningEvent`，让操作者看到"第 3 机位剔了 1203/2880，
    查这条录像"。
- **agents-cli.md**：reconstruct 行说明补"含逐观测离群剔除统计"；错误码表不变；新 BaStats /
  DTO 字段写进文档。

## B.6 重建层测试（TDD：注入离群 known-good）

sidecar pytest（`test_reconstruct.py`）：
- `test_outlier_injection_rejected`(S6)：合成多机位干净观测，注入**三类**错 id 并分别断言——
  (a) 随机远配错、(b) 局部近邻同箱体配错、(c) 单视图相干栅格平移；开剔除断言收敛低 rms +
  被剔集 ≈ 注入集（每类各算 precision/recall）；`..._diverges_without_rejection`(对照)。
  **(b)(c) 是对抗 Stage A 盲区的关键用例，绝不能只测 (a) 随机远离群——否则测试假阳性通过、给假信心。**
- `test_dirty_view_does_not_break_solve`(S7)：2 干净机位 + 1 大量误码机位（含相干错），开剔除断言
  总解收敛 ≈ 仅 2 干净机位水平——验证是 Stage B 跨视图把相干脏视图顶出来，而非靠 Stage A。
- `test_coherent_error_caught_by_global_not_stageA`：构造单视图相干平移，断言 Stage A 把它当 inlier
  放过、而 Stage B 全局残差 / 整组守卫把它剔掉（证明主权威在 B、不在 A）。
- `test_two_view_coherent_is_flagged_not_silently_wrong`：某箱体仅 2 视图、其一相干错，断言系统
  **不静默给错解**——要么高残差 Warning、要么 `observability_failed`（验证根本限度被暴露而非掩盖）。
- `test_stageA_pnp_ransac_inliers`：单 (机位,箱体) 组注入**远**离群，断言 inlier mask 正确（Stage A
  只承诺砍远离群）。
- `test_rejection_stats_reported`(S8)：断言 BaStats / CabinetPose 的剔除数字段正确填充。
- observability 边界：`test_overtrim_stops_at_floor`（trim 不把箱体砍破 min_points）、
  `test_aggressive_rejection_raises_observability`（脏到断连 → `observability_failed`，消息含剔除说明）。
- 改完 `build_exe.sh` 重建 binary。

CLI E2E（`cli_e2e.rs`）：reconstruct 既有 refuse/dry-run/single-corr 用例不变；新增
`reconstruct_..._reports_rejection_stats`（happy：跑含离群的合成 corr，断言 envelope 里
`ba_rejected > 0` 且 `converged`，复用 `LMT_VBA_SIDECAR_PATH`）。

---

# Part C · 重建层初始化：凹凸翻转歧义（planar PnP 两解）

## C.1 问题本质 & 为什么多机位救不掉

每个箱体是平面点阵。斜视角下平面 PnP 有**两个低重投影解**（IPPE 的 mirror ambiguity，
Schweighofer–Pinz）：平面"朝前 / 朝后"两种 tilt 都能近乎完美重投影。init 挑错分支 → BA 锁死在
"曲率方向整体取反"的局部最优——**形状本身对（实测 3.5mm），但整条弧凹 / 凸被深度镜像翻转**。

**为什么加再多机位也救不掉**：① 这是**离散**歧义，BA 是连续局部优化，跨不过两分支间的能垒——
一旦种在错分支就出不来；② 所有相机都在屏幕**正面同侧**，错分支产生的是一个**全局一致的镜像**，
从任何正面视角看重投影都几乎一样低 → 加正面机位等于给两个分支同时加等量证据，**打不破平局**。
只有对两分支**不对称**的约束能破：相机在正面这个物理事实、shape_prior 弯曲方向、或从背面看
（LED 墙背面看不到）。所以必须在 **init 阶段**用先验把每个箱体的曲率符号钉死，再进 BA。

## C.2 核实结论（修正"用 nominal 朝向"的措辞）

- `_solve_pnp`（reconstruct.py:510）现用 `cv2.solvePnP(SOLVEPNP_ITERATIVE)`，**返回单解、不处理歧义**——
  挑哪个分支取决于 homography init，不可控。
- `nominal_cabinet_centers_model_frame(cabinet_array, shape_prior)`（nominal.py）**只给每箱中心(平移)、
  不给朝向**；nonroot init 朝向来自 `estimate_nonroot_cabinet_init` 的桥接 PnP（也走 `_solve_pnp`，
  同样歧义），无桥接时 fallback **单位旋转**。所以"用已知 nominal 朝向"原话不准确——朝向得**现算**。
- 但 `cmd.project.shape_prior` 可用（curved 带 `radius_mm`）→ **能从同一段弧几何算出每箱 nominal 朝向**
  （nominal.py 现有 `_cabinet_center_model_m` 的兄弟函数），作消歧验证。folded 当前 nominal.py 直接
  raise（M2 不支持）→ 不会进到这里，Part C 只需覆盖 flat / curved。
- 法向约定须对齐 `reconstruct_cabinet_geometry`（reconstruct.py:423，从 corners_local winding 出 normal）——
  否则消歧可能**反向**（确定性翻转，更糟）；用一致性单测守住。

## C.3 修法（init PnP 取两解 + 分层消歧）

把 init 用的 PnP 升级成"取两解 + 消歧"：
1. 在（Stage A RANSAC 的）inlier 上跑 `cv2.solvePnPGeneric(obj,img,K,None,flags=SOLVEPNP_IPPE)`
   → 最多 2 个分支 + 各自重投影误差（需 OpenCV ≥4）。
2. 消歧，按可靠性分层：
   - **主：相机在正面（front-facing，无先验）**——正确分支的箱体发光法向 `R·n_local` 必指回相机
     （相机系 z 分量 < 0；`n_local` 取与 `reconstruct_cabinet_geometry` 一致的发光面约定）。斜视角下
     两分支法向发散、错分支朝后 → 直接排除。**这条用同一物理约束钉死全局符号 → 不会出现整弧镜像。**
   - **近正面（两分支都朝前、front-facing 不决）**：取 IPPE 两解**重投影误差比**小的那个（此时歧义本就弱、可靠）。
   - **验证 / tie-break：shape_prior nominal 朝向**——composed world_from_cabinet 的法向应与该箱 nominal
     弧法向同号；明显相反（仍翻转）则纠正。（用户建议的信号，定位为验证而非主判据：它需现算朝向、
     且 folded 不可用；front-facing 才是永远在场的兜底。）
   - **邻居一致性**：相邻箱体朝向应平滑、不 zig-zag（弧的平滑先验）。
3. 返回消歧后的 (R,t)。低裕度（两分支都说得通）→ 发 `WarningEvent`（凹凸是 close call，让操作者知道）。

## C.4 与 Part B 的合流（同一个 `_solve_pnp` 升级）

Part B Stage A 要 RANSAC inlier、Part C 要 IPPE 两解消歧——合成**一个**升级后的 PnP 例程：
`solvePnPRansac 取 inlier → 在 inlier 上 solvePnPGeneric(IPPE) 取两分支 → front-facing(+nominal) 消歧
→ 返回 (R,t,inlier_mask)`。该例程同时服务 Stage A 离群剔除、凹凸消歧、桥接 / 相机鲁棒初值；
`_solve_pnp` 及其两个调用方（`estimate_nonroot_cabinet_init`、`_pnp_camera`）统一走它。**实现上交付项
⑨⑩ 与 Part C 是同一处改动，不要拆成两遍写。**

## C.5 接口契约

- **无新 CLI flag / 无新输入**：shape_prior 已随 project 传入；消歧全在 sidecar init。
- **新 nominal 朝向 helper**（nominal.py 纯函数，sidecar 内部，无对外契约面）。
- **report / DTO 不变、无新错误码**：低裕度消歧用 `WarningEvent`（复用现有机制，no silent caps）。
- 法向约定对齐 `reconstruct_cabinet_geometry`。

## C.6 测试（合成凹凸 known-good）

sidecar pytest（`test_reconstruct.py`）：
- `test_oblique_arc_not_flipped`(S9)：合成 concave 弧 + 全正面斜视角机位 → 重建凹凸方向 == 真值；
  `..._iterative_baseline_can_flip`（对照：旧单解 ITERATIVE 会翻）。
- `test_seeded_flip_is_corrected`：故意喂翻转初值，断言 front-facing 消歧纠回。
- `test_front_facing_picks_branch`：单箱斜视角，断言 IPPE 两解里选中法向朝相机的那个。
- `test_nominal_orientation_tiebreak`：near-frontal 两分支都朝前，断言 shape_prior nominal 朝向 tie-break 选对。
- `test_low_margin_emits_warning`：构造高度歧义，断言发 WarningEvent。
- `test_normal_convention_matches_geometry`：消歧用的 `n_local` 与 `reconstruct_cabinet_geometry` 法向同约定。
- 改完 `build_exe.sh` 重建 binary。

---

## 3. 范围外（统一，明确不做）

- **检测层**：帧间配准 / 防抖（机位锁死不需要，手持是另一里程碑）；matched-filter（拿已知编码
  与每像素时间曲线相关，后续加固）；逐帧背景建模；屏面玻璃上反射的运动物体（靠形状 + 解码闸兜底）。
- **重建层**：传递式多箱体桥接（沿用现有，见 Path B 历史）；离群剔除的 CLI 调参 flag（默认 ON，
  YAGNI）；新错误码 `outlier_rejection_failed`（复用 14/17）；BA 求解器替换（沿用 scipy least_squares）。
- **"2 视图 + 1 相干错"的本质二义不自动消解**：见 B.2 根本限度——靠多机位冗余 + 暴露（高残差 /
  observability_failed），不在本里程碑做"少数服从多数失效时仍判对"的求解。
- **现场实拍验证**：暂无现场真值素材，以合成 known-good（S1–S9）验收；有 lmt-test 9×5 弧真实录像
  可做 S7（脏视图）/ S9（凹凸）的非回归手验，但不进 CI（依赖仓库外数据）。
- 两层之外的 reconstruct 既有行为（provenance gate、根箱体强制、measured.yaml 命名）保持不动。

## 4. 交付清单（任一缺失视为未完成）

**Part A 检测层**：① `sl_decode.py` 三遍管线 + `ipc.py` 两字段；② sidecar pytest(S1–S5+id0+roi) + 重建 binary；
③ `cli.rs`/`commands/visual.rs` 两 flag + ROI 解析校验；④ `lmt-app`/`adapter` 透传；
⑤ `dto.rs` `DecodeStructuredLightResult` 两 Optional 字段(+schema dump)；⑥ `manifest.rs` CLI 串；
⑦ `cli_e2e.rs` 新用例；⑧ `agents-cli.md`。

**Part B 重建层**：⑨ `_solve_pnp`→solvePnPRansac；⑩ Stage A per-(cam,cab) PnP-RANSAC 预清
(砍远离群，装配↔observability 之间，**不作权威**)；⑪ Stage B 全局 BA 鲁棒残差裁剪(**主权威**:
跨视图 clean-seed + 逐点裁剪 + 整组相干守卫 + observability 地板 + 残差重算);
⑫ `BaStats`+`CabinetPose` 剔除数字段 + WarningEvent；⑬ Rust `VisualReconstructResult` 全局剔除数
三字段(+schema dump) + adapter 解包(per-cabinet 剔除数留 pose_report.json，Rust summary 不动)；
⑭ sidecar pytest(S6–S8+边界) + 重建 binary；
⑮ `cli_e2e.rs` 剔除统计用例；⑯ `manifest.rs` 说明 + `agents-cli.md`。

**Part C 凹凸消歧**（与 ⑨⑩ 同一处 PnP 改动）：⑰ init PnP 取 IPPE 两解 + front-facing 主消歧
（+ shape_prior nominal 朝向验证 / 邻居一致性 / 低裕度 WarningEvent）；⑱ nominal.py 新 nominal-朝向
纯函数（从 curved 弧几何算）；⑲ sidecar pytest(S9 + 翻转纠正 + 法向约定一致性) + 重建 binary。
（无 CLI / DTO / 错误码改动。）

**联调自检**（合并前）：`cargo test --workspace`、`./target/debug/lmt --json schema | jq`(新字段进 dump)、
`./target/debug/lmt visual decode-structured-light --help` / `reconstruct-structured-light --help`(新 flag / 文档)。
