# VP-QSP 快速屏幕校准管线 — 设计与集成方案

版本：v1.0 · 分支 `feat/vp-qsp-fast-cal`
定位：用**自编码 marker** 替换 ChArUco，作为 lmt 的默认快速屏幕模型校准（Quick Screen
Fit）管线。复用 lmt 现有的 model-constrained per-cabinet BA 后端。

总纲文档：`docs/LED_ScreenModelCal_Dual_Mode_Scheme.md`（Part A = VP-QSP 快速模式）。
本文件把 Part A 的方向性描述落地成精确实现 spec + 集成契约。

---

## 1. 为什么替换 ChArUco

ChArUco 方案共享一本 `DICT_6X6_1000`（1000 个 marker）字典，逐箱切片分配 ID：

```
最大 cabinet 数 = floor(1000 / markers_per_cabinet)
markers_per_cabinet = (squares_x * squares_y) // 2     # 交替格放 ArUco
```

非方箱体（典型 16:9）≈ 72 markers/箱 → **~13 cabinets 硬上限**（见
`pattern.py:212-219` 的容量 guard），无法支撑真实 LED Wall（2000+ cabinets）。

**VP-QSP 的根本区别**：每个 marker 在图案里**自编码自己所属的 cabinet (col,row) + 屏内
local 位置 + screen_id**。没有全局 ID 分配、没有路由表、没有字典容量墙。解码直接给出
cabinet 与 local 位置，BA 所需的 `(cabinet_idx, p_local)` 全部来自单个 marker。

---

## 2. Marker 编码（含 screen_id）

延伸 vpcal VP-QCP（24-bit = 6-bit row + 8-bit col + 2-bit sub + 8-bit CRC，6×6 grid）
到更宽的地址 + 更大的 grid。

### 2.1 Bit 布局

```
payload 24 bit  =  screen_id(4) | cab_col(7) | cab_row(7) | local_id(6)
codeword 32 bit =  payload(24) << 8 | CRC-8(8)
```

| 字段 | 宽 | 容量 | 含义 |
|---|---|---|---|
| `screen_id` | 4 | 16 屏 | Volume 内哪一块屏（wall/floor/ceiling/side…） |
| `cab_col` | 7 | 128 列 | cabinet 列号（0-based） |
| `cab_row` | 7 | 128 行 | cabinet 行号（0-based） |
| `local_id` | 6 | 64 / 箱 | marker 在该 cabinet 内的局部网格编号 |
| `CRC-8` | 8 | — | CRC-8/AUTOSAR，**仅检错不纠错**，CRC 失败的 marker 直接丢弃 |

容量 = 16 屏 × 128×128 箱 × 64 marker/箱 ≫ 2000-cabinet 目标。字段宽是 `vpqsp_codec.py`
里的命名常量，可调。

### 2.2 CRC

CRC-8/AUTOSAR（poly `0x2F`, init `0xFF`, xorout `0xFF`, 不反射），自检值
`crc8(b"123456789") == 0xDF`（从 vpcal 逐位移植）。CRC 输入 = **24-bit payload 的 3 字节
大端序列化** `[payload>>16, payload>>8, payload]`（vpcal 是 16-bit/2 字节，这里自然加宽到
3 字节）。跨实现（生成端 vs 检测端）必须就 payload 位宽 + 字节序列化 + 大端一致。

### 2.3 Cell grid 视觉结构

```
7×7 数据格 + 1 格亮边框（→ 9×9 effective），暗 panel
4 角点定向：TL=1, TR=1, BL=1, BR=0（非对称 L，解 4 向旋转歧义）
中心 3×3 暗井：bake 一个 Gaussian 白点（subpixel 质心定位）
36 数据格：32 = codeword，余 4 pad=0
```

- 角点 + Gaussian 井 + `_MARGIN_FRAC` 边框 = vpcal 方案原样复用（参数化 `GRID=7`）。
- Gaussian dot 是喂 BA 的关键测量（比硬边角点更稳，抗 defocus / LED bloom）。

---

## 3. 布局（MVP：逐箱规则网格）

**MVP 走逐箱规则 marker 网格**（镜像 ChArUco 的逐箱 board 结构）：每个 cabinet 在自己的
LED canvas 上铺 `markers_x × markers_y` 个 marker，`local_id = mr*markers_x + mc`（row-major）。

