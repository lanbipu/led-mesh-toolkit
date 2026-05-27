# Path B 修复设计 · 夹角屏 BA 收敛 + 位姿直出 OBJ

> 日期：2026-05-27
> 范围：让相机视觉路径（Path B，零全站仪）对夹角/折叠屏走通到 disguise。

## 1. 背景与目标

Path B = 拍屏 ChArUco 照片 → 自标内参 → BA 反算每块箱体位姿，全程不用全站仪。
当前有两个阻塞，分别对应本设计的两个问题。

成功判据：
- **P1**：`lmt visual reconstruct` 对台架双屏（夹角约 113°）正确收敛，
  `cabinet_pose_report.json` 里两屏法向夹角 ≈ 真值、`ba_rms` 低（PoC 实测 4.99px）。
  这是 `VALIDATION_PATH_B.md` 的验证终点（`compare-known` 看夹角/间距）。
- **P2**：能把反算出的每块屏位姿，单独导出成各自的 OBJ，顶点落在共享世界系，
  导入 disguise 后相对位置正确。

## 2. 核实结论（修正原任务描述）

原任务把问题二描述为"在 Python `nominal.py` 实现 folded，`lmt reconstruct surface`
就能跑折叠屏"。读源码核实后，这个描述与实际架构不符，本设计据实修正：

1. **`lmt reconstruct surface` 是纯 Rust**（`lmt_app::reconstruct::run_reconstruction`
   → `lmt_core::reconstruct`），**不调用 Python `nominal.py`**。后者只用于 visual BA
   的初始化种子；P1 用桥接相机取代该种子后，`nominal.py` 退化为 fallback。
2. **visual → surface 这条链对所有 shape_prior 都没通，不只是 folded**：
   `visual reconstruct` 写出的 `measured.yaml` 是「每块 cabinet 一个中心点、命名
   `MAIN_V000_R000`（0-based）、`sampling_mode: grid`」；而所有 Rust reconstructor
   （DirectLink / Nominal / RadialBasis / BoundaryInterp）都要求点命名为
   `{screen_id}_V{1-based}_R{1-based}` 的网格顶点。两者对不上 → `auto_reconstruct`
   必然报 "no applicable reconstructor"。台架现有的 `BENCH_mesh.obj` 头部写明
   "generated from cabinet_pose_report.json"——是脚本绕过 `reconstruct surface`
   直接从 pose report 的 corners 生成的。
3. **`lmt export obj` 也消费不了 pose report**：它读 DB 里某条 reconstruction run 的
   `ReconstructionReport.surface`，而 run 来自 `reconstruct surface`，链路同样断在第 2 点。

**方向决策（已与用户对齐）**：问题二走「pose report → 每屏单独 OBJ（世界坐标）」的
直出路径，**不**修 `reconstruct surface`、**不**实现 Python `nominal.py` folded
（该路径下无消费者，写了就是死代码）。

## 3. 问题一：桥接相机初始化

### 3.1 根因

`python-sidecar/src/lmt_vba_sidecar/reconstruct.py` 的 `run_reconstruct`，第 7 步
（约 line 336–344）对非根 cabinet 用 `(np.eye(3), t_mm)` 初始化——单位旋转 = 假设所有
cabinet 与根 cabinet 共面。对夹角屏（法向差 ~67°），初值离真值太远，BA 收敛到错误局部
最优（台架实测 ba_rms 2911px，两屏被压成共面）。

### 3.2 解法（PoC 已验证）

在 BA 之前加「桥接相机」估计非根 cabinet 的世界系初值：
- 对每张同时看到根 cabinet（≥4 角）和某非根 cabinet（≥4 角）的照片，做两次 PnP：
  `camera_from_root` 与 `camera_from_nonroot`；
- 由此求 `world_from_nonroot`：`R01 = Rc0ᵀ·Rc1`，`t01 = Rc0ᵀ·(tc1 − tc0)`；
- 多张桥接照片：旋转用 SVD 平均，平移用各分量 median；
- 用 `(R01, t01)` 作为该非根 cabinet 的 BA 初值，替换原 flat nominal。
- 没有桥接照片的 cabinet：fallback 到现有 nominal 平移 + 单位旋转（保留 flat/curved）。

`model_constrained_ba` 本身不动（它已接受 `init_cabinets` 的 `(R, t)`）。

### 3.3 代码结构（可测）

把桥接估计抽成纯函数，便于单测：

```python
def estimate_nonroot_cabinet_init(
    per_view_cab_corners: dict[tuple[int,int], list[tuple[np.ndarray, np.ndarray]]],
    root_idx: int,
    K: np.ndarray,
    min_corners: int = MIN_PNP_CORNERS,
) -> dict[int, tuple[np.ndarray, np.ndarray]]:
    """非根 cabinet_idx → (R_world_from_cab, t) 的桥接估计；无桥接的 cabinet 不在返回里。"""
```

