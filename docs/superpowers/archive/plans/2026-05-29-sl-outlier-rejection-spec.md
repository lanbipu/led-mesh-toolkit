> ⚠️ **SUPERSEDED / 已实现（2026-05-30 核实）。** 本文档"现状"段（只有 Huber、无
> RANSAC/无显式剔除）已**过时**——该能力随后已落地，见 commit `194d4fe`（merge: SL
> reconstruct robustness）。当前 `reconstruct.py` 已实现：Stage A `stage_a_prune`
> （per-(cam,cab) `cv2.solvePnPRansac` 预清洗）+ Stage B `stage_b_robust_solve`
> （Huber + MAD 残差 trim + 整组 coherence guard，迭代 ≤3 次）+ per-cabinet/total
> `rejected_points`/`n_rejected` 计数与 `high_rejection` 告警。
> **不要照本文档"可选方法"重做剔除管线**（会与现有实现重复/冲突）。如仍有失败数据集，
> 应改写为针对现有 Stage A/B 的 delta：先复现失败、定位是哪一道 guard 漏了、只补那个
> gap。保留此文件仅作历史问题陈述。（Codex 对抗审查标出此过时，2026-05-30。）

# 结构光重建：几何离群剔除（outlier rejection）问题描述 / 实施需求

> 这是一份自包含的问题描述，供并入"检测前端"方案。读它不需要其它上下文。

## 背景
LED 屏结构光反算管线：屏上每个白点按 **二进制 + 偶校验闪码** 编码一个全局 `id`。多机位拍摄 → 解码每个机位得到一批 **对应点(correspondence)**：每点 = `{id, 屏幕坐标(u,v), 相机像素(x,y)}`。重建（`reconstruct-structured-light`）用所有机位的对应点做 **model-constrained bundle adjustment (BA)**，反求每个相机位姿 + 每个箱体(cabinet) 的 SE3，得到屏幕 3D 模型。根箱体 (0,0) 固定为世界原点，尺度来自每箱像素 pitch。

## 问题
解码会产生 **误配对（离群点）**：某点的闪码被读错（噪声 / 大斜角变暗 / 相邻点粘连 / 2-bit 以上错码恰好通过校验且落在一个合法 `id` 上）→ 它被贴上 **错误的 id** → "相机像素 (x,y)" 被配到 **屏幕上错误的 3D 位置**（错 id 的 canonical (u,v) → cabinet-local mm）。这是一个几何上不可能成立的配对。

BA 要让 **所有** 配对同时成立；少数离群配对跟正确那批在几何上互斥，把整个解拽偏 → **发散或解错**。

**实证（本项目 `lmt-test`，9×5 弧，真实 disguise 录像）**：
- 两个 100% 干净解码的机位 → BA 收敛，reproj **0.108px**。
- 第三个机位只解出 **1677/2880** 点（含蒙混过关的错配对）→ 加进去 BA **发散到 228px**；去掉它就好。
- 结论：**只要混进足够的离群配对，BA 就崩。**

## 为什么"检测前端"治不了它（互补，不是重复）
时序/ROI/每点阈值/校验闸能 **减少** 误码，但挡不住两类：
1. **2-bit 以上错码恰好过偶校验、又落在合法 id 上** → "自信地错"，校验闸放它过（因为 id 合法）。
2. **大斜角下两点粘成一个 blob** → 只检出一个、闪码混合 → 必然误码（空间分辨率问题，阈值治不了粘连）。
这些漏网的错配对，**只能在重建层用几何一致性剔除**。

## 代码位置（现状）
- `python-sidecar/src/lmt_vba_sidecar/sl_reconstruct.py :: run_reconstruct_structured_light`
  读每个机位 correspondence → 构造 `Observation`（`camera_idx`, `cabinet_idx`, `p_local` = 按 id 从 sl_meta 取 **canonical (u,v)** 换成 cabinet-local mm, `pixel` = 对 (x,y) 去畸变）→ `check_observability` → 调 `solve_and_emit`。
- `python-sidecar/src/lmt_vba_sidecar/reconstruct.py :: solve_and_emit`
  桥接初始化 `estimate_nonroot_cabinet_init`（内部 `_solve_pnp` = `cv2.solvePnP`）、相机初始化 `_pnp_camera`（也 `_solve_pnp`）→ 然后 `model_constrained_ba(..., loss="huber")`。
- **现状：只有 Huber 损失**（对大残差降权，但不剔除），**没有任何 RANSAC / 显式离群剔除**。`check_observability` 只查每柜视图数/点数，**不查几何一致性**。
- 一个 `Observation` = 一个点；离群 `Observation` = 该点 id 解错、`p_local` 与它在相机里的真实位置不符。

## 修复目标
在 BA **之前 / 之中**，**自动识别并剔除几何不一致的对应点**，使少量误配对不再拖垮整个解。

## 可选方法（细节由实施 session 定）
1. **Per-view PnP-RANSAC 过滤（首选，因为初始化本就在做 PnP）**：把 `_solve_pnp` 换/补成 `cv2.solvePnPRansac`，对每个 `(camera, cabinet)` 的点集求共识位姿 + inlier mask；mask 外（重投影残差超阈值，如 2–3px）= 误配对，从 `Observation` 集删掉，再 BA。
2. **解后残差阈值 + 重解（IRLS/二次剔除）**：先用干净子集或 Huber 出一次解 → 算每个 Observation 的重投影残差 → 删掉 > N px / > Nσ → 重解，迭代 1–2 轮。
3. 二者结合：RANSAC-PnP 出干净 init + 干净 Observation，再 robust BA，最后残差复核。
- 注意：`_00002` 那种**整个机位**就能让 BA 发散 → 剔除最好 **在 init/PnP 阶段就做**（per-view RANSAC），别等 BA 发散再救。

## 成功判据（可直接测）
- **复现**：`lmt-test` 用 `_00002`(1677,含离群) + `_00003`(2880) + `_00004`(2880) 三机位一起重建，**不发散**，结果与只用两个干净机位（reproj 0.108px）一致或更好。
- **TDD 合成**：在一批正确对应点里 **人为注入 5–10% 错 id 配对** → 重建仍收敛、注入的错点全部被标 outlier 剔除、recovered 位姿与无离群时一致。
- **不回归**：干净输入（0 离群）时结果与现在一致。

## 边界（不做）
只解决 **离群配对**。**不**解决：① 真实背景/运动鲁棒性（检测前端 ROI+时序的事）；② 深度精度（要更多方向分散的机位）。三者独立互补。

## CLI / 契约
逻辑放 sidecar（`sl_reconstruct` / `reconstruct`），**不新增 CLI 子命令**；若要暴露阈值，按项目 6 点契约加可选参数（cli → app → adapter → sidecar）+ docs/agents-cli.md + E2E 测试；纯内部默认则无需。
