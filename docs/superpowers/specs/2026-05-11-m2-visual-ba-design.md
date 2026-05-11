# M2 视觉反算 Adapter — Design Spec

**Date**: 2026-05-11
**Author**: m2 session (LED Mesh Toolkit)
**Status**: Approved (brainstorming → writing-plans)
**Predecessor**: `2026-05-10-led-mesh-toolkit-design.md` §5 (M2)
**Base tag**: `m0.1-complete`

---

## 1. 范围与边界

### 1.1 在范围

- `crates/adapter-visual-ba/`：完整实现 Rust adapter（headless）
- `python-sidecar/`：Python 子进程实现（OpenCV + scipy + numpy）
- ChArUco pattern 生成器（在 sidecar 内）
- 镜头标定子命令（在 sidecar 内）
- PyInstaller 打包：Windows + macOS
- PoC 验证工具 + PoC 闸门

### 1.2 明确不在范围

| 项 | 归属 |
|---|---|
| GUI 视图（`/charuco`、`/photoplan`、`/import` 图像部分） | M0.2（GUI shell） |
| H 方案（ChArUco BA + LED 几何先验 refinement） | M3 |
| Linux 平台打包 | 不支持 |
| B 方案（GCP 锚定） | 砍掉，不实现 |
| 修改 `core` crate（IR / 重建 / 导出） | 不动；如发现接口不够，独立 PR + 与 M1 session sync |

### 1.3 设计约束

- IR 已冻结（`docs/IR-FROZEN.md`），M2 输出契约 = `MeasuredPoints { source: VisualBA, uncertainty: Covariance3x3 }`，已在 origin 坐标系下
- 与 M1 session 在 git 上零冲突（仅动 `crates/adapter-visual-ba/`、`python-sidecar/`、新建 `docs/`）
- 子进程模式参考 UECM（`/Users/bip.lan/AIWorkspace/vp/ue-cache-manager` 的 `core/powershell.rs`），但因 BA 运行时 1–5 分钟，stdout 改为流式 NDJSON 以推进度

---

## 2. 整体架构

```
[Tauri command / CLI bin]            （M0.2 后接入；M2 阶段用 cargo test + bin 入口）
   │
   ▼
[Rust: lmt-adapter-visual-ba]
   │   - 输入校验
   │   - tokio::process::Command spawn
   │   - stdin: 一次性 JSON
   │   - stdout: BufReader::lines() → NDJSON 解析
   │   - 进度通过 mpsc channel 回传
   │   - cancel = child.kill()
   ▼
[Python sidecar (PyInstaller --onefile)]
   ├─ subcmd: calibrate         （棋盘格 → intrinsics.json）
   ├─ subcmd: generate_pattern  （cabinet_array → 三件套 PNG/meta）
   └─ subcmd: reconstruct       （images + intrinsics → MeasuredPoints）
   │
   │ exit 0 = 正常完成（result 事件已写入 stdout）
   │ exit !0 = 错误（error 事件已写入 stdout）
   │ SIGKILL = 被 Rust cancel
   ▼
[Rust: 解析 NDJSON → MeasuredPoints (IR) → core::auto_reconstruct]
```

---

## 3. 组件分解

| 组件 | 路径 | 职责 |
|---|---|---|
| **Rust adapter** | `crates/adapter-visual-ba/` | 子进程管理；输入/输出校验；NDJSON 解析；进度 channel；类型化错误；输出 `MeasuredPoints` |
| **Python sidecar entrypoint** | `python-sidecar/src/lmt_vba_sidecar/__main__.py` | argparse 子命令路由；统一 NDJSON 写出；异常 → error 事件 + 非 0 退出 |
| **Pattern generator** | `python-sidecar/src/lmt_vba_sidecar/pattern.py` | 输入 cabinet_array → 输出三件套（per-cabinet PNG + 整屏 PNG + pattern_meta.json） |
| **Calibration** | `python-sidecar/src/lmt_vba_sidecar/calibrate.py` | 棋盘格图像 → intrinsics.json（K、dist_coeffs、reproj_error） |
| **Reconstruct pipeline** | `python-sidecar/src/lmt_vba_sidecar/reconstruct.py` | ChArUco 检测 + 亚像素细化 + scipy BA + Procrustes 对齐（A/C 共用） |
| **IPC types** | `python-sidecar/src/lmt_vba_sidecar/ipc.py` + `crates/adapter-visual-ba/src/ipc.rs` | 双方共享 schema（pydantic + serde），以 JSON Schema 文件作 single source of truth |
| **PoC 对比工具** | `crates/adapter-visual-ba/src/bin/poc_compare.rs` | 输入两组 `MeasuredPoints`（visual + 全站仪）→ 输出 RMS / per-point error / 95th percentile |
| **打包脚本** | `python-sidecar/build_exe.ps1`、`build_exe.sh` | PyInstaller --onefile，输出到 `target/sidecar-vendor/<platform>/` |