- 因为每个 marker 自带全局唯一 payload，规则网格**不会**产生身份歧义 —— 设计文档反对
  「规则重复排列」是针对**纯白点矩阵**（局部邻域不可分），对自编码 marker 不适用。
- 单一真相源：marker 像素中心由 `vpqsp_layout.marker_center_px()` 计算，生成端与重建端
  **共用同一函数**，保证 `p_local` 与实际渲染位置逐像素一致。

**p_local 约定**（与 `screen_mapping.charuco_corner_local_mm` 完全一致，手性关键）：

```
mr, mc = divmod(local_id, markers_x)
cell_w, cell_h = W/markers_x, H/markers_y                 # W,H = cabinet resolution_px
cx_px = (mc + 0.5)*cell_w ;  cy_px = (mr + 0.5)*cell_h    # 图像坐标 y-down, 左上原点
x_mm  = (cx_px - W/2) * pitch_x
y_mm  = (H/2 - cy_px) * pitch_y                           # +y UP（OpenCV board 帧）
p_local = [x_mm, y_mm, 0.0]                               # center origin, mm
```

> **deferred（非 MVP）**：多尺度 Large Anchor、blue-noise 打散、normal/inverted 双帧、
> QLE 镜头联合标定。bit 已为多尺度/screen_id 留位，布局函数可后续替换为 blue-noise 而
> 不动 codec/detector/BA 契约。见 §8。

---

## 4. 集成契约（最小 transport surface）

VP-QSP 不新增 CLI 命令，而是挂到现有 `generate-pattern` / `reconstruct` 的 `--method`：

| 阶段 | 现状 | VP-QSP |
|---|---|---|
| 生成 | `visual generate-pattern --method=charuco` | `--method=vpqsp`（**新默认**） |
| 重建 | `visual reconstruct --method=charuco` | `--method=vpqsp`（**新默认**） |
| BA 后端 | `solve_and_emit` / `model_constrained_ba` | **零改动，完全复用** |

数据流（5 跳，全部复用）：CLI shim → `lmt_app::visual` → `adapter-visual-ba::api` →
`run_sidecar`（子进程 NDJSON）→ sidecar 模块。

### 4.1 sidecar 侧改动

| 文件 | 改动 |
|---|---|
| `vpqsp_codec.py`（新） | bit 布局 + CRC + cellgrid + orientation |
| `vpqsp_layout.py`（新） | 逐箱 marker 网格选择 + `marker_center_px` + `marker_local_mm` + 模板渲染 |
| `vpqsp_detect.py`（新） | 移植 vpcal 检测器，输出 lmt 检测缝 |
| `pattern.py` | `run_generate_pattern` 按 `cmd.method` 分支；vpqsp 走 `vpqsp_pattern.run_generate_pattern_vpqsp` |
| `reconstruct.py` | `run_reconstruct` 按 `manifest.method` 分支；vpqsp 段建 Observation → `solve_and_emit(gauge_strategy="fix_root_cabinet")` |
| `ipc.py` | 加 `VpqspPatternMeta` / `VpqspMarkerGrid`；`GeneratePatternInput.method` |
| `capture_manifest.py` | `CaptureManifest.method` Literal += `"vpqsp"` |

### 4.2 检测缝（VP-QSP 必须产出兼容形状）

```
dict[image_path -> list[{
    "cabinet": (col, row),       # 解码得到，直接路由 cab_to_idx
    "screen_id": int,            # 解码得到，过滤到目标屏
    "local_id": int,             # 解码得到，→ p_local 查表
    "corner_px": [x, y],         # Gaussian 质心，subpixel
}]]
```

reconstruct 把每条 detection 转成 `Observation(camera_idx, cabinet_idx, p_local, pixel)`
（`pixel` 经 `_undistort_obs` 去畸变；`p_local` 经 `marker_local_mm` +y-up）→ 与 charuco
路径**逐字段同构**，下游 stage_a/observability/BA/pose_report/measured 全部共享。

### 4.3 pattern_meta（VP-QSP 变体）