`run_reconstruct` 第 7 步：先算 `bridge = estimate_nonroot_cabinet_init(...)`，
再构 `init_cabinets`：根 = (I, 0)；非根优先用 `bridge[idx]`，否则 nominal 平移 + I。

### 3.4 局限（明确写下，不做）

只做「根 ↔ 非根」直接桥接 + nominal fallback。对「与根无共视照片的远端 cabinet」
（大屏多箱体链式拓扑）不做传递桥接——当前验证目标是台架双屏 + 小屏，YAGNI。代码里留注释。

### 3.5 测试

- `test_reconstruct.py` 加单测：合成两块夹角板 + 一组合成相机（已知位姿），构造
  `per_view_cab_corners`，断言 `estimate_nonroot_cabinet_init` 返回的旋转/平移接近真值。
- 端到端断言：喂合成观测跑 `run_reconstruct` 主流程的 BA，断言收敛后两 cabinet 法向夹角
  ≈ 真值、`ba_rms` < 阈值。合成数据在测试内构造，不依赖仓库外的台架照片。
- 改完重建 sidecar binary：`python-sidecar/build_exe.sh`。

## 4. 问题二：pose report → 每屏单独 OBJ

### 4.1 输入 / 输出

- 输入：`cabinet_pose_report.json`（`CabinetPoseReport`，含 `cabinet_poses[].corners_mm`，
  4 角 BL,BR,TR,TL，世界系 mm）；`target`（disguise / unreal / neutral）；输出目录。
- 输出：每块 cabinet 一个 OBJ，文件名 `<cabinet_id>_<target>.obj`（pose report 不带
  screen_id，故文件名只用 cabinet_id），顶点为**共享世界系**坐标（相对位置烘进几何）。

### 4.2 复用现成导出管线

每块屏当成一个 1×1 网格（4 个世界系角点）：
1. mm → m，构 `ReconstructedSurface { topology: 1×1, vertices: 4 角, uv: compute_grid_uv(1×1) }`；
2. `cabinet_array = 1×1`（该屏尺寸）；
3. 调 `lmt_core::export::build::surface_to_mesh_output(surface, cabinet_array, target, weld_tol)`
   ——复用 disguise 的单位换算、winding 反转、UV 镜像；
4. 写 OBJ。

复用 `surface_to_mesh_output` 保证每屏 OBJ 与 Path A 的 disguise 约定一致。

### 4.3 分层（遵守 CLI 契约）

- **lmt-shared**：新增读 pose report 的 DTO（`cabinet_id` + `corners_mm`），
  派生 `Serialize/Deserialize + JsonSchema`，加进 `schema::dump_all()`。
  导出结果 summary DTO（写了哪些文件）同样派生 JsonSchema。
- **lmt-app**：`export::run_export_pose_obj(pose_report_path, target, out_dir)` —— 读 JSON、
  逐 cabinet 构 surface、调 `surface_to_mesh_output`、写文件、返回 summary。业务逻辑在此。
- **lmt-cli**：`ExportCmd` 加子命令（如 `pose-obj <pose_report> <target> --out-dir <dir>`），
  thin shim；destructive（写文件）走 `gate_destructive` + `--yes` / `--dry-run`。
- **Tauri shim**：若 GUI 需要，加 `#[tauri::command]` thin wrapper（可后置；至少在
  `docs/agents-cli.md` 说明）。

### 4.4 测试

- `cli_e2e.rs` 新 case：写一个两屏 pose report fixture → 跑 `export pose-obj ... --yes`
  → 断言生成两个 OBJ、各含 4 顶点、顶点坐标匹配 corners（经 target adapter 变换后）、
  两屏相对位置保留；外加 dry-run（不写文件）+ 错误信封（坏 JSON / 缺文件）各一。
- lmt-core 层：1×1 surface 过 `surface_to_mesh_output` 的单测（若现有覆盖不足）。

### 4.5 docs

`docs/agents-cli.md`：命令表加新行 + side_effect 标 destructive；若新增错误码则更新错误码表。

## 5. 范围外（明确不做）

- Python `nominal.py` folded：本方向无消费者，不实现。
- Rust folded surface fit（`SurfaceFitReconstructor` / `NominalReconstructor`）：option A
  绕过 `reconstruct surface`，不动。
- 传递式多箱体桥接：见 3.4。
- `reconstruct.py` 里硬编码的 `MAIN_` 前缀（应是真实 screen_id）：对本方向（读 pose report，
  用裸 `cabinet_id`）无影响，保持改动外科手术式，**不**顺手改，仅在此记录。

## 6. 交付清单

P1：① `estimate_nonroot_cabinet_init` + 接入 `run_reconstruct`；② pytest（单测 + 端到端合成）；
③ 重建 sidecar binary。
P2：① lmt-shared DTO（+schema dump）；② lmt-app helper；③ lmt-cli 子命令（+dry-run/--yes）；
④ cli_e2e（happy + dry-run + 错误信封）；⑤ docs/agents-cli.md。
