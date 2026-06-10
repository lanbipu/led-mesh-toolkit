# 研发路线 Phase 2 / Phase 3 修订建议

> 日期：2026-06-10
> 对象：`docs/LED_ScreenModelCal_Dual_Mode_Scheme.md` §35 推荐研发路线（Phase 2 / Phase 3）+ 其依赖的 Part C 统一架构（§30–34）。
> 依据：2026-06 全仓算法审查（`_walkthrough/algo-review-2026-06-10.md`）、精密校正改进方案（`docs/precision-scan-improvement-plan.md`）、本次对 §35/Part C 的逐项代码核对。

---

## 0. 前提修正：Phase 1 并未按文档自己的清单完成

按 §35 Phase 1 的 10 项清单逐项对照代码：

| Phase 1 清单项 | 现状 |
| --- | --- |
| VP-QSP 图案生成 | ✅（但 P2.3–P3.9@500mm 箱体被拒 + blit 半像素偏差） |
| coded marker detection | ✅（模糊悬崖未表征，零鲁棒性测试） |
| Gaussian dot centroid | ✅（无加权、无偏差模型） |
| normal / inverted confidence | ❌ **死代码**：检测器支持差分参数，生成器从不输出 inverted 帧，无任何调用方 |
| live capture | ❌ 不存在（纯离线 manifest 工作流） |
| automatic frame selection | ❌ 不存在 |
| screen-level transform solve | ✅（单屏；多屏 screen-to-screen 无） |
| panel group offset solve | ✅（per-cabinet SE(3)） |
| reprojection dashboard | 部分：pose report 有逐箱体 RMS/quality；GUI dashboard 形态未核对 |
| OBJ / nDisplay mesh export | OBJ ✅；**nDisplay ❌**（代码中仅存在于注释；unreal target 还是 no-op） |

结论：Phase 1 状态是"核心链路可用，约 4 项承诺缺失"。其中 normal/inverted（环境光鲁棒性的文档依据）和 nDisplay/unreal 出口直接影响 Phase 2/3 的规划假设，不能默认存在。

---

## 1. Phase 2 修订（Precision Screen Scan）

### 1.1 原清单逐项判定

| 原清单项 | 判定 |
| --- | --- |
| Gray Code decoder | **修正**：降级为可选 fallback。主方案改为多频外差相位（Gray Code 的二值边沿正撞 LED 屏摩尔纹/失焦/bloom 弱点），粗定位复用已实现的 VP-QSP marker。理由详见 `precision-scan-improvement-plan.md` §2.3 |
| Phase Shift decoder | ✅ 保留，升格为核心（multi-frequency heterodyne） |
| dense UV observation | ✅ 保留，但见 §1.3 数据结构前置 |
| multi-view capture manager | **修正**：capture planner 已存在但对目标场景给错误答案（候选机位全部瞄墙中心 → 宽墙边缘箱体结构性不可覆盖；弯墙法线镜像 → 可见性判反）。此项 = 修复现有 planner，不是从零新建 |
| scale bar / control point input | ✅ 保留（§25 最低配置不许省）；注意当前 control point 进 BA 的约束接口不存在，需新建（measured.yaml 是死端，不能复用） |
| per-cabinet pose solve | ✅ 已有（model_constrained_ba），但需解析 Jacobian 改造（见 §1.2-c） |
| B-spline deformation | ✅ 保留（分层求解，参数量控制的关键） |
| UV-to-XYZ map | ✅ 保留；前置：先修导出链 UV 均匀网格假设，否则稠密输出继承错位 |
| validation report | **修正**：先换指标。center/normal/size 类指标对箱体 roll、全体倾斜数学上恒为零；headline 必须用 SE(3)-holdout 角点 RMS（函数已存在未接线）。指标不换，validation report 只会自证清白 |

### 1.2 原清单缺失项（必须补进 Phase 2）

a. **第零步：单箱体物理实验**。摩尔纹/PWM/动态范围的拍摄配方 + σ_phase→mm 误差预算表，go/no-go 门槛：单视角 σ_UV ≤ 0.1 LED px。一天工作量，消掉最大落地风险（详见 `precision-scan-improvement-plan.md` §3.0）。

b. **位姿底座账本修复**（曲面法线镜像、y-up/y-down 混用、传递桥接、标定门 roll 穿透、converged 硬编码）。稠密重建放大底座误差，顺序不能反。

c. **求解器规模化**：解析 Jacobian + 向量化（必要时 sparse Schur / Ceres 类后端）。3–6 相机 × 16K×4K 的逐像素观测是千万级，现纯 Python BA 撑不住——这在 Phase 2 是前置条件不是优化项。

d. **DPX 10-bit ingest 修复**（现 `>>2` 截断为 8-bit）。相位精度 ∝ 强度 SNR，丢 2 bit 直接吃精度。

e. **墙地夹角 / 多屏几何表达**——原清单最大的结构性缺口。§17 把"墙地夹角是多少""每块屏幕在哪"列为精密模式的核心问题，§27.1 的 screen-level 变量包含 wall-floor angle、screen-to-screen angle，但当前几何表达根本承载不了：`shape_prior` 只有 flat/curved，folded 显式 refuse，无多 screen joint solve（VP-QSP 的 4-bit screen_id 只是预留），无世界帧多屏合并。**不补这一项，Phase 2 做完也回答不了 §17 的问题清单。** 建议：multi-screen joint solve（共享相机位姿、屏间 SE(3)）列为 Phase 2 正式交付项，folded/L 形组合至少支持两平面拼合。