---

## 4. 数据流

```
[准备阶段：镜头标定]
  现场前一次性。棋盘格图片 → sidecar.calibrate → intrinsics.json
  intrinsics 与镜头/相机绑定（同一镜头同一光圈可复用）

[准备阶段：Pattern 生成]
  项目 YAML → sidecar.generate_pattern → patterns/
    ├── cabinets/V001_R001.png        （单箱 PNG，调试 / 重生用）
    ├── cabinets/V001_R002.png
    ├── ...
    ├── full_screen.png               （整屏拼接，Disguise media bin 直接拿）
    └── pattern_meta.json             （cabinet ↔ ArUco ID 映射，Rust 解码用）

[现场：拍摄]
  按 SOP（spec §5.3）拍 30–60 张图 → images/

[导入：重建]
  adapter.reconstruct(project_yaml, intrinsics_json, images_dir, pattern_meta_json)
    ↓ spawn sidecar reconstruct
    ↓ 收 NDJSON 进度事件 → emit 给上层
    ↓ 收 result 事件
  → MeasuredPoints (IR)
    ↓ core::auto_reconstruct
  → ReconstructedSurface
    ↓ surface_to_mesh_output + write_obj
  → OBJ
```

---

## 5. 坐标系策略：A 主 + C fallback

`reconstruct` 命令通过 `frame_strategy` 字段切换两种模式，**两种模式共用同一段 Procrustes 对齐实现**，差别只在"参考点的物理坐标从哪来"。

### 5.1 A：标称 anchoring（默认）

- sidecar 内部按 `cabinet_array` + `shape_prior` 推算每个 ChArUco ID 的标称物理位置（origin 坐标系下）
- BA 输出（相对相机锚点）→ Procrustes 对齐到标称位置 → origin 坐标系下的绝对位置
- 失效模式：真实屏与先验形状偏差大 → 整体漂移
- M2 主路径，目标"独立可用"（不依赖全站仪）

### 5.2 C：3-参考点 fallback

- 与 M1 一致：用户用全站仪测 origin / X 轴 / XY 平面三点
- sidecar 用这 3 个测点的 ChArUco ID 替代标称坐标做 Procrustes
- 实现复用 A 的 Procrustes 代码
- 失效模式：3 点距离过近 / 共线 → 长基线外推漂移
- 当 A 在某项目精度不足时启用

### 5.3 B 不做的理由

GCP 方案需要在屏幕外布置全站仪测过的 marker —— 仍依赖全站仪，与 M2 "独立可用" 的卖点冲突，不实现。

---

## 6. IPC 协议

### 6.1 进程模型

- **One-shot**：每次 BA 调用 spawn 一个新 sidecar 进程
- 启动开销几百毫秒，相对 1–5 分钟运行时间忽略
- 简化 cancel 实现（直接 kill child）和资源管理（不调用时不占内存）

### 6.2 输入（stdin，一次性 JSON）

```json
{
  "command": "reconstruct",
  "version": 1,
  "project": {
    "origin": { /* CoordinateFrame YAML 同款 */ },
    "cabinet_array": { /* 同 IR */ },
    "shape_prior": "Curved",
    "frame_strategy": "nominal_anchoring",
    "frame_anchors": null
  },
  "images": ["/abs/path/img1.jpg", "..."],
  "intrinsics": {
    "K": [[fx,0,cx],[0,fy,cy],[0,0,1]],
    "dist_coeffs": [k1,k2,p1,p2,k3]
  },
  "pattern_meta": { /* 同 generate_pattern 输出 */ }
}
```

