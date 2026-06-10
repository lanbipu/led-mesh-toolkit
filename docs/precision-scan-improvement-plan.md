# Precision Screen Scan 落地改进方案

> 日期：2026-06-10
> 性质：对 `docs/LED_ScreenModelCal_Dual_Mode_Scheme.md` Part B（§17–29，精密校正 / VP-SSP）的技术评审与落地改进建议。
> 背景：2026-06 全仓算法审查（报告：`_walkthrough/algo-review-2026-06-10.md`）确认 Part B 目前**零实现**——仓库内无任何 graycode / phase 模块，现有第二条路（时序点阵 SL）与 VP-QSP 一样只输出刚体箱体位姿，测不了亚箱体形变。本文回答三个问题：这条路线是否成熟、有无更优替代、落地要补什么。

---

## 1. 结论摘要

| 问题 | 结论 |
| --- | --- |
| Gray Code + Phase Shift 路线成熟吗 | **核心算法教科书级成熟**（工业 3D 扫描标准方法）；且"LED 屏自发图案"比经典投影仪结构光更简单——图案源无需标定，屏幕 UV 精确已知 |
| 风险在哪 | **不在算法，在发光体拍摄物理**：摩尔纹/拍频、LED PWM/扫描刷新 × 曝光、动态范围。设计文档把它们列为前提条件（§29），但没给达成方法 |
| 有更优路线吗 | **路线级没有**（deflectometry 不同构、DIC 精度不够、逐 LED 检测不可扩展）；同族内有一个明确升级：**多频外差相位解包裹替代大部分 Gray Code 帧** |
| 能直接照 §21 的 54 帧序列开写吗 | **不能**。正确顺序：单箱体物理实验消风险 → 修位姿底座账本 → 解析 Jacobian → 再搭管线 |

---

## 2. 路线评估细节

### 2.1 为什么这条路是对的

- 精密模式需要的数据是 `camera pixel ↔ LED continuous UV` 的稠密对应（§20），时序结构光是获取这种对应的标准且唯一工业成熟手段。
- 屏幕内容完全受控 = 经典结构光里最难的"投影仪标定"环节直接消失；每箱体 pixel pitch 已知 = 度量尺度天然内置。
- 文档自身边界意识良好：§25 无外部尺度不承诺绝对 1:1、§27.5 timing 层标注非必需、§29 列明前提——不是一份过度承诺的文档。

### 2.2 替代方案对比（为什么不换）

| 方案 | 判定 |
| --- | --- |
| 偏折术 deflectometry（屏显条纹测反射） | 不同构：测的是镜面反射面，LED 墙是发光体不是镜面 |
| 随机纹理 + DIC 稠密匹配 | 精度低于相位法，且浪费内容受控优势 |
| 近距逐 LED 直接检测 | 整墙不可扩展；相位法本质是它的正确插值版 |
| 加密 VP-QSP / 点阵 | 永远到不了逐像素密度，测不了 bowing/twist |
| 全量 SfM/COLMAP | 扔掉已知度量靶先验，倒退（同快速模式结论） |

### 2.3 同族内的关键升级：多频相位替代 Gray Code

**问题**：Gray Code 依赖二值边沿，而摩尔纹、失焦、bloom 最先破坏的恰恰是二值边沿——它的失效模式与 LED 屏的物理弱点正面相撞。§21 的序列里 Gray Code 占 28 帧（08–35），是最脆弱也最冗长的部分。

**改法**：
- 精细对应全部交给 **multi-frequency heterodyne phase shift**（如 3 频 × 5-step ≈ 15 帧），全程平滑正弦，对发光面板鲁棒，帧数比 Gray+PS 组合更少；
- 粗定位（"在哪个周期/哪个箱体"）交给**已实现的 VP-QSP marker 或 Large Anchor 层**——粗码只需周期级分辨率，箱体级 ID 完全够用；
- Gray Code 整层降级为可选 fallback（极端长墙单频周期数过多时再启用）。

文档 §21 已把 multi-frequency 列为抗噪备选；本方案将其升为主方案。

---

## 3. 落地前置条件（顺序不能反）

### 3.0 第零步：单箱体物理实验（一天，消掉最大风险）

在写任何管线代码之前：一块真实面板（建议 P2.6 左右，主流 pitch）+ 一台 global shutter 相机，播 5-step 相移序列，实测：

1. **相位噪声 vs 失焦量**：摩尔纹的标准解法是轻微失焦把 LED 网格低通成连续条纹——失焦多少、拍摄距离/pitch 比例多少，必须实测出配方，文档没有给；
2. **相位噪声 vs 曝光时间**：验证曝光 ≫ LED PWM 周期的下限；扫描刷新（scanout）与快门的交互；
3. **相位噪声 vs 灰阶/HDR 档位**：确定 radiometric normalization（§21 Layer 0）的实际档位需求；
4. 顺手产出**误差预算表**：σ_phase（rad）→ σ_UV（LED px）→ 结合基线几何 → σ_3D（mm）。§29 给了目标数字（0.5–2mm relative）但没有推导链，这张表决定后续每个参数（step 数、频率组合、视角数）怎么选。

**通过标准**：在可达成的失焦/曝光配方下，单视角 σ_UV ≤ 0.1 LED px（对应 P2.6 ≈ 0.26mm，三角化后留出 2–3× 余量到 mm 级）。达不到就回到台架调配方，不进入下一步。

### 3.1 位姿底座账本（继承自全仓审查 P0，精密模式全部继承且被放大）

稠密 UV→XYZ 三角化建立在相机位姿 + K 之上。以下账本问题不修，逐像素精度只会把它们放大到更显眼：