### 1.3 Part C 统一数据结构落地（Phase 2 顺手完成，为 Phase 3 留口）

§30 承诺的统一 Observation（confidence / covariance / camera_id / frame_id / timecode / tracking_pose / FIZ），当前实现只有 4 个字段（camera_idx / cabinet_idx / p_local / pixel）：

- Phase 2 **自身需要** confidence/covariance（相位幅度低的像素必须降权，否则稠密观测的异方差会拖偏解）——这两个字段随 dense UV observation 一起落地；
- timecode / tracking_pose / FIZ 是 Phase 3 字段，本期只在 IPC schema 留 **optional** 位（schemars 向后兼容），避免 Phase 3 破坏性改 IR。

---

## 2. Phase 3 修订（与整体 VP 校准系统联动）

### 2.1 范围修正：Phase 3 实际是另外两个产品

原清单的 lens solver / tracking solver / temporal solver，按文档自己的 §33 划分属于 **CameraGeoCal** 和 **TemporalCal**——它们不是 ScreenModelCal 的"第三阶段"，是并列的独立系统。把两个产品压进一个 Phase 会严重低估范围。

**修订**：Phase 3 拆为两条线——
- **Phase 3a（ScreenModelCal 收口）**：calibration package 打包、OpenTrackIO/UE Lens File/STMap 等导出格式、与外部 solver 的数据接口（消费 §1.3 预留的 optional 字段）；
- **CameraGeoCal / TemporalCal**：独立立项，单独做 spec（各自的工作量都不小于 ScreenModelCal 本体）。

### 2.2 关键顺序修正：Validate Current Mesh 必须从 Phase 3 提前

原清单把 daily drift comparison 排在 Phase 3 末尾，但：

- §36 最终结论把它列为三大产品支柱之一（Quick 解决"快"、Precision 解决"准"、**Validate 解决"每天还能不能用"**）；
- §34.2 每日开机流程整个建立在它之上——它是使用频率最高的入口（每天），而 Quick/Precision 是低频动作（搭台/大修时）；
- 它的依赖**只有 Phase 1 已有能力**：显示少量 VP-QSP 帧 → 检测 → 对现有 as-built mesh 做重投影比对 → 报告漂移。离线骨架（`compare-known` CLI 命令、pose report、检测器）全部现成。

**修订**：新设 **Phase 1.5：Validate Current Mesh**，排在 Phase 2 之前。这是当前性价比最高的未开发功能——成本最低（复用件最多）、用户价值兑现最快，且不依赖精密模式存在。

### 2.3 UE 出口顺序修正

Phase 3 列了 UE Lens File / STMap，但 Phase 1 的 `pose-obj --target unreal` 至今是 no-op（接受参数、零适配、米级 neutral 帧直接出文件）。**要么在 Phase 1 收尾时修好 unreal OBJ 适配（core 的 adapt_to_target 已有 unreal 轴/单位定义，只是 pose-obj 没接），要么 CLI 显式拒绝该 target**——不能带着一个假出口进入 Phase 3 再谈 UE 生态。

### 2.4 保留项

calibration package、temporal solver 留在 Phase 3 / 独立产品线没有问题（§27.5 已正确判断 timing 层非屏幕反算必需）。§34.1 流程第 1 步"导入 CAD/LED topology"目前只支持 nominal 网格参数，任意 CAD 导入是否需要，建议在 Phase 3a 立项时按真实需求决定，不预做。

---

## 3. 修订后的路线总览

```text
Phase 1 收尾（短）：
  unreal 出口修复或显式拒绝；normal/inverted 决策（实现或从文档移除）；
  P2.3–P3.9 箱体方案决策（低分辨率 marker 方案 / 明确走 SL）

Phase 1.5 Validate Current Mesh（新增，提前自原 Phase 3）：
  VP-QSP 少帧检测 + 现有 mesh 重投影比对 + 漂移报告
  复用：compare-known 骨架、pose report、检测器
  按 CLI 契约交付：lmt visual validate-mesh + E2E + agents-cli.md

底座修复（与 Phase 1.5 并行）：
  法线镜像 / y 混用 / 传递桥接 / 标定门 / converged 如实上报 /
  holdout 指标接线 / DPX 10-bit / 导出 UV

Phase 2 Precision Screen Scan（修订清单）：
  P-0 单箱体物理实验（go/no-go）
  多频相位 decoder（Gray Code 降级 fallback）+ VP-QSP 粗定位
  统一 Observation 落地（confidence/covariance；Phase 3 字段留 optional 位）
  解析 Jacobian / 稀疏后端
  multi-screen joint solve + 墙地夹角表达   ← 原清单缺失的结构性项
  control point 约束接口（替代 measured.yaml 死端）
  B-spline 形变层 + UV-to-XYZ + holdout 指标的 validation report
  capture planner 修复（瞄点参数化 + 正确法线）

Phase 3a ScreenModelCal 收口：
  calibration package / OpenTrackIO / UE Lens File / STMap 导出与接口

CameraGeoCal、TemporalCal：独立立项，单独 spec
```

---

**Knowledge Sources**: `docs/LED_ScreenModelCal_Dual_Mode_Scheme.md` §30–36；`_walkthrough/algo-review-2026-06-10.md`；`docs/precision-scan-improvement-plan.md`。
**External Inputs**: 无新增——本文全部判断基于上述文档与本仓库代码逐项核对（关键词存在性、Observation 字段、compare-known/export 入口均已实测）。