`frame_strategy` ∈ `{"nominal_anchoring", "three_points"}`。
`frame_anchors` 在 `three_points` 模式下传 3 个全站仪测点，否则为 `null`。

`calibrate` 命令的输入 schema 不同（接受棋盘格图像目录 + 棋盘规格），见 §6.5。
`generate_pattern` 命令接受 `project` 字段，输出三件套路径。

### 6.3 输出（stdout，NDJSON）

每行一个 JSON 对象，事件类型枚举：

- `progress`：进度事件
  ```json
  {"event":"progress","stage":"detect_charuco","percent":0.05,"message":"3/30 images"}
  ```
  `stage` ∈ `{"load","detect_charuco","subpixel_refine","bundle_adjustment","procrustes_align","output"}`
  `percent` ∈ `[0.0, 1.0]`

- `warning`：非致命警告（例如某 cabinet 观测相机数偏少）
  ```json
  {"event":"warning","code":"low_observation","cabinet":"V003_R002","message":"only 3 cameras observe"}
  ```

- `result`：最终成功结果（每次运行**仅一条**，写在最后）
  ```json
  {"event":"result","data":{
    "measured_points":[ /* MeasuredPoint 列表，已在 origin 坐标系 */ ],
    "ba_stats":{"rms_reprojection_px":0.42,"iterations":18,"converged":true},
    "frame_strategy_used":"nominal_anchoring"
  }}
  ```

- `error`：致命错误（每次运行最多一条，写在最后）
  ```json
  {"event":"error","code":"detection_failed","message":"only 2/30 images had valid ChArUco","fatal":true}
  ```

退出码：
- `0`：完成（result 事件已写）
- `1`：错误（error 事件已写）
- `SIGKILL` / 非正常退出：被 Rust cancel 或 sidecar 崩溃

### 6.4 错误码枚举（初版）

| code | 含义 |
|---|---|
| `invalid_input` | 输入 JSON schema 校验失败 |
| `image_load_failed` | 图像无法读取 / 不是支持格式 |
| `detection_failed` | ChArUco 检测命中率过低（< 50% 图像） |
| `ba_diverged` | scipy.least_squares 不收敛 |
| `procrustes_failed` | 锚点不足 / 退化（共线 / 共面） |
| `intrinsics_invalid` | K 矩阵或畸变参数明显错误（焦距 ≤ 0 等） |
| `internal_error` | 兜底未分类异常 |

### 6.5 IPC schema 单一来源

- `python-sidecar/schema/ipc.schema.json`：JSON Schema 文件
- Python 端用 `pydantic` 生成模型；启动时 schema 自检
- Rust 端用 `serde` + `schemars`（可选）生成模型；CI 步骤校验两端 schema 一致

---

## 7. 不确定度传播

BA 收敛后，sidecar 从 Jacobian 提取每个 3D 点的协方差：
```
cov_3x3 = sigma_residual^2 * (J^T @ J)^{-1}_block(point)
```
注入到 IR（与 `crates/core/src/point.rs` 实际定义一致）：
```rust
MeasuredPoint {
    name: "MAIN_V003_R002",
    position: Vector3::new(x, y, z),                  // model / origin frame，单位 m
    uncertainty: Uncertainty::Covariance3x3(cov_3x3),
    source: PointSource::VisualBA { camera_count: 5 }, // 该点的有效观测相机数
}
```

如果 BA 不返回可用协方差（例如点观测数 < 2），fallback 到 `Uncertainty::Isotropic` + 记录 warning 事件。

---

## 8. 打包

### 8.1 PyInstaller 输出

| 平台 | 路径 | Runner |
|---|---|---|
| Windows x86_64 | `target/sidecar-vendor/windows-x86_64/lmt-vba-sidecar.exe` | `windows-latest` |
| macOS arm64 | `target/sidecar-vendor/darwin-arm64/lmt-vba-sidecar` | `macos-14` |

`--onefile` 模式：单 exe / 单二进制，启动时 PyInstaller 解压到临时目录。启动开销 1–2 秒，对 1–5 分钟的 BA 忽略。

### 8.2 Rust 端定位 sidecar 的查找顺序