- 曲面法线镜像（`nominal.py:114`，正确应为 R_y(−a)·ẑ）→ 统一为单一 SE(3) nominal tile 真源；
- y-down/y-up 混用（init seed / align 目标三处不翻转）→ rows≥2 + 弯墙回归 fixtures；
- 无传递桥接 + identity 旋转 fallback → pose graph 初始化；
- `converged=True` 硬编码 + 双条件验收门 → 如实上报 + 绝对 RMS 门；
- 标定门 roll 穿透 → 改测视轴夹角；K 带先验进 BA（精密模式 §27.4 本来就要求 camera-level 联合求解，这是顺路）。

### 3.2 输入/输出链两个已知坑

- **DPX ingest `>>2` 10-bit 截断必须先修**（`dpx.py`）：相位精度正比于强度 SNR，丢 2 bit 在快速模式是浪费、在相位模式是直接吃精度。改为保留 10/16-bit 浮点路径进相位解码；
- **导出 UV 均匀网格假设必须先修**（`export.rs` pose-obj 路径）：`uv_to_xyz.exr` 等稠密输出依赖 UV↔几何逐像素对应，现有均匀 cols×rows 假设与非均匀 `input_rect_px` 布局矛盾，会让稠密输出继承错位。

### 3.3 求解器规模化

3–6 相机 × 16K×4K 画布的逐像素观测是**千万级**，现纯 Python 残差 + scipy 数值差分 BA 撑不住：

- 解析 Jacobian + 向量化残差从"优化项"升级为**前置条件**；必要时换 sparse Schur 求解器（Ceres 绑定或等价物）；
- 参数化照 §27 分层执行：screen SE(3) / cabinet SE(3) / **B-spline 形变系数**——求系数不求裸点，把 surface-level 参数量压到千级；
- 稠密观测做空间抽稀 + 协方差加权（相位幅度低的像素降权），而不是全量塞进 BA。

---

## 4. 实施路线图

### Phase P-0：风险消除（≈1 周）
- §3.0 单箱体台架实验 + 误差预算表。**交付**：拍摄配方文档（失焦/曝光/灰阶）、σ_phase→mm 预算表、go/no-go 结论。

### Phase P-1：底座修复（与快速模式 P0 合并执行）
- §3.1 全部账本项 + §3.2 两个链路坑。**交付**：弯墙/多行回归 fixtures 绿。

### Phase P-2：相位解码核心（纯 sidecar，无 GUI 依赖）
- 模块：`phase_codec.py`（多频序列生成）+ `phase_decode.py`（归一化 → 相位 → 外差解包裹 → 稠密 UV 图 + 置信度图）。
- 粗定位复用 VP-QSP marker 检测（箱体 ID + 周期锚定）；帧序列同步复用时序 SL 的 sentinel/.seq/DPX 采集链。
- **交付**：合成台（proper 投影渲染，禁用图像 warp——手性 artifact 前车之鉴）+ 单箱体实拍各一组 decode 验证，σ_UV 达预算表。

### Phase P-3：稠密重建
- 多视角三角化 + 分层 BA（§27 变量分层）：先解 screen/cabinet SE(3)（复用现 model_constrained_ba 框架 + 解析 Jacobian 改造），再固定位姿解 B-spline 形变。
- 尺度基准照 §25 最低配置执行：cabinet 尺寸 + pitch + 4–8 控制点（少量全站仪点，Path A 工具链现成）——**这条不许省**。
- **交付**：`as_built_screen_mesh.obj` + `uv_to_xyz.exr` + 逐箱体/逐区域误差报告；验证用 holdout 角点/控制点 RMS（不用 center/normal/size 类盲指标）。

### Phase P-4：产品化集成
按项目 CLI 底座契约（CLAUDE.md），以下为不可缺交付项：
1. `lmt-app` helper（业务逻辑全在 `crates/lmt-app`，不进 src-tauri）；
2. Tauri thin shim；
3. `lmt` CLI 子命令（建议：`lmt visual scan-decode` / `lmt visual scan-reconstruct`，destructive 走 `--yes`/`--dry-run`）；
4. CLI E2E 测试（happy / refuse / dry-run / error envelope 四类）;
5. `docs/agents-cli.md` 命令表 + 错误码表更新；
6. 新 DTO 派生 `JsonSchema` 并入 `schema::dump_all()`。

### 明确不做
- 不做 §21 原始 54 帧序列的逐帧照搬（Gray Code 28 帧按 §2.3 裁掉）；
- 不做边移动边采集（文档 §22 已正确否决，维持）；
- timing-level（§27.5）维持文档判断：本期不做，留接口；
- 不在底座账本修复前开写稠密管线。

---

## 5. 可复用资产清单

| 资产 | 复用点 |
| --- | --- |
| 时序 SL 的 sentinel/帧序列同步 + disguise .seq/DPX 采集链 | Layer 0 采集与帧对齐（修完 `>>2` 截断后） |
| VP-QSP marker 检测 + codec | 粗定位层（替代 Gray Code 的箱体/周期锚定） |
| `model_constrained_ba` 框架 + Stage A/B 外点剔除 | 位姿层求解骨架（需解析 Jacobian 改造） |
| `intrinsics_solve` 共享门（修复 roll 漏洞后） | 相机标定 / K 先验 |
| Path A 全站仪工具链 | §25 控制点采集 |
| `procrustes` / holdout 评估函数（`se3_aligned_holdout_rms`） | 验证指标（接线后） |

---

**Knowledge Sources**: `docs/LED_ScreenModelCal_Dual_Mode_Scheme.md` §17–29；`_walkthrough/algo-review-2026-06-10.md`（账本/链路缺陷的代码证据）。
**External Inputs**: 结构光测量标准方法学（N-step 相移、多频外差解包裹、Gray code）与显示计量中的摩尔纹/失焦处理惯例——用于成熟度判定与 §2.3 升级建议。