```jsonc
{
  "schema_version": "vpqsp.v1",
  "screen_id_code": 0,                  // 4-bit 数值 screen_id，全屏 marker 共用
  "cabinets": [{
    "col": 0, "row": 0,
    "resolution_px": [W, H],
    "markers_x": 4, "markers_y": 4,
    "marker_px": 120,
    "pixel_pitch_mm": [px, py]
  }]
}
```

`pattern_hash(vpqsp_meta)` = SHA-256(model_dump_json)[:16]，preflight 对 screen_mapping
的 `expected_pattern_hash` 校验（与 charuco 同机制）。

### 4.4 错误码

全部复用现有：`invalid_input`(2) / `detection_failed`(13) / `observability_failed`(17) /
`ba_diverged`(14) / `internal_error`(11)。VP-QSP 无新增失败类，**不**新增错误码/退出码。

---

## 5. CLI 交付项（CLAUDE.md 契约）

| # | 交付 | 内容 |
|---|---|---|
| 1 | lmt-app helper | 无新函数；`run_generate_pattern` / `run_reconstruct` 透传 `method` |
| 2 | Tauri shim | visual 组本就 CLI-only，无新 `#[tauri::command]` |
| 3 | CLI 子命令 | `generate-pattern` / `reconstruct` 的 `--method` 值枚举加 `vpqsp` 并设默认；放开 `method!=charuco→UNSUPPORTED` guard |
| 4 | CLI E2E | `cli_e2e.rs`：vpqsp generate/reconstruct 的 happy / refuse / dry-run / error-envelope |
| 5 | docs/agents-cli.md | 命令表 method 说明更新；错误码表无变化（复用） |
| 6 | DTO schemars | `GeneratePatternResult` / `VisualReconstructResult` 复用；如加 method 字段则 `schema.rs::dump_all()` 注册 + schemars 派生 |

**Not exposed in CLI**：无。VP-QSP 全功能经 `--method=vpqsp` 暴露。

---

## 6. 求解变量（继承现有 BA）

lmt 现有 model-constrained BA 解 **per-cabinet 6-DoF SE(3) pose**（root=(0,0) 为 gauge，
固定 I,0）+ 全相机 pose，cabinet 尺寸/pitch 为已知刚性约束（不解形变）。VP-QSP 只是换了
观测来源（marker Gaussian 质心 ↔ ChArUco 角点），求解变量与 charuco 路径完全一致。
`gauge_strategy="fix_root_cabinet"`（与 charuco 同；align_to_nominal 的 drop-in 帧列为后续）。

观测门槛（复用 `check_observability`）：每 cabinet ≥2 视角、≥8 观测；root (0,0) 必须存在。
故每箱 marker 数须 ≥8（MVP 默认 markers_short=4 → 方箱 16 marker，足够）。

---

## 7. 验收

```bash
cargo test --workspace                       # 含 cli_e2e
python-sidecar/.venv/bin/pytest              # 含 vpqsp codec/layout/detect/reconstruct
./target/debug/lmt --json schema | jq        # 新 DTO 进 schema dump
./target/debug/lmt visual reconstruct --help # method 默认 vpqsp
./target/debug/lmt visual generate-pattern --help
```

端到端验证：合成台（已知 cabinet 姿态 → 渲染 VP-QSP → 检测 → 反算）逐点 3D 误差对账，
对齐现有 `test_reconstruct_per_cabinet` 的合成验收风格。

---

## 8. 后续（documented deferral，超出本 MVP 任务范围）

1. **多尺度布局**：Large Anchor（每 2–4m）+ Medium + Seam Marker 加密；bit 已留 screen_id，
   anchor 可用更少 bit / 更大尺寸单独编码。
2. **blue-noise 打散**：替换 §3 规则网格的布局函数，不动 codec/detector/BA。
3. **normal/inverted 双帧**：detector 已支持 `inverted` 差分参数，生成端补第二帧 + 序列。
4. **QLE 镜头联合标定**：借鉴 vpcal observability gate（列归一 κ、focal-as-scale），从拍屏
   marker 自标 k1/k2/cx/cy（对应 §14 「simple lens correction」）。
5. **align_to_nominal**：VP-QSP 输出落 disguise drop-in 设计帧（复用现有 Procrustes 分支）。
6. **default 翻转风险**：把 `--method` 默认从 charuco 改为 vpqsp 会改变既有无 flag 调用的
   行为；现有 charuco 测试须显式传 `--method=charuco`。
```