1. 环境变量 `LMT_VBA_SIDECAR_PATH`（dev / 测试覆盖用）
2. Cargo target 下的 vendor 路径：`target/sidecar-vendor/<platform>/lmt-vba-sidecar[.exe]`
3. 系统 PATH 里同名二进制（兜底）

### 8.3 Dev 模式

不打包，直接 `python -m lmt_vba_sidecar` 跑（同样实现 entry-point）。`LMT_VBA_SIDECAR_PATH=python -m lmt_vba_sidecar` 通过环境变量切换。

### 8.4 Tauri 集成

M2 阶段不集成 Tauri（headless）。`target/sidecar-vendor/<platform>/` 路径预留，M0.2 GUI shell 起来后由 Tauri `bundle.resources` 引用。

---

## 9. PoC 闸门

### 9.1 PoC 何时跑

Plan 分两段。**A 模式和 C 模式都必须在 Part A 交付**（PoC 要拿两者对比，不能让 gate 依赖未实现的代码）：

- **Part A — MVP**（PoC 之前必须交付）：
  - sidecar 三个子命令在合成数据上跑通
  - Reconstruct 子命令同时支持 A 模式（标称 anchoring）和 C 模式（3-参考点 fallback），共用 Procrustes
  - 两种模式各自的单元测试覆盖
  - Rust adapter 端到端跑通 reconstruct 流程（两种模式都通过 IR 转换输出）
  - ChArUco pattern 三件套能输出
  - PoC 对比工具能跑（含 holdout 分离逻辑，见 §9.3）

- **PoC 闸门**（用户在外部安排，plan 里是 manual gate）

- **Part B — 生产化**（PoC 通过后做）：
  - 错误处理 + cancel 机制完整化
  - PyInstaller 打包 + CI 集成
  - 精度报告 / 日志 / 诊断
  - Windows + macOS build 验证

### 9.2 PoC 测试设计

| 项 | 配置 |
|---|---|
| LED 测试墙 | 4×4 箱体（1m × 1m） |
| Ground truth | 全站仪测**全部** ChArUco 角点（不只 3 个），按命名 `MAIN_V<col>_R<row>` 与 sidecar 输出对应 |
| 拍摄 | 10–15 张图（远 / 中 / 近混合） |
| 模式 | A 模式 + C 模式各跑一次（同一组照片） |

### 9.3 通过门槛与 holdout 策略

PoC 报告必须区分**对齐 anchor（拿来做 Procrustes 的点）**和**验证点（不参与对齐，只参与 RMS）**：

- **A 模式**：Procrustes 用全部 ChArUco 标称位置做对齐目标。RMS 是 BA 点 → 标称位置的对齐残差；这反映"实际形状 vs 先验形状"的偏差，本身就是有效指标。无需 holdout。
- **C 模式**：Procrustes 只用 3 个 `frame_anchors` 做对齐。**RMS 必须排除这 3 个 anchor**，仅在剩余的 `(N-3)` 个验证点上计算，避免在训练集上测精度。3 个 anchor 在屏幕上**空间分布要尽量分散**（不共线、近角落），PoC 报告需明确列出 anchor ID。

通过门槛：

| 模式 | 指标 | 门槛 |
|---|---|---|
| A（标称 anchoring） | 全部 ChArUco 点对齐残差 RMS | < 10mm（宽松，留 M3 refinement 空间） |
| C（3-参考点） | **holdout** 验证点（N-3 个）RMS | < 5mm（VP 标准，spec §10.2 PoC 阶段定义） |
| C（3-参考点） | 95th percentile（同 holdout 集） | < 8mm（避免单点拉低均值掩盖局部漂移） |

判定：
- A 通过 + C 通过 → 进 Part B，A 作为默认
- 仅 C 通过 → 进 Part B，但 A 标记为"实验性"，默认走 C
- 都不通过 → plan 暂停，分析失败原因（pattern / SOP / 算法）→ 重新设计

### 9.4 PoC 交付物

- `docs/poc/2026-XX-XX-m2-poc-report.md`：包含
  - 测试条件（屏幕规格、相机、镜头、光照）
  - 原始数据路径（图像 + 全站仪 CSV）
  - 列出的 C 模式 anchor ID + 它们在屏上的位置
  - A 模式：全点对齐残差 RMS、95th percentile、per-point error 表格
  - C 模式：holdout 集的 RMS、95th percentile、per-point error 表格；anchor 残差单独列出（应接近 0）
  - 通过/不通过结论 + 下一步动作

---

## 10. 验收标准

照搬 spec §10.2，外加：

| 测试项 | 通过标准 |
|---|---|
| sidecar 三个子命令端到端单元测试 | 合成数据全过 |
| Rust adapter 端到端测试 | 合成数据通过；spawn / NDJSON / 错误 / cancel 全覆盖 |
| PoC 报告 | 至少 C 模式 RMS < 5mm |
| PyInstaller Windows build | 单 exe 在干净 Windows 上能跑（无 Python 运行时） |
| PyInstaller macOS build | 单二进制在干净 macOS arm64 上能跑 |
| Cancel 机制 | 运行中调 cancel，sidecar 进程在 < 5s 内退出 |
| 异常退出捕获 | sidecar 崩溃 / 非 0 退出，Rust 侧返回类型化错误 |

---

## 11. Plan 结构预告

`writing-plans` 阶段产出的实施计划预计 **20–24 个 task**，分 7 块：

1. **Sidecar 骨架**：pyproject、CLI 入口、IPC schema 文件、空命令路由、合成数据 fixture
2. **Pattern 生成**：单箱 PNG → 整屏拼接 → pattern_meta.json
3. **Calibration 子命令**：棋盘格检测 → intrinsics 输出
4. **Reconstruct 核心**：检测 + 亚像素 + BA + Procrustes（A + C 共用）+ 两种模式的单测
5. **Rust adapter**：spawn + NDJSON + 进度 channel + 错误 + IR 转换（两种模式都覆盖）
6. **PoC 闸门**：对比工具（含 holdout 分离）+ 报告模板 + manual gate 标识
7. **生产化**：错误/cancel 完整化 + PyInstaller + CI + Windows/macOS build 验证

每个 task 在 commit 前用 `/codex:adversarial-review` 走一遍。

---

## 12. 风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| PoC 不通过 | 高 | Part A / Part B 分段；不通过则 plan 暂停，不浪费 Part B 工作 |
| OpenCV ChArUco 在 Windows + macOS 行为差异 | 中 | sidecar 三个子命令各加合成数据单元测试，CI 双平台跑 |
| BA 收敛性 | 中 | 用 OpenCV 标准 baseline；不收敛 fallback 到逐图 PnP |
| PyInstaller 双平台 build | 中 | Part B 集成 GitHub Actions；先 Windows，再 macOS |
| Procrustes 退化（共线 / 共面锚点） | 中 | 启动时检查锚点几何条件，不满足 → `procrustes_failed` 错误 |
| 协方差不可用时降级 | 低 | fallback `Isotropic` + warning 事件 |

---

## 13. 与 IR 的契约

- 输入：项目 YAML（与 M1 共用 `Project` schema）+ 图像目录 + intrinsics
- 输出：`MeasuredPoints { points, source: VisualBA }`，已在 origin 坐标系
- IR 不动；如果发现接口不够（如 BA 元数据字段），独立 PR + 与 M1 session sync

---

## 附录 A：Cargo / Python 依赖锁定

### Rust（`crates/adapter-visual-ba/Cargo.toml`）

预计新增依赖：
- `tokio = { workspace = true, features = ["process", "io-util"] }`
- `serde`、`serde_json`（workspace 已有）
- `thiserror`（错误类型）
- `tokio-util` 或 `futures`（用于 BufReader::lines async）
- 已有：`lmt-core`（IR）

### Python（`python-sidecar/pyproject.toml`）

```toml
[project]
name = "lmt-vba-sidecar"
requires-python = ">=3.10,<3.13"

dependencies = [
    "numpy>=1.24,<2.0",
    "scipy>=1.10,<2.0",
    "opencv-contrib-python>=4.8,<5.0",
    "pydantic>=2.0,<3.0",
]

[project.scripts]
lmt-vba-sidecar = "lmt_vba_sidecar.__main__:main"

[build-system]
requires = ["setuptools>=68"]
build-backend = "setuptools.build_meta"
```

PyInstaller 作 dev 依赖（不进 runtime requirements）。
