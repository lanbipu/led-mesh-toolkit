# 相机视觉分支重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把相机视觉分支从"自由点 BA"重构为"model-constrained BA",完全不用全站仪,先在合成台 + 两显示器台架验证精度,再全部接入 `lmt` CLI。

**Architecture:** 保留 Rust adapter ↔ Python sidecar 的 NDJSON 子进程架构;sidecar 内核换成"相机 SE3 + 每箱体 SE3 + 已知 local 角点"的 model-constrained BA;ChArUco 与(后续 gated)结构光共用同一 BA 内核,只换对应关系前端;坐标系锚定在屏幕自身(零全站仪),尺度来自已知像素间距。

**Tech Stack:** Python sidecar(numpy / scipy.optimize.least_squares / opencv-contrib `CharucoDetector`)、Rust(`adapter-visual-ba` tokio 子进程 + `lmt-app` 服务层 + `lmt-cli` clap + `lmt-shared` serde/schemars envelope)。

**Spec:** `docs/superpowers/specs/2026-05-24-camera-branch-rework-design.md`(最终版 v2)。

**阶段总览**(对应 spec §14;本 plan 把 spec 的概念阶段细化为可执行任务):
- **Phase 0** — 纯 Python 算法 + 合成几何台,pytest 验证(证明 model-constrained ≫ 自由点,定精度门槛)。**不碰 Rust/CLI**。
- **Phase 1** — ChArUco 前端 + sidecar reconstruct 重构 + **全部 Rust CLI surface**(5 个 `lmt visual` op)。
- **Phase 2** — 两显示器台架验证(已知几何对账工具 + 手动采集协议)。
- **Phase 3** — 鲁棒性收紧 + 生产化打包(PyInstaller / CI)。
- **OPT-SL**(结构光)= gated,不在本 plan 实施任务内(spec §16 已设计,留独立 plan)。

---

## 文件结构（创建 / 修改）

**Python sidecar**(`python-sidecar/src/lmt_vba_sidecar/`):
- `model_constrained_ba.py` *(新)* — SE3 参数化 + 相机/箱体联合 BA + gauge fix + robust loss + sparse Jacobian + 协方差。
- `simulate.py` *(新)* — 几何模拟器:真值箱体/相机位姿 → 带噪观测。
- `evaluate.py` *(新)* — gauge-invariant 指标(尺寸/距离/夹角)+ SE(3) 对齐 holdout RMS。
- `observability.py` *(新)* — per-cabinet 观测门 + 观测图连通性。
- `screen_mapping.py` *(新)* — screen_mapping 模型 + 像素↔mm + preflight 校验。
- `capture_manifest.py` *(新)* — capture_manifest 加载(ChArUco;结构光帧序列留接口)。
- `ipc.py` *(改)* — 新增 simulate/eval/screen_mapping/capture_manifest/cabinet_pose DTO。
- `detect.py` *(改)* — 加 `CharucoDetector` board 角点提取 → `(cabinet, local_mm, pixel)`。
- `reconstruct.py` *(改)* — 改用 model_constrained_ba + capture_manifest + screen_mapping + cabinet_pose_report 输出。
- `calibrate.py` *(改)* — RMS 门槛收紧 + 覆盖检查。
- `pattern.py` *(改)* — local 坐标以发光面中心为原点(与 reconstruct 对齐)。
- `__main__.py` *(改)* — 注册 `simulate` / `eval` 子命令。

**Rust**:
- `crates/lmt-shared/src/dto.rs` *(改)* — 新增 visual op 的对外 DTO。
- `crates/lmt-shared/src/envelope.rs` *(改)* — `error_codes` 加视觉错误码。
- `crates/lmt-shared/src/exit_codes.rs` *(改)* — 加对应退出码 + 映射。
- `crates/lmt-shared/src/schema.rs` *(改)* — `dump_all` 注册新 DTO。
- `crates/adapter-visual-ba/src/api.rs` + `ipc.rs` *(改)* — 新 api fn(calibrate/generate_pattern/reconstruct-by-manifest/simulate/eval)+ 事件/DTO。
- `crates/lmt-app/src/visual.rs` *(新)* + `lib.rs` *(改)* — `run_*` 服务层 helper。
- `crates/lmt-cli/src/cli.rs` *(改)* — 加 `Visual(VisualCmd)` 子命令枚举。
- `crates/lmt-cli/src/commands/visual.rs` *(新)* + `mod.rs` *(改)* + `main.rs` *(改 dispatch)*。
- `crates/lmt-cli/tests/cli_e2e.rs` *(改)* — visual op 的 happy/refuse/dry-run/error。

**Docs**:
- `docs/agents-cli.md` *(改)* — 命令表 + side_effect + 错误码表。
- `docs/contract-manifest.json` *(改)* — 快照刷新。
- `docs/poc/2026-XX-XX-monitor-bench-report.md` *(新)* — 两显示器台架报告模板。

**打包 / CI**:
- `python-sidecar/build_exe.ps1` + `build_exe.sh` *(新)*。
- `.github/workflows/visual-ci.yml` *(新)*。

---

## Phase 0 — Python 算法 + 合成几何台（pytest 验证，不碰 Rust）

**Phase 0 出口判据:** `pytest python-sidecar/tests/ -k "ba or simulate or eval or observability"` 全过;`test_eval_matrix` 证明 model-constrained 在固定 seed 矩阵上 RMS 显著低于 free-point baseline,且 nominal 档(0.3px / 20 视角 / 80% 可见)RMS ≤ 3mm、p95 ≤ 6mm、尺寸误差 ≤ 2mm、距离 ≤ 3mm、夹角 ≤ 0.3°(阈值起步,执行期按实测微调并写回 spec §10.3)。

### Task 0.1：新增 simulate / eval / pose DTO（ipc.py）

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py`
- Test: `python-sidecar/tests/test_ipc.py`

- [ ] **Step 1: 写失败测试**

```python
# tests/test_ipc.py（追加）
from lmt_vba_sidecar.ipc import (
    SimulateInput, SimulateScene, CameraSamplingSpec, NoiseSpec,
    EvalInput, CabinetPose, CabinetPoseReport,
)

def test_simulate_input_roundtrip():
    inp = SimulateInput.model_validate({
        "command": "simulate", "version": 1,
        "scene": {"cabinet_array": {"cols": 2, "rows": 1, "cabinet_size_mm": [600, 340]},
                  "shape_prior": "flat", "inter_board_angle_deg": 0.0},
        "cameras": {"n_views": 20, "distance_mm_range": [1500, 3000],
                    "yaw_deg_range": [-40, 40], "pitch_deg_range": [-20, 20]},
        "intrinsics": {"K": [[2000,0,960],[0,2000,540],[0,0,1]],
                       "dist_coeffs": [0,0,0,0,0], "image_size": [1920,1080]},
        "noise": {"pixel_sigma": 0.3, "outlier_frac": 0.0,
                  "visibility_frac": 0.8, "pixel_pitch_error_frac": 0.0},
        "seed": 42,
    })
    assert inp.cameras.n_views == 20
    assert inp.noise.pixel_sigma == 0.3

def test_cabinet_pose_report_serializes():
    rep = CabinetPoseReport(
        schema_version="visual_pose_report.v1",
        frame={"type": "screen_local", "gauge_strategy": "fix_root_cabinet",
               "root_cabinet": [0, 0], "units": "mm", "handedness": "right", "z_axis": "outward"},
        cabinet_poses=[CabinetPose(
            cabinet_id="V000_R000", position_mm=[0,0,0],
            rotation_matrix=[[1,0,0],[0,1,0],[0,0,1]], normal=[0,0,1],
            corners_mm=[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]],
            reprojection_rms_px=0.4, observed_views=7, observed_points=128, quality="ok")],
    )
    d = rep.model_dump()
    assert d["cabinet_poses"][0]["cabinet_id"] == "V000_R000"
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd python-sidecar && python -m pytest tests/test_ipc.py -k "simulate_input or cabinet_pose_report" -v`
Expected: FAIL（`ImportError: cannot import name 'SimulateInput'`）

- [ ] **Step 3: 实现 DTO（ipc.py 追加）**

```python
# 复用已有 Vec3 / Mat3 / CabinetArray / Intrinsics / ShapePrior

class CameraSamplingSpec(BaseModel):
    n_views: int = Field(ge=2)
    distance_mm_range: Annotated[list[float], Field(min_length=2, max_length=2)]
    yaw_deg_range: Annotated[list[float], Field(min_length=2, max_length=2)]
    pitch_deg_range: Annotated[list[float], Field(min_length=2, max_length=2)]

class NoiseSpec(BaseModel):
    pixel_sigma: float = Field(ge=0.0)
    outlier_frac: float = Field(ge=0.0, le=1.0, default=0.0)
    visibility_frac: float = Field(gt=0.0, le=1.0, default=1.0)
    pixel_pitch_error_frac: float = Field(ge=0.0, default=0.0)

class SimulateScene(BaseModel):
    cabinet_array: CabinetArray
    shape_prior: ShapePrior = "flat"
    inter_board_angle_deg: float = 0.0  # 两块/多块板间夹角(显示器台架用)

class SimulateInput(BaseModel):
    command: Literal["simulate"]
    version: Literal[1]
    scene: SimulateScene
    cameras: CameraSamplingSpec
    intrinsics: Intrinsics
    noise: NoiseSpec
    seed: int = 0
    out_dir: str | None = None  # CLI 模式写盘;None 则结果走 stdout

class EvalInput(BaseModel):
    command: Literal["eval"]
    version: Literal[1]
    dataset_dir: str
    method: Literal["free_point", "charuco", "structured_light"] = "charuco"
    seed_matrix: list[int] = Field(default_factory=lambda: [0])

class FrameSpec(BaseModel):
    type: Literal["screen_local"] = "screen_local"
    gauge_strategy: Literal["fix_root_cabinet", "align_to_nominal"] = "fix_root_cabinet"
    root_cabinet: Annotated[list[int], Field(min_length=2, max_length=2)] = [0, 0]
    units: Literal["mm"] = "mm"
    handedness: Literal["right"] = "right"
    z_axis: Literal["outward"] = "outward"

class CabinetPose(BaseModel):
    cabinet_id: str
    position_mm: Vec3
    rotation_matrix: Mat3
    normal: Vec3
    corners_mm: Annotated[list[Vec3], Field(min_length=4, max_length=4)]
    reprojection_rms_px: float
    observed_views: int
    observed_points: int
    quality: Literal["ok", "low_observation", "high_residual"]

class CabinetPoseReport(BaseModel):
    schema_version: Literal["visual_pose_report.v1"]
    frame: FrameSpec
    cabinet_poses: list[CabinetPose]
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd python-sidecar && python -m pytest tests/test_ipc.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/ipc.py python-sidecar/tests/test_ipc.py
git commit -m "feat(sidecar): add simulate/eval/cabinet-pose IPC models"
```

### Task 0.2：model-constrained BA 内核（零噪声精确恢复）

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/model_constrained_ba.py`
- Test: `python-sidecar/tests/test_model_constrained_ba.py`

**设计要点:** 状态 = 各相机 SE3 `(rvec,t)` + **非根**箱体 SE3;根箱体(`frame.root_cabinet`)固定为 `R=I, t=0`(gauge fix → world frame ≡ 根箱体发光面 frame)。观测 = `(cam_idx, cab_idx, p_local_mm, pixel)`。残差 = `project(K, R_cam·(R_cab·p_local + t_cab) + t_cam) − pixel`。robust loss = Huber。

- [ ] **Step 1: 写失败测试（零噪声必须精确恢复）**

```python
# tests/test_model_constrained_ba.py
import numpy as np
import cv2
from lmt_vba_sidecar.model_constrained_ba import model_constrained_ba, Observation

def _project(K, R_cam, t_cam, R_cab, t_cab, p_local):
    xw = R_cab @ p_local + t_cab
    xc = R_cam @ xw + t_cam
    p = K @ xc
    return p[:2] / p[2]

def test_zero_noise_recovers_two_boards_exactly():
    K = np.array([[2000.,0,960],[0,2000,540],[0,0,1]])
    # 真值:board0 在原点(根),board1 右移 700mm 且绕 y 转 15°
    R0, t0 = np.eye(3), np.zeros(3)
    R1, _ = cv2.Rodrigues(np.array([0., np.deg2rad(15), 0.]))
    t1 = np.array([700., 0., 0.])
    # 每板 4 个已知 local 角点(±300, ±170)
    corners = np.array([[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]], float)
    boards = [(R0, t0), (R1, t1)]
    # 5 个相机,各自看两块板
    cams = []
    for i in range(5):
        rvec = np.array([0.05*i, 0.1*i, 0.0])
        Rc, _ = cv2.Rodrigues(rvec)
        tc = np.array([50.*i, -20.*i, 2500.])
        cams.append((Rc, tc))
    obs = []
    for ci,(Rc,tc) in enumerate(cams):
        for bj,(Rb,tb) in enumerate(boards):
            for p in corners:
                px = _project(K, Rc, tc, Rb, tb, p)
                obs.append(Observation(camera_idx=ci, cabinet_idx=bj,
                                       p_local=p.copy(), pixel=px.copy()))
    # 初值:相机给真值附近的扰动,board1 给 nominal(700,0,0 无旋转)
    init_cams = [(Rc, tc) for Rc, tc in cams]
    init_boards = {1: (np.eye(3), np.array([700.,0,0]))}  # 根 board0 固定不在状态里
    result = model_constrained_ba(
        K=K, observations=obs, n_cameras=5, n_cabinets=2,
        root_cabinet_idx=0, init_cameras=init_cams, init_cabinets=init_boards,
        loss="linear",  # 零噪声用线性 loss 验精确性
    )
    assert result.converged
    # board1 平移恢复到亚毫米
    assert np.linalg.norm(result.cabinet_poses[1][1] - t1) < 0.05
    # board1 旋转恢复(法向夹角 < 0.05°)
    n_est = result.cabinet_poses[1][0] @ np.array([0,0,1.])
    n_true = R1 @ np.array([0,0,1.])
    ang = np.degrees(np.arccos(np.clip(n_est @ n_true, -1, 1)))
    assert ang < 0.05
    assert result.rms_reprojection_px < 1e-3
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd python-sidecar && python -m pytest tests/test_model_constrained_ba.py -v`
Expected: FAIL（模块不存在）

- [ ] **Step 3: 实现 model_constrained_ba.py**

```python
"""Model-constrained bundle adjustment.

State = per-camera SE3 (rvec,t) + per-NON-root cabinet SE3 (rvec,t).
Root cabinet (gauge) is fixed at R=I,t=0 so the world frame equals the
root cabinet's active-surface frame. Observations carry the known local
mm coordinate of each detected corner. Scale is fixed by these metric
local coords — no anchors, no total station.
"""
from __future__ import annotations
from dataclasses import dataclass
import cv2
import numpy as np
from scipy.optimize import least_squares
from scipy.sparse import lil_matrix


@dataclass
class Observation:
    camera_idx: int
    cabinet_idx: int
    p_local: np.ndarray  # (3,) mm
    pixel: np.ndarray     # (2,)


@dataclass
class BAResult:
    camera_poses: list[tuple[np.ndarray, np.ndarray]]
    cabinet_poses: dict[int, tuple[np.ndarray, np.ndarray]]  # idx -> (R,t); 含 root=I,0
    rms_reprojection_px: float
    iterations: int
    converged: bool
    cabinet_covariances: dict[int, np.ndarray]


def _nonroot_cabinets(n_cabinets: int, root: int) -> list[int]:
    return [j for j in range(n_cabinets) if j != root]


def _pack(cams, cabs, nonroot):
    parts = []
    for R, t in cams:
        rvec, _ = cv2.Rodrigues(R)
        parts.append(np.concatenate([rvec.ravel(), t]))
    for j in nonroot:
        R, t = cabs[j]
        rvec, _ = cv2.Rodrigues(R)
        parts.append(np.concatenate([rvec.ravel(), t]))
    return np.concatenate(parts)


def _unpack(x, n_cams, nonroot):
    cams = []
    for i in range(n_cams):
        seg = x[i*6:i*6+6]
        R, _ = cv2.Rodrigues(seg[:3])
        cams.append((R, seg[3:6]))
    cabs = {}
    base = n_cams*6
    for k, j in enumerate(nonroot):
        seg = x[base+k*6: base+k*6+6]
        R, _ = cv2.Rodrigues(seg[:3])
        cabs[j] = (R, seg[3:6])
    return cams, cabs


def _residuals(x, n_cams, nonroot, root, K, obs):
    cams, cabs = _unpack(x, n_cams, nonroot)
    res = np.zeros(len(obs)*2)
    for k, o in enumerate(obs):
        Rc, tc = cams[o.camera_idx]
        if o.cabinet_idx == root:
            Rb, tb = np.eye(3), np.zeros(3)
        else:
            Rb, tb = cabs[o.cabinet_idx]
        xw = Rb @ o.p_local + tb
        xc = Rc @ xw + tc
        p = K @ xc
        res[k*2:k*2+2] = p[:2]/p[2] - o.pixel
    return res


def _sparsity(n_cams, nonroot, root, obs):
    n = n_cams*6 + len(nonroot)*6
    A = lil_matrix((len(obs)*2, n), dtype=int)
    nonroot_pos = {j: k for k, j in enumerate(nonroot)}
    base = n_cams*6
    for k, o in enumerate(obs):
        A[k*2:k*2+2, o.camera_idx*6:o.camera_idx*6+6] = 1
        if o.cabinet_idx != root:
            c = base + nonroot_pos[o.cabinet_idx]*6
            A[k*2:k*2+2, c:c+6] = 1
    return A


def model_constrained_ba(*, K, observations, n_cameras, n_cabinets,
                         root_cabinet_idx, init_cameras, init_cabinets,
                         loss="huber", f_scale=2.0, max_nfev=200,
                         compute_covariance=True) -> BAResult:
    nonroot = _nonroot_cabinets(n_cabinets, root_cabinet_idx)
    cabs0 = dict(init_cabinets)
    for j in nonroot:
        cabs0.setdefault(j, (np.eye(3), np.zeros(3)))
    x0 = _pack(init_cameras, cabs0, nonroot)
    sp = _sparsity(n_cameras, nonroot, root_cabinet_idx, observations)
    sol = least_squares(
        _residuals, x0, jac_sparsity=sp, method="trf",
        loss=loss, f_scale=f_scale, max_nfev=max_nfev, verbose=0,
        args=(n_cameras, nonroot, root_cabinet_idx, K, observations),
    )
    cams, cabs = _unpack(sol.x, n_cameras, nonroot)
    cabs[root_cabinet_idx] = (np.eye(3), np.zeros(3))
    rms = float(np.sqrt((sol.fun**2).reshape(-1, 2).sum(axis=1).mean()))
    covs: dict[int, np.ndarray] = {}
    if compute_covariance and sol.jac is not None:
        try:
            J = sol.jac.toarray() if hasattr(sol.jac, "toarray") else np.asarray(sol.jac)
            dof = max(1, J.shape[0]-J.shape[1])
            cov = np.linalg.pinv(J.T @ J) * float((sol.fun**2).sum()/dof)
            base = n_cameras*6
            for k, j in enumerate(nonroot):
                a = base + k*6 + 3  # translation block
                covs[j] = cov[a:a+3, a:a+3]
        except np.linalg.LinAlgError:
            pass
    return BAResult(camera_poses=cams, cabinet_poses=cabs,
                    rms_reprojection_px=rms, iterations=int(sol.nfev),
                    converged=bool(sol.success), cabinet_covariances=covs)
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd python-sidecar && python -m pytest tests/test_model_constrained_ba.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/model_constrained_ba.py python-sidecar/tests/test_model_constrained_ba.py
git commit -m "feat(sidecar): model-constrained BA core (camera+cabinet SE3, gauge-fixed root)"
```

### Task 0.3：几何模拟器（simulate.py）

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/simulate.py`
- Test: `python-sidecar/tests/test_simulate.py`

- [ ] **Step 1: 写失败测试**

```python
# tests/test_simulate.py
import numpy as np
from lmt_vba_sidecar.ipc import SimulateInput
from lmt_vba_sidecar.simulate import build_scene

def _inp(seed=42, n=12, vis=1.0, pitch=0.0):
    return SimulateInput.model_validate({
        "command":"simulate","version":1,
        "scene":{"cabinet_array":{"cols":2,"rows":1,"cabinet_size_mm":[600,340]},
                 "shape_prior":"flat","inter_board_angle_deg":10.0},
        "cameras":{"n_views":n,"distance_mm_range":[1500,3000],
                   "yaw_deg_range":[-40,40],"pitch_deg_range":[-20,20]},
        "intrinsics":{"K":[[2000,0,960],[0,2000,540],[0,0,1]],
                      "dist_coeffs":[0,0,0,0,0],"image_size":[1920,1080]},
        "noise":{"pixel_sigma":0.0,"visibility_frac":vis,"pixel_pitch_error_frac":pitch},
        "seed":seed})

def test_scene_is_deterministic_per_seed():
    a = build_scene(_inp(seed=7))
    b = build_scene(_inp(seed=7))
    assert np.allclose(a.true_cabinet_poses[1][1], b.true_cabinet_poses[1][1])
    assert len(a.observations) == len(b.observations)

def test_zero_noise_observations_reproject_exactly():
    scene = build_scene(_inp(seed=1))
    # 用真值位姿重投影,残差应为 0
    K = scene.K
    for o in scene.observations[:50]:
        Rc, tc = scene.true_camera_poses[o.camera_idx]
        Rb, tb = scene.true_cabinet_poses[o.cabinet_idx]
        xw = Rb @ o.p_local + tb; xc = Rc @ xw + tc; p = K @ xc
        assert np.linalg.norm(p[:2]/p[2] - o.pixel) < 1e-6

def test_inter_board_angle_is_applied():
    scene = build_scene(_inp())
    n0 = scene.true_cabinet_poses[0][0] @ np.array([0,0,1.])
    n1 = scene.true_cabinet_poses[1][0] @ np.array([0,0,1.])
    ang = np.degrees(np.arccos(np.clip(n0 @ n1, -1, 1)))
    assert abs(ang - 10.0) < 1e-6
```

- [ ] **Step 2: 跑确认失败**

Run: `cd python-sidecar && python -m pytest tests/test_simulate.py -v`  → FAIL

- [ ] **Step 3: 实现 simulate.py**

```python
"""Geometric simulator (Level 0A). Builds true cabinet/camera poses and
noisy (cam, cabinet, local_mm, pixel) observations. Validates BA math only
— NOT a substitute for real capture (no LED bloom/moire/rolling shutter)."""
from __future__ import annotations
from dataclasses import dataclass
import cv2
import numpy as np
from lmt_vba_sidecar.ipc import SimulateInput
from lmt_vba_sidecar.model_constrained_ba import Observation


@dataclass
class Scene:
    K: np.ndarray
    true_camera_poses: list[tuple[np.ndarray, np.ndarray]]
    true_cabinet_poses: dict[int, tuple[np.ndarray, np.ndarray]]
    cabinet_corners_local: dict[int, np.ndarray]  # idx -> (M,3) mm
    observations: list[Observation]
    n_cameras: int
    n_cabinets: int


def _board_corners_local(w_mm: float, h_mm: float, nx=8, ny=8) -> np.ndarray:
    # 发光面中心为原点;nx×ny 内角点格点(模拟 ChArUco 内角点)
    xs = (np.arange(nx)-(nx-1)/2)/(nx-1)*w_mm
    ys = (np.arange(ny)-(ny-1)/2)/(ny-1)*h_mm
    gx, gy = np.meshgrid(xs, ys)
    return np.stack([gx.ravel(), gy.ravel(), np.zeros(gx.size)], axis=1)


def build_scene(inp: SimulateInput) -> Scene:
    rng = np.random.default_rng(inp.seed)
    K = np.array(inp.intrinsics.K, float)
    cab = inp.scene.cabinet_array
    cw, ch = cab.cabinet_size_mm
    n_cab = cab.cols * cab.rows
    pitch_err = inp.noise.pixel_pitch_error_frac

    # 真值箱体位姿:沿 x 平铺,相邻板按 inter_board_angle_deg 累积夹角
    cabinet_poses: dict[int, tuple[np.ndarray, np.ndarray]] = {}
    corners_local: dict[int, np.ndarray] = {}
    x_cursor = 0.0
    ang = np.deg2rad(inp.scene.inter_board_angle_deg)
    for j in range(n_cab):
        R, _ = cv2.Rodrigues(np.array([0., ang*j, 0.]))
        t = np.array([x_cursor, 0., 0.])
        cabinet_poses[j] = (R, t)
        # local 角点;pitch 误差缩放(模拟标称间距与真实不符)
        scale = 1.0 + pitch_err
        corners_local[j] = _board_corners_local(cw*scale, ch*scale)
        x_cursor += cw  # 简化:中心间距 = cabinet 宽

    # 真值相机位姿:绕屏阵列中心采样
    center = np.array([x_cursor/2 - cw/2, 0., 0.])
    cams = []
    for _ in range(inp.cameras.n_views):
        dist = rng.uniform(*inp.cameras.distance_mm_range)
        yaw = np.deg2rad(rng.uniform(*inp.cameras.yaw_deg_range))
        pitch = np.deg2rad(rng.uniform(*inp.cameras.pitch_deg_range))
        cam_pos = center + dist*np.array([np.sin(yaw)*np.cos(pitch),
                                          np.sin(pitch), -np.cos(yaw)*np.cos(pitch)])
        fwd = (center - cam_pos); fwd /= np.linalg.norm(fwd)
        up = np.array([0.,1.,0.])
        right = np.cross(up, fwd); right /= np.linalg.norm(right)
        up2 = np.cross(fwd, right)
        R = np.stack([right, up2, fwd])  # world->camera
        t = -R @ cam_pos
        cams.append((R, t))

    obs: list[Observation] = []
    for ci, (Rc, tc) in enumerate(cams):
        for j in range(n_cab):
            Rb, tb = cabinet_poses[j]
            for p in corners_local[j]:
                if rng.random() > inp.noise.visibility_frac:
                    continue
                xw = Rb @ p + tb; xc = Rc @ xw + tc
                if xc[2] <= 0:
                    continue
                px = (K @ xc)[:2] / (K @ xc)[2]
                if inp.noise.pixel_sigma > 0:
                    px = px + rng.normal(0, inp.noise.pixel_sigma, 2)
                if inp.noise.outlier_frac > 0 and rng.random() < inp.noise.outlier_frac:
                    px = px + rng.normal(0, 50, 2)
                obs.append(Observation(camera_idx=ci, cabinet_idx=j,
                                       p_local=p.copy(), pixel=px))
    return Scene(K=K, true_camera_poses=cams, true_cabinet_poses=cabinet_poses,
                 cabinet_corners_local=corners_local, observations=obs,
                 n_cameras=len(cams), n_cabinets=n_cab)
```

- [ ] **Step 4: 跑确认通过**

Run: `cd python-sidecar && python -m pytest tests/test_simulate.py -v`  → PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/simulate.py python-sidecar/tests/test_simulate.py
git commit -m "feat(sidecar): geometric simulator for synthetic bench (Level 0A)"
```

### Task 0.4：评估器（gauge-invariant 指标 + SE(3) holdout）

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/evaluate.py`
- Test: `python-sidecar/tests/test_evaluate.py`

**指标(spec §10.2):** ① 每箱体尺寸误差;② 两两中心距离误差;③ 两两法向夹角误差;④ SE(3) 对齐后逐角点 RMS/p95(对齐用一组点,打分用互斥 holdout)。

- [ ] **Step 1: 写失败测试**

```python
# tests/test_evaluate.py
import numpy as np
from lmt_vba_sidecar.evaluate import (
    gauge_invariant_metrics, se3_aligned_holdout_rms, umeyama_no_scale,
)

def test_umeyama_recovers_known_rigid():
    rng = np.random.default_rng(0)
    src = rng.normal(size=(20,3))*100
    R, _ = np.linalg.qr(rng.normal(size=(3,3)))
    if np.linalg.det(R) < 0: R[:, 0] *= -1
    t = np.array([10., -5., 3.])
    dst = (src @ R.T) + t
    R_est, t_est = umeyama_no_scale(src, dst)
    assert np.allclose(R_est, R, atol=1e-8)
    assert np.allclose(t_est, t, atol=1e-8)

def test_gauge_invariant_metrics_zero_when_perfect():
    # 两块板:真值与重建一致 → 所有误差应 ~0
    true_centers = {0: np.zeros(3), 1: np.array([700.,0,0])}
    true_normals = {0: np.array([0,0,1.]), 1: np.array([0,0,1.])}
    true_sizes = {0: (600.,340.), 1: (600.,340.)}
    m = gauge_invariant_metrics(true_centers, true_normals, true_sizes,
                                true_centers, true_normals, true_sizes)
    assert m["max_distance_error_mm"] < 1e-9
    assert m["max_angle_error_deg"] < 1e-9
    assert m["max_size_error_mm"] < 1e-9
```

- [ ] **Step 2: 跑确认失败** → `python -m pytest tests/test_evaluate.py -v` FAIL

- [ ] **Step 3: 实现 evaluate.py**

```python
"""Gauge-invariant evaluation. All headline metrics (sizes, pairwise
distances, pairwise normal angles) are SE(3)-invariant, so no datum /
total station is needed. Full-field RMS uses SE(3) alignment with
disjoint align/score split to avoid self-scoring."""
from __future__ import annotations
import itertools
import numpy as np


def umeyama_no_scale(src: np.ndarray, dst: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    sc, dc = src.mean(0), dst.mean(0)
    H = (src - sc).T @ (dst - dc)
    U, _, Vt = np.linalg.svd(H)
    d = np.sign(np.linalg.det(Vt.T @ U.T))
    R = Vt.T @ np.diag([1, 1, d]) @ U.T
    t = dc - R @ sc
    return R, t


def gauge_invariant_metrics(tc, tn, ts, ec, en, es) -> dict:
    """t*=true, e*=estimated; c=centers{idx:vec3}, n=normals, s=sizes{idx:(w,h)}."""
    size_err = [abs(es[i][0]-ts[i][0]) for i in tc] + [abs(es[i][1]-ts[i][1]) for i in tc]
    ang_err, dist_err = [], []
    for i, j in itertools.combinations(sorted(tc), 2):
        td = np.linalg.norm(tc[i]-tc[j]); ed = np.linalg.norm(ec[i]-ec[j])
        dist_err.append(abs(ed-td))
        ta = np.degrees(np.arccos(np.clip(tn[i]@tn[j], -1, 1)))
        ea = np.degrees(np.arccos(np.clip(en[i]@en[j], -1, 1)))
        ang_err.append(abs(ea-ta))
    return {
        "max_size_error_mm": float(max(size_err)) if size_err else 0.0,
        "rms_size_error_mm": float(np.sqrt(np.mean(np.square(size_err)))) if size_err else 0.0,
        "max_distance_error_mm": float(max(dist_err)) if dist_err else 0.0,
        "max_angle_error_deg": float(max(ang_err)) if ang_err else 0.0,
    }


def se3_aligned_holdout_rms(true_pts: np.ndarray, est_pts: np.ndarray,
                            align_idx, score_idx) -> dict:
    R, t = umeyama_no_scale(est_pts[align_idx], true_pts[align_idx])
    aligned = (est_pts[score_idx] @ R.T) + t
    err = np.linalg.norm(aligned - true_pts[score_idx], axis=1)
    return {"rms_mm": float(np.sqrt(np.mean(err**2))),
            "p95_mm": float(np.percentile(err, 95)),
            "max_mm": float(err.max())}
```

- [ ] **Step 4: 跑确认通过** → PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/evaluate.py python-sidecar/tests/test_evaluate.py
git commit -m "feat(sidecar): gauge-invariant evaluator (sizes/distances/angles + SE3 holdout)"
```

### Task 0.5：可观测性门 + 观测图连通性（observability.py）

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/observability.py`
- Test: `python-sidecar/tests/test_observability.py`

- [ ] **Step 1: 写失败测试**

```python
# tests/test_observability.py
import pytest
from lmt_vba_sidecar.model_constrained_ba import Observation
import numpy as np
from lmt_vba_sidecar.observability import check_observability, ObservabilityError

def _obs(ci, bj):
    return Observation(camera_idx=ci, cabinet_idx=bj, p_local=np.zeros(3), pixel=np.zeros(2))

def test_connected_graph_passes():
    obs = [_obs(0,0),_obs(0,1),_obs(1,1),_obs(1,2)]  # 0-0,0-1,1-1,1-2 连通
    rep = check_observability(obs, n_cabinets=3, min_views=1, min_points=1)
    assert rep["connected"]

def test_disconnected_cabinet_raises():
    obs = [_obs(0,0),_obs(0,1),  _obs(2,2)]  # cabinet 2 只被 cam2 看到,孤立
    with pytest.raises(ObservabilityError):
        check_observability(obs, n_cabinets=3, min_views=2, min_points=1)
```

- [ ] **Step 2: 跑确认失败** → FAIL

- [ ] **Step 3: 实现 observability.py**

```python
"""Per-cabinet observability gates + bipartite (camera↔cabinet) graph
connectivity. Disconnected cabinets form locally-independent solutions
that can look converged but are silently wrong (spec §12)."""
from __future__ import annotations
import collections


class ObservabilityError(Exception):
    pass


def check_observability(observations, n_cabinets, min_views=2, min_points=8) -> dict:
    views_per_cab = collections.defaultdict(set)
    points_per_cab = collections.defaultdict(int)
    adj = collections.defaultdict(set)  # cabinet -> set(cameras)
    for o in observations:
        views_per_cab[o.cabinet_idx].add(o.camera_idx)
        points_per_cab[o.cabinet_idx] += 1
        adj[o.cabinet_idx].add(o.camera_idx)

    weak = []
    for j in range(n_cabinets):
        nv = len(views_per_cab.get(j, ()))
        npts = points_per_cab.get(j, 0)
        if nv < min_views or npts < min_points:
            weak.append({"cabinet_idx": j, "views": nv, "points": npts})

    # 连通性:把 cabinet 与 camera 当二部图节点,BFS。
    cam_nodes = {("cam", c) for s in adj.values() for c in s}
    cab_nodes = {("cab", j) for j in adj}
    if not cab_nodes:
        raise ObservabilityError("no cabinet observed")
    g = collections.defaultdict(set)
    for j, cams in adj.items():
        for c in cams:
            g[("cab", j)].add(("cam", c)); g[("cam", c)].add(("cab", j))
    start = next(iter(cab_nodes)); seen = {start}; stack = [start]
    while stack:
        for nb in g[stack.pop()]:
            if nb not in seen:
                seen.add(nb); stack.append(nb)
    connected = (cab_nodes | cam_nodes) <= seen
    isolated = sorted(j for ("cab", j) in cab_nodes if ("cab", j) not in seen)

    if weak or not connected:
        raise ObservabilityError(
            f"observability failed: weak={weak}, connected={connected}, isolated_cabinets={isolated}")
    return {"connected": True, "weak": weak}
```

- [ ] **Step 4: 跑确认通过** → PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/observability.py python-sidecar/tests/test_observability.py
git commit -m "feat(sidecar): observability gates + camera-cabinet graph connectivity"
```

### Task 0.6：free-point baseline 适配 + 端到端 eval 矩阵（证明新算法更强）

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/eval_runner.py`
- Test: `python-sidecar/tests/test_eval_matrix.py`

**目的:** 把"旧自由点 BA"(现有 `ba.py`)和"新 model-constrained BA"包成统一 `run_method(scene, method)`,在固定 seed 矩阵上对比,固化 Phase 0 出口判据。

- [ ] **Step 1: 写失败测试（出口判据）**

```python
# tests/test_eval_matrix.py
import numpy as np
from lmt_vba_sidecar.ipc import SimulateInput
from lmt_vba_sidecar.simulate import build_scene
from lmt_vba_sidecar.eval_runner import run_method, reconstruct_cabinet_geometry

def _inp(seed, pixel_sigma=0.3, n=20, vis=0.8):
    return SimulateInput.model_validate({
        "command":"simulate","version":1,
        "scene":{"cabinet_array":{"cols":2,"rows":1,"cabinet_size_mm":[600,340]},
                 "shape_prior":"flat","inter_board_angle_deg":10.0},
        "cameras":{"n_views":n,"distance_mm_range":[1500,3000],
                   "yaw_deg_range":[-40,40],"pitch_deg_range":[-20,20]},
        "intrinsics":{"K":[[2000,0,960],[0,2000,540],[0,0,1]],
                      "dist_coeffs":[0,0,0,0,0],"image_size":[1920,1080]},
        "noise":{"pixel_sigma":pixel_sigma,"visibility_frac":vis},
        "seed":seed})

def test_model_constrained_beats_free_point_on_seed_matrix():
    seeds = [0,1,2,3,4]
    mc, fp = [], []
    for s in seeds:
        scene = build_scene(_inp(s))
        mc.append(run_method(scene, "charuco")["max_distance_error_mm"])
        fp.append(run_method(scene, "free_point")["max_distance_error_mm"])
    assert np.median(mc) < np.median(fp)          # 新算法更准
    assert np.median(mc) < 3.0                      # nominal 档距离误差 < 3mm(起步阈值)

def test_nominal_tier_thresholds():
    errs = [run_method(build_scene(_inp(s)), "charuco") for s in range(5)]
    assert np.median([e["max_size_error_mm"] for e in errs]) < 2.0
    assert np.median([e["max_angle_error_deg"] for e in errs]) < 0.3
```

- [ ] **Step 2: 跑确认失败** → FAIL

- [ ] **Step 3: 实现 eval_runner.py**

```python
"""Run a method end-to-end on a synthetic Scene and return gauge-invariant
metrics. 'charuco' = model-constrained BA; 'free_point' = legacy ba.py for
baseline comparison."""
from __future__ import annotations
import numpy as np
from lmt_vba_sidecar.model_constrained_ba import model_constrained_ba
from lmt_vba_sidecar.evaluate import gauge_invariant_metrics
from lmt_vba_sidecar.observability import check_observability
from lmt_vba_sidecar import ba as legacy_ba


def reconstruct_cabinet_geometry(R, t, corners_local):
    """板的中心/法向/尺寸(从位姿 + local 角点重建)。"""
    world = (corners_local @ R.T) + t
    center = world.mean(0)
    normal = R @ np.array([0., 0., 1.])
    # 尺寸:local x/y 跨度(发光面尺寸)
    w = corners_local[:, 0].ptp(); h = corners_local[:, 1].ptp()
    return center, normal, (float(w), float(h)), world


def run_method(scene, method: str) -> dict:
    check_observability(scene.observations, scene.n_cabinets, min_views=2, min_points=8)
    if method == "charuco":
        init_cams = scene.true_camera_poses  # Phase 0:用真值附近初值(隔离 BA 数学)
        init_cabs = {j: (np.eye(3), scene.true_cabinet_poses[j][1].copy())
                     for j in range(scene.n_cabinets)}
        res = model_constrained_ba(
            K=scene.K, observations=scene.observations,
            n_cameras=scene.n_cameras, n_cabinets=scene.n_cabinets,
            root_cabinet_idx=0, init_cameras=init_cams, init_cabinets=init_cabs)
        est_c, est_n, est_s = {}, {}, {}
        for j in range(scene.n_cabinets):
            R, t = res.cabinet_poses[j]
            c, n, s, _ = reconstruct_cabinet_geometry(R, t, scene.cabinet_corners_local[j])
            est_c[j], est_n[j], est_s[j] = c, n, s
    elif method == "free_point":
        est_c, est_n, est_s = _free_point_geometry(scene)
    else:
        raise ValueError(f"unknown method {method}")

    true_c, true_n, true_s = {}, {}, {}
    for j in range(scene.n_cabinets):
        R, t = scene.true_cabinet_poses[j]
        c, n, s, _ = reconstruct_cabinet_geometry(R, t, scene.cabinet_corners_local[j])
        true_c[j], true_n[j], true_s[j] = c, n, s
    return gauge_invariant_metrics(true_c, true_n, true_s, est_c, est_n, est_s)


def _free_point_geometry(scene):
    """旧自由点 BA baseline:每个 (cabinet, local-corner) 当独立自由 3D 点,
    箱体中心=质心,法向=PCA 最小奇异向量,尺寸=主轴跨度。不追求精确,
    只为给出"明显更差"的对照数字。"""
    pt_index: dict = {}
    init_pts: list = []
    for j in range(scene.n_cabinets):
        Rb, tb = scene.true_cabinet_poses[j]  # 仅用于 nominal 初值
        for p in scene.cabinet_corners_local[j]:
            key = (j, tuple(np.round(p, 6)))
            if key not in pt_index:
                pt_index[key] = len(init_pts)
                init_pts.append(Rb @ p + tb)
    init_points = np.array(init_pts, float)
    obs = [(o.camera_idx, pt_index[(o.cabinet_idx, tuple(np.round(o.p_local, 6)))], o.pixel)
           for o in scene.observations]
    res = legacy_ba.bundle_adjust(
        K=scene.K, dist_coeffs=np.zeros(5), initial_points=init_points,
        initial_cam_poses=list(scene.true_camera_poses), observations=obs,
        compute_covariance=False)
    est_c, est_n, est_s = {}, {}, {}
    for j in range(scene.n_cabinets):
        idxs = [pt_index[(j, tuple(np.round(p, 6)))] for p in scene.cabinet_corners_local[j]]
        pts = res.points[idxs]
        c = pts.mean(0)
        _, _, vt = np.linalg.svd(pts - c)
        normal = vt[2]                      # 最小奇异向量 = 平面法向
        proj = (pts - c) @ vt[:2].T          # 投影到前两主轴
        est_c[j], est_n[j] = c, normal
        est_s[j] = (float(proj[:, 0].ptp()), float(proj[:, 1].ptp()))
    return est_c, est_n, est_s
```

- [ ] **Step 4: 跑确认通过** → `python -m pytest tests/test_eval_matrix.py -v` PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/eval_runner.py python-sidecar/tests/test_eval_matrix.py
git commit -m "feat(sidecar): eval runner + seed-matrix gate (model-constrained beats free-point)"
```

### Task 0.7：sidecar 注册 simulate / eval 子命令

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/__main__.py`
- Create: `python-sidecar/src/lmt_vba_sidecar/simulate_cmd.py`（写盘 + NDJSON result）、`eval_cmd.py`
- Test: `python-sidecar/tests/test_main_dispatch.py`（追加）

- [ ] **Step 1: 写失败测试**

```python
# tests/test_main_dispatch.py（追加）
import json, subprocess, sys, pathlib

def test_simulate_subcommand_writes_dataset(tmp_path):
    payload = {"command":"simulate","version":1,
        "scene":{"cabinet_array":{"cols":2,"rows":1,"cabinet_size_mm":[600,340]},
                 "shape_prior":"flat","inter_board_angle_deg":0.0},
        "cameras":{"n_views":8,"distance_mm_range":[1500,2500],
                   "yaw_deg_range":[-30,30],"pitch_deg_range":[-15,15]},
        "intrinsics":{"K":[[2000,0,960],[0,2000,540],[0,0,1]],
                      "dist_coeffs":[0,0,0,0,0],"image_size":[1920,1080]},
        "noise":{"pixel_sigma":0.3},"seed":1,"out_dir":str(tmp_path/"ds")}
    p = subprocess.run([sys.executable,"-m","lmt_vba_sidecar","simulate"],
                       input=json.dumps(payload), capture_output=True, text=True)
    assert p.returncode == 0
    assert (tmp_path/"ds"/"scene.npz").exists()
    last = json.loads(p.stdout.strip().splitlines()[-1])
    assert last["event"] == "result"
```

- [ ] **Step 2: 跑确认失败** → FAIL

- [ ] **Step 3: 实现** — `simulate_cmd.py::run_simulate(SimulateInput)`(调 `build_scene`,存 `scene.npz` + `meta.json` 到 `out_dir`,写 `ResultEvent`);`eval_cmd.py::run_eval(EvalInput)`(载 dataset,跑 `run_method` over seed_matrix,写含 metrics 的 `ResultEvent`);`__main__.py` 的 `SUBCOMMAND_MODULES`/`SUBCOMMAND_ENTRYPOINTS` 加两项,argparse 加 `sub.add_parser("simulate")` / `"eval"`,import `SimulateInput`/`EvalInput`。

> **执行说明:** `ResultData` 当前要求 `measured_points`/`ba_stats`/`frame_strategy_used`。simulate/eval 不产 MeasuredPoints,需在 `ipc.py` 给 `ResultData` 的这些字段加默认值(`measured_points=[]` 等)或新增 `SimulateResultData`/`EvalResultData` 事件类型——**执行期二选一**:倾向新增独立事件类型,避免污染 reconstruct 的 result 契约。

- [ ] **Step 4: 跑确认通过** → PASS

- [ ] **Step 5: Commit**

```bash
git add python-sidecar/src/lmt_vba_sidecar/__main__.py python-sidecar/src/lmt_vba_sidecar/simulate_cmd.py python-sidecar/src/lmt_vba_sidecar/eval_cmd.py python-sidecar/tests/test_main_dispatch.py python-sidecar/src/lmt_vba_sidecar/ipc.py
git commit -m "feat(sidecar): register simulate/eval subcommands"
```

**Phase 0 验收:** `cd python-sidecar && python -m pytest tests/ -v` 全过;`test_eval_matrix` 绿即 Phase 0 出口达成。把 nominal/stress 实测阈值写回 spec §10.3。

---

## Phase 1 — ChArUco 前端 + sidecar reconstruct 重构 + 全套 Rust CLI surface

**Phase 1 出口判据:** `cargo test --workspace` + `pytest` 全过;`lmt visual reconstruct --capture-manifest <合成 manifest> --output json` 在合成图像上跑出 `cabinet_pose_report.json` + measured.yaml;`lmt --json schema | jq '.types'` 含新 DTO;`lmt manifest` 含 5 个 `visual.*` operation;`lmt visual <op> --help` 可读。

### Task 1.1：detect.py 提 ChArUco board 角点

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/detect.py`
- Test: `python-sidecar/tests/test_detect.py`(追加)

**改动:** 现有只 `detectMarkers`(返回 marker 4 角中心)。新增 `detect_charuco_corners(image_paths, board_lookup)`:用 `cv2.aruco.CharucoDetector` 提每块 board 的内角点,返回 `{path: [{"cabinet": (col,row), "charuco_id": int, "corner_px": [x,y]}]}`。marker 检测保留,仅用于 ID→cabinet 路由。

- [ ] **Step 1: 写失败测试**

```python
# tests/test_detect.py（追加）
import numpy as np, cv2, pathlib
from lmt_vba_sidecar.detect import detect_charuco_corners
from lmt_vba_sidecar.pattern import generate_cabinet_png

def test_charuco_corners_detected_on_rendered_board(tmp_path):
    p = tmp_path / "V000_R000.png"
    generate_cabinet_png(out_path=p, cabinet_pixel_size=(900, 510), aruco_id_start=0)
    # board_lookup: aruco_id_start→(cabinet, charuco geometry) 由 pattern_meta 提供;
    # 这里直接喂单 board。检测应返回 > 一半内角点(8x8 内角点 = 64)。
    out = detect_charuco_corners([str(p)], board_lookup_for_test=True)
    corners = out[str(p)]
    assert len(corners) >= 32
    assert all("charuco_id" in c and "corner_px" in c for c in corners)
```

- [ ] **Step 2: 跑确认失败** → FAIL

- [ ] **Step 3: 实现** — 在 `detect.py` 加 `detect_charuco_corners`:对每图先 `detectMarkers`,再 `CharucoDetector(board).detectBoard(img)` 拿 `charucoCorners + charucoIds`;`board` 由每 cabinet 的 sub-dictionary + `CharucoBoard` 重建(参数同 `pattern.py`:`size=(9,9)`, `squareLength`/`markerLength` 比例一致)。`charuco_id` → board 内 `(row,col)` → local mm(发光面中心为原点,由 `screen_mapping` 的 `active_size_mm` + 内角点格点算)。`board_lookup_for_test` 仅测试用单 board 旁路。

> **执行说明:** local mm 坐标的来源是 `screen_mapping.active_size_mm` 与内角点网格(`checkerboard_inner_corners`),不是 `squareLength=1.0`(那是渲染用的无量纲值)。Task 1.2 的 `screen_mapping.charuco_corner_local_mm(cabinet, charuco_id)` 提供这个映射;`detect_charuco_corners` 只返回像素 + charuco_id,local mm 在 reconstruct 组装观测时查。

- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(sidecar): ChArUco board corner detection (CharucoDetector)"`

### Task 1.2：screen_mapping.py（模型 + 像素↔mm + preflight）

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/screen_mapping.py`
- Test: `python-sidecar/tests/test_screen_mapping.py`

- [ ] **Step 1: 写失败测试**

```python
# tests/test_screen_mapping.py
import pytest, numpy as np
from lmt_vba_sidecar.screen_mapping import ScreenMapping, ScreenMappingError

def _mapping():
    return ScreenMapping.model_validate({
        "screen_id":"S","cabinets":[{
            "cabinet_id":"V000_R000","resolution_px":[900,510],
            "active_size_mm":[600,340],"pixel_pitch_mm":[0.667,0.667],
            "active_origin":"center","input_rect_px":[0,0,900,510],
            "rotation":0,"mirror_x":False,"mirror_y":False}],
        "expected_pattern_hash":"abc123"})

def test_charuco_corner_local_mm_centered():
    m = _mapping()
    # 8x8 内角点,charuco_id=0 应在左上角附近(负 x 负 y),中心对称
    p0 = m.charuco_corner_local_mm("V000_R000", 0, inner=8)
    p_last = m.charuco_corner_local_mm("V000_R000", 63, inner=8)
    assert np.allclose(p0[:2], -p_last[:2], atol=1e-6)  # 关于中心对称
    assert p0[2] == 0.0

def test_preflight_rejects_pattern_hash_mismatch():
    m = _mapping()
    with pytest.raises(ScreenMappingError):
        m.preflight(actual_pattern_hash="WRONG")
```

- [ ] **Step 2: 跑确认失败** → FAIL
- [ ] **Step 3: 实现** — `ScreenMapping` pydantic 模型(字段见 spec §6);`charuco_corner_local_mm(cabinet_id, charuco_id, inner)` 把 charuco_id 解成 `(r,c)` 网格 → 以发光面中心为原点的 mm;`preflight(actual_pattern_hash, image_size=None)` 校验 `expected_pattern_hash` 一致、`active_origin=="center"`、`rotation in {0,90,180,270}`,不符抛 `ScreenMappingError`。
- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(sidecar): screen_mapping model + pixel↔mm + preflight"`

### Task 1.3：capture_manifest.py（加载器）

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/capture_manifest.py`
- Test: `python-sidecar/tests/test_capture_manifest.py`

- [ ] **Step 1: 写失败测试**

```python
# tests/test_capture_manifest.py
from lmt_vba_sidecar.capture_manifest import load_capture_manifest

def test_charuco_manifest_lists_views(tmp_path):
    (tmp_path/"a.png").write_bytes(b"x"); (tmp_path/"b.png").write_bytes(b"x")
    mf = tmp_path/"capture.json"
    mf.write_text('{"method":"charuco","intrinsics":"i.json","pattern_meta":"pm.json",'
                  '"screen_mapping":"sm.json","views":[{"view_id":"c1","images":["a.png"]},'
                  '{"view_id":"c2","images":["b.png"]}]}')
    m = load_capture_manifest(str(mf))
    assert m.method == "charuco"
    assert [v.view_id for v in m.views] == ["c1","c2"]
```

- [ ] **Step 2: 跑确认失败** → FAIL
- [ ] **Step 3: 实现** — `CaptureManifest` pydantic(`method`、`intrinsics`、`pattern_meta`、`screen_mapping`、`views: [{view_id, images}]`;结构光的 `frames` 字段 `Optional`,本期不解析);`load_capture_manifest(path)` 读 JSON、相对路径按 manifest 所在目录解析、校验 view 非空。`--images <dir>` 的 convenience 转换(扫描目录每图当一个 view)也放这里:`manifest_from_images_dir(dir, method, intrinsics, pattern_meta, screen_mapping)`。
- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(sidecar): capture_manifest loader (charuco; SL frames reserved)"`

### Task 1.4：reconstruct.py 重构为 model-constrained

**Files:**
- Modify: `python-sidecar/src/lmt_vba_sidecar/reconstruct.py`
- Modify: `python-sidecar/src/lmt_vba_sidecar/ipc.py`(`ReconstructInput` 改为吃 capture_manifest + screen_mapping;移除 `frame_strategy`/`frame_anchors`)
- Test: `python-sidecar/tests/test_reconstruct.py`(重写)

**改动要点(spec §6/§7/§9):**
1. 输入改:`ReconstructInput { command, version, project(screen_id+cabinet_array+shape_prior), capture_manifest_path, screen_mapping_path }`。移除 `frame_strategy`/`frame_anchors`/`images`/`intrinsics`/`pattern_meta`(后三者从 manifest 引)。
2. 流程:载 manifest + screen_mapping → preflight → `detect_charuco_corners` → 组装 `Observation(cam, cab, p_local_mm(查 screen_mapping), pixel(undistort))` → `check_observability` → `model_constrained_ba`(root=cabinet(0,0))→ 每 cabinet 算 center/normal/corners → 写 `cabinet_pose_report.json` → MeasuredPoint(position=center,m;source=VisualBA)→ `ResultEvent`。
3. 删除 `nominal.py` 的质心平均路径、`procrustes` 锚定路径(A/C 模式整段移除)。

- [ ] **Step 1: 写失败测试(用 simulate 渲染的合成 manifest 端到端)**

```python
# tests/test_reconstruct.py（重写核心用例）
import json, numpy as np
from lmt_vba_sidecar.reconstruct import run_reconstruct
from lmt_vba_sidecar.ipc import ReconstructInput
# fixture: conftest 提供 synthetic_charuco_capture(tmp_path) → 渲染 2 板多视角 PNG +
# 写 capture.json / screen_mapping.json / pattern_meta.json / intrinsics.json,
# 已知真值 inter-board 距离 700mm、夹角 10°。

def test_reconstruct_writes_pose_report_and_matches_known_geometry(synthetic_charuco_capture, capsys):
    paths = synthetic_charuco_capture  # dict of file paths + known truth
    inp = ReconstructInput.model_validate({
        "command":"reconstruct","version":1,
        "project":{"screen_id":"S",
                   "cabinet_array":{"cols":2,"rows":1,"cabinet_size_mm":[600,340]},
                   "shape_prior":"flat"},
        "capture_manifest_path": paths["capture"],
        "screen_mapping_path": paths["screen_mapping"]})
    rc = run_reconstruct(inp)
    assert rc == 0
    rep = json.loads(open(paths["pose_report"]).read())
    assert rep["schema_version"] == "visual_pose_report.v1"
    poses = {p["cabinet_id"]: p for p in rep["cabinet_poses"]}
    c0 = np.array(poses["V000_R000"]["position_mm"])
    c1 = np.array(poses["V001_R000"]["position_mm"])
    assert abs(np.linalg.norm(c1-c0) - 700.0) < 5.0     # 距离误差 < 5mm
    n0 = np.array(poses["V000_R000"]["normal"]); n1 = np.array(poses["V001_R000"]["normal"])
    ang = np.degrees(np.arccos(np.clip(n0@n1,-1,1)))
    assert abs(ang - 10.0) < 0.5                          # 夹角误差 < 0.5°
```

- [ ] **Step 2: 跑确认失败** → FAIL
- [ ] **Step 3: 实现 reconstruct.py 重构 + ipc.py `ReconstructInput` 改写**（按上面"改动要点";`_world_to_model`/`_select_anchors_*`/`procrustes` 调用全删;新增 `_cabinet_geometry_from_pose`(复用 `eval_runner.reconstruct_cabinet_geometry` 的逻辑,抽到公共处)）。
- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(sidecar): reconstruct via model-constrained BA + capture_manifest + pose report; drop A/C anchoring"`

### Task 1.5：calibrate.py 门槛收紧

**Files:** Modify `python-sidecar/src/lmt_vba_sidecar/calibrate.py`;Test `tests/test_calibrate.py`(追加)

- [ ] **Step 1: 写失败测试** — 断言 `MAX_REPROJECTION_RMS_PX` 现为 `0.5`,且新增"角点覆盖跨度"检查(所有帧 corner 的 bbox 须覆盖 image 面积 ≥ 阈值)在覆盖不足时报 `intrinsics_invalid`。
- [ ] **Step 2: 跑确认失败** → FAIL
- [ ] **Step 3: 实现** — `MAX_REPROJECTION_RMS_PX = 0.5`;加 `_has_corner_coverage(img_points_list, image_size, min_frac=0.6)`;不足 → `intrinsics_invalid`。
- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "fix(sidecar): tighten calibration gate (RMS<0.5px + corner coverage)"`

### Task 1.6：lmt-shared DTO + 错误码 + 退出码 + schema（Rust）

**Files:**
- Modify: `crates/lmt-shared/src/dto.rs`、`envelope.rs`、`exit_codes.rs`、`schema.rs`
- Test: 各文件内 `#[cfg(test)]`(跟现有风格一致)

**错误码新增(envelope.rs `error_codes` + exit_codes.rs,三处同步:常量、退出码、映射、agents-cli.md):**

| error_code 常量 | 字符串 | exit code(新数字) |
|---|---|---|
| `DETECTION_FAILED` | `detection_failed` | 13 |
| `BA_DIVERGED` | `ba_diverged` | 14 |
| `PROCRUSTES_FAILED` | `procrustes_failed` | 15 |
| `INTRINSICS_INVALID` | `intrinsics_invalid` | 16 |
| `OBSERVABILITY_FAILED` | `observability_failed` | 17 |
| `DECODE_FAILED` | `decode_failed` | 18 |

(`image_load_failed` 复用现有 `IO`;`invalid_input`/`internal` 已有。13–18 在 `surface_fit_failed=12` 之后,无冲突。)

- [ ] **Step 1: 写失败测试** — 在 `exit_codes.rs` 的 `each_known_error_code_maps_to_distinct_exit_code` 测试数组追加 6 个新 pair;在 `dto.rs` 加 `visual_dto_roundtrips` 测试断言 `VisualReconstructResult`/`CabinetPoseSummary`/`SimulateResult`/`EvalResult` serde + schemars 可用。
- [ ] **Step 2: 跑确认失败** → `cargo test -p lmt-shared` FAIL
- [ ] **Step 3: 实现**
  - `envelope.rs error_codes`:加 6 个 `pub const`;`from_api_error_code` 加 6 个 match 臂。
  - `exit_codes.rs`:加 `DETECTION_FAILED=13 … DECODE_FAILED=18`;`from_api_error_code` 加映射;测试数组同步。
  - `dto.rs`:加(`#[derive(Serialize,Deserialize,JsonSchema)]`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CabinetPoseSummary {
    pub cabinet_id: String,
    pub position_mm: [f64; 3],
    pub normal: [f64; 3],
    pub reprojection_rms_px: f64,
    pub observed_views: u32,
    pub quality: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VisualReconstructResult {
    pub screen_id: String,
    pub measured_yaml_path: String,
    pub pose_report_path: String,
    pub cabinet_count: usize,
    pub ba_rms_px: f64,
    pub cabinets: Vec<CabinetPoseSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SimulateResult { pub dataset_dir: String, pub n_views: u32, pub n_observations: u32, pub seed: i64 }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvalResult {
    pub method: String,
    pub max_size_error_mm: f64,
    pub max_distance_error_mm: f64,
    pub max_angle_error_deg: f64,
    pub seeds: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CalibrateResult { pub intrinsics_path: String, pub reproj_error_px: f64, pub frames_used: u32 }

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneratePatternResult { pub output_dir: String, pub cabinet_count: usize, pub markers_per_cabinet: u32 }
```
  - `schema.rs dump_all`:加 `add!("VisualReconstructResult", dto::VisualReconstructResult);` 等 6 个 + `CabinetPoseSummary`。
- [ ] **Step 4: 跑确认通过** → `cargo test -p lmt-shared` PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(shared): visual DTOs + error/exit codes + schema dump"`

### Task 1.7：adapter-visual-ba 新 api（Rust）

**Files:** Modify `crates/adapter-visual-ba/src/api.rs`、`ipc.rs`、`error.rs`;Test `tests/api_test.rs`、`tests/sidecar_e2e_test.rs`

**改动:**
- `ipc.rs`:`ReconstructProject` 移除 `frame_strategy`/`frame_anchors`;新增 `CaptureManifestRef`/`ScreenMappingRef`(路径)、`SimulateArgs`/`EvalArgs`/`CalibrateArgs`/`GeneratePatternArgs` payload struct;`Event` 加 simulate/eval 的 result 变体(或复用通用 result + 不同 data)。`VbaError` 加 `Protocol{code}` 已能承载新错误码(`detection_failed` 等),无需新枚举,但 `error.rs` 把 sidecar `code` 透传到上层(已是 `VbaError::Protocol{code,message}`)。
- `api.rs`:新增
  - `pub async fn calibrate(args: CalibrateArgs) -> VbaResult<CalibrateOut>`
  - `pub async fn generate_pattern(args: GeneratePatternArgs) -> VbaResult<GeneratePatternOut>`
  - `pub async fn reconstruct(args: ReconstructArgs)`(改:payload 用 capture_manifest_path + screen_mapping_path;返回额外带 `pose_report_path`)
  - `pub async fn simulate(args: SimulateArgs) -> VbaResult<SimulateOut>`
  - `pub async fn eval(args: EvalArgs) -> VbaResult<EvalOut>`
  - 全部走 `run_sidecar(SidecarRequest{ subcommand, payload, .. })`。

- [ ] **Step 1: 写失败测试** — `sidecar_e2e_test.rs` 加 `simulate_then_eval_roundtrip`(用真实 `python -m lmt_vba_sidecar` via `LMT_VBA_SIDECAR_PATH`,跑 simulate 写 tmp dataset,再 eval,断言 `EvalOut.max_distance_error_mm < 3.0`);`api_test.rs` 加 reconstruct payload 形状断言(mock)。
- [ ] **Step 2: 跑确认失败** → `cargo test -p lmt-adapter-visual-ba` FAIL
- [ ] **Step 3: 实现** 上述 api fn + ipc struct。
- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(adapter): visual calibrate/generate-pattern/reconstruct/simulate/eval api"`

### Task 1.8：lmt-app::visual 服务层（Rust）

**Files:** Create `crates/lmt-app/src/visual.rs`;Modify `crates/lmt-app/src/lib.rs`(`pub mod visual;`)、`Cargo.toml`(加 `lmt-adapter-visual-ba` 依赖 + `tokio` runtime)
Test: `crates/lmt-app/tests/`(若有)或 `visual.rs` 内 `#[cfg(test)]`

**职责:** 业务编排,**不做 transport 翻译**。每个 `run_*` 同步签名(内部用 `tokio::runtime::Runtime::new()?.block_on(...)` 跑 adapter 的 async fn,因为 CLI 是同步的),解析 project.yaml 拿 cabinet_array/screen 配置,调 adapter,落盘(measured.yaml / pose_report.json / intrinsics.json / dataset),返回 `lmt_shared::dto::*Result`。

```rust
// crates/lmt-app/src/visual.rs（骨架）
use lmt_shared::dto::{VisualReconstructResult, SimulateResult, EvalResult, CalibrateResult, GeneratePatternResult};
use lmt_shared::error::{LmtError, LmtResult};
use std::path::Path;

fn rt() -> LmtResult<tokio::runtime::Runtime> {
    tokio::runtime::Runtime::new().map_err(|e| LmtError::Other(format!("tokio runtime: {e}")))
}

pub fn run_reconstruct(project_path: &Path, screen_id: &str, capture_manifest: &Path)
    -> LmtResult<VisualReconstructResult> {
    use lmt_adapter_visual_ba::api::{reconstruct, ReconstructArgs};
    let cfg = crate::projects::load_project_yaml_from_path(project_path)?;
    if !cfg.screens.contains_key(screen_id) {
        return Err(LmtError::NotFound(format!("screen '{screen_id}' not in project")));
    }
    // ReconstructArgs 的新构造器(Task 1.7):从 capture_manifest + project cfg 组装 payload。
    let args = ReconstructArgs::from_capture_manifest(project_path, screen_id, capture_manifest, &cfg)?;
    let out = rt()?.block_on(reconstruct(args)).map_err(map_vba_err)?;
    // adapter 返回(Task 1.7 扩展):measured_points + pose_report_path + ba_rms_px + cabinet_summaries。
    let measured_path =
        crate::measurements::write_measured_yaml(project_path, screen_id, &out.measured_points)?;
    Ok(VisualReconstructResult {
        screen_id: screen_id.to_string(),
        measured_yaml_path: measured_path.display().to_string(),
        pose_report_path: out.pose_report_path,
        cabinet_count: out.measured_points.points.len(),
        ba_rms_px: out.ba_rms_px,
        cabinets: out.cabinet_summaries,
    })
}

/// adapter `VbaError` → `LmtError`,透传 sidecar 错误 code(见 Task 1.6 错误码表)。
fn map_vba_err(e: lmt_adapter_visual_ba::error::VbaError) -> LmtError {
    match &e {
        lmt_adapter_visual_ba::error::VbaError::Protocol { code, message } =>
            LmtError::Visual(code.clone(), message.clone()),  // Task 1.8 给 LmtError 加 Visual(code,msg)
        lmt_adapter_visual_ba::error::VbaError::Cancelled => LmtError::Other("cancelled".into()),
        other => LmtError::Other(other.to_string()),
    }
}
// run_calibrate / run_generate_pattern / run_simulate / run_eval 同构:block_on 对应 adapter fn → 落盘 → 映射 DTO。
```

> **执行期对齐:** `write_measured_yaml` 的确切签名按现有 `crates/lmt-app/src/measurements.rs` 核对;`ReconstructArgs::from_capture_manifest`、adapter 返回结构、`LmtError::Visual` 变体均在 Task 1.6/1.7/1.8 内定义,本块引用即来自那些任务。

- [ ] **Step 1: 写失败测试** — `run_simulate` 写出 dataset dir 且返回 `SimulateResult.n_views` 正确(用 `LMT_VBA_SIDECAR_PATH` 指向 `python -m`)。
- [ ] **Step 2: 跑确认失败** → `cargo test -p lmt-app` FAIL
- [ ] **Step 3: 实现** 5 个 `run_*`(`reconstruct`/`calibrate`/`generate_pattern`/`simulate`/`eval`)。错误:adapter `VbaError::Protocol{code,..}` → 用 code 直接构 `LmtError`(加 `LmtError` 变体 or 在 visual.rs 内 map 到对应 `ApiError` code;**执行期**:倾向在 `lmt-shared::error` 加 `Visual(String, String)` 承载 (code,message),`From<LmtError> for ApiError` 用其 code)。
- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(app): visual service-layer helpers (run_reconstruct/calibrate/generate-pattern/simulate/eval)"`

### Task 1.9：lmt-cli `visual` 子命令（Rust）— 完整 reconstruct + 其余 4 个同构

**Files:** Modify `crates/lmt-cli/src/cli.rs`(加 `Visual(VisualCmd)` + `VisualCmd` enum)、`main.rs`(dispatch)、`commands/mod.rs`(`pub mod visual;`);Create `commands/visual.rs`

- [ ] **Step 1: 写失败测试**(放 cli_e2e,见 Task 1.10,先红)
- [ ] **Step 2: cli.rs 加枚举**

```rust
// cli.rs Command 枚举追加:
/// 相机视觉测量(零全站仪):标定 / 生成 pattern / 重建 / 合成台。
Visual(VisualCmd),

#[derive(Subcommand)]
pub enum VisualCmd {
    /// 棋盘格 → intrinsics.json。side_effect: destructive
    Calibrate { project_path: String, screen_id: String, checkerboard_dir: String,
                #[arg(long, default_value_t = 20.0)] square_mm: f64,
                #[arg(long, default_value="9x9")] inner: String },
    /// 生成 ChArUco pattern 三件套。side_effect: destructive
    GeneratePattern { project_path: String, screen_id: String,
                      #[arg(long, default_value="charuco")] method: String },
    /// 多视角照片 → measured.yaml + cabinet_pose_report.json。side_effect: destructive
    Reconstruct { project_path: String, screen_id: String,
                  #[arg(long)] capture_manifest: Option<String>,
                  #[arg(long)] images: Option<String>,
                  #[arg(long, default_value="charuco")] method: String },
    /// 合成数据集生成。side_effect: destructive
    Simulate { config: String, #[arg(long)] out: String },
    /// 方法 vs 真值评估。side_effect: write_safe
    Eval { dataset: String, #[arg(long, default_value="charuco")] method: String },
}
```

- [ ] **Step 3: commands/visual.rs 实现**(完整 reconstruct,其余同构)

```rust
//! `lmt visual ...` 子命令。thin transport:解析 → 调 lmt_app::visual → envelope。
use crate::cli::VisualCmd;
use crate::commands::util::{self, DestructiveDecision};
use crate::output::{self, Mode};
use lmt_shared::envelope::{error_codes, ApiError};
use std::io::Write as _;
use std::path::Path;

pub fn run(cmd: VisualCmd, mode: Mode, yes: bool, dry_run: bool) -> i32 {
    match cmd {
        VisualCmd::Reconstruct { project_path, screen_id, capture_manifest, images, method } =>
            reconstruct(mode, &project_path, &screen_id, capture_manifest, images, &method, yes, dry_run),
        VisualCmd::Simulate { config, out } => simulate(mode, &config, &out, yes, dry_run),
        VisualCmd::Eval { dataset, method } => eval(mode, &dataset, &method),
        VisualCmd::Calibrate { project_path, screen_id, checkerboard_dir, square_mm, inner } =>
            calibrate(mode, &project_path, &screen_id, &checkerboard_dir, square_mm, &inner, yes, dry_run),
        VisualCmd::GeneratePattern { project_path, screen_id, method } =>
            generate_pattern(mode, &project_path, &screen_id, &method, yes, dry_run),
    }
}

fn reconstruct(mode: Mode, project_path: &str, screen_id: &str,
               capture_manifest: Option<String>, images: Option<String>,
               method: &str, yes: bool, dry_run: bool) -> i32 {
    if method != "charuco" {
        return output::err(mode, ApiError::new(error_codes::UNSUPPORTED,
            "only --method charuco implemented (structured-light is gated, spec §16)"));
    }
    // capture_manifest 与 images 二选一(images 走 convenience 转 manifest)。
    let manifest = match (capture_manifest, images) {
        (Some(m), _) => m,
        (None, Some(_dir)) => return output::err(mode, ApiError::new(error_codes::UNSUPPORTED,
            "--images convenience not yet wired; pass --capture-manifest")),
        (None, None) => return output::err(mode, ApiError::new(error_codes::INVALID_INPUT,
            "need --capture-manifest <json> (or --images <dir>)")),
    };
    let decision = match util::gate_destructive(yes, dry_run, "visual reconstruct") {
        Ok(d) => d, Err(e) => return output::err(mode, e),
    };
    match decision {
        DestructiveDecision::DryRun => {
            let payload = serde_json::json!({"dry_run": true,
                "would_write": format!("{project_path}/measurements/{screen_id}/measured.yaml + reports/cabinet_pose_report.json"),
                "capture_manifest": manifest});
            output::ok(mode, payload, |_| {
                let _ = writeln!(std::io::stdout(),
                    "[dry-run] would reconstruct screen {screen_id} from {manifest}");
            })
        }
        DestructiveDecision::Execute => {
            match lmt_app::visual::run_reconstruct(Path::new(project_path), screen_id, Path::new(&manifest)) {
                Ok(r) => output::ok(mode, r, |p| {
                    let _ = writeln!(std::io::stdout(),
                        "reconstructed {} cabinets (ba_rms={:.3}px)\n  measured: {}\n  poses: {}",
                        p.cabinet_count, p.ba_rms_px, p.measured_yaml_path, p.pose_report_path);
                }),
                Err(e) => output::err(mode, ApiError::from(e)),
            }
        }
    }
}
// simulate / calibrate / generate_pattern:同构(destructive → gate_destructive → dry-run 预览 / Execute 调 lmt_app::visual::run_*)
// eval:read/write_safe,不 gate,直接调 run_eval → output::ok
```

- [ ] **Step 4: main.rs dispatch** 加 `Command::Visual(c) => commands::visual::run(c, mode, yes, dry_run)`;`cargo build` 通过。
- [ ] **Step 5: Commit** — `git commit -m "feat(cli): lmt visual calibrate/generate-pattern/reconstruct/simulate/eval"`

### Task 1.10：CLI E2E（Rust）

**Files:** Modify `crates/lmt-cli/tests/cli_e2e.rs`

- [ ] **Step 1: 写测试**(happy / refuse / dry-run / error envelope 各覆盖)

```rust
// 关键用例(跟现有 cli_e2e 风格一致,用 assert_cmd / 临时 DB / LMT_VBA_SIDECAR_PATH 指 python -m):
// 1. visual simulate --out tmp → exit 0, stdout envelope ok=true
// 2. visual eval --dataset tmp --method charuco → ok, data.max_distance_error_mm < 3.0
// 3. visual reconstruct(无 --capture-manifest)→ exit 2(invalid_input), stderr ErrorEnvelope
// 4. visual reconstruct --method structured-light → exit 7(unsupported)
// 5. visual reconstruct ... --dry-run → ok, data.dry_run=true, 不写盘
```

- [ ] **Step 2: 跑确认(部分先失败,实现后)** → `cargo test -p lmt-cli --test cli_e2e` PASS
- [ ] **Step 3-5: 修到绿 + Commit** — `git commit -m "test(cli): visual op e2e (happy/refuse/dry-run/envelope)"`

### Task 1.11：docs 同步

**Files:** Modify `docs/agents-cli.md`(命令表加 5 行 + side_effect + 错误码表加 6 行)、`docs/contract-manifest.json`(刷新快照:`lmt manifest --output json > docs/contract-manifest.json` 后人工核对)

- [ ] **Step 1**: `lmt --json schema | jq '.types | keys'` 确认含新 DTO;`lmt manifest --output json | jq '.operations[].operation_id'` 含 5 个 `visual.*`。
- [ ] **Step 2**: 更新 `agents-cli.md`(命令表、side_effect 段、错误码表三处)。
- [ ] **Step 3**: 刷新 `contract-manifest.json`。
- [ ] **Step 4: Commit** — `git commit -m "docs(cli): agents-cli + contract-manifest for visual ops"`

**Phase 1 验收:** `cargo test --workspace` + `pytest` 全过;自检命令(CLAUDE.md):`./target/debug/lmt --json schema | jq`、`lmt manifest`、`lmt visual reconstruct --help` 全部 OK。

---

## Phase 2 — 两显示器台架验证（已知几何，零全站仪）

**Phase 2 出口判据:** 用户用两台已知尺寸显示器实拍 → `lmt visual reconstruct` → `lmt visual compare-known` 输出尺寸/距离/夹角误差,达到 spec §10.3 nominal 档同量级(尺寸 ≤ 2mm、距离 ≤ 3mm、夹角 ≤ 0.3°,起步值;不达标则按 §11 风险表分析:标定?检测率?screen_mapping 尺寸填错?)。

> **本阶段性质:** 算法已在 Phase 0 合成台证过;Phase 2 是**首个现实验证**,验几何 + 尺度 + 前端在真实清晰显示器上的表现(LED bloom/摩尔纹要等真 LED,本期不覆盖,spec §11 已记)。采集是**手动**(用户),工具部分是 `compare-known`。

### Task 2.1：已知几何对账工具 `visual compare-known`

**Files:**
- Create: `python-sidecar/src/lmt_vba_sidecar/compare_known.py` + sidecar 子命令 `compare_known`
- Create: `crates/lmt-app/src/visual.rs::run_compare_known` + `lmt-cli` `VisualCmd::CompareKnown`
- Test: `python-sidecar/tests/test_compare_known.py`

**输入:** `cabinet_pose_report.json`(重建结果)+ `known_geometry.json`(用户填的真值):
```json
{ "cabinets": {"V000_R000": {"size_mm": [600,340]}, "V001_R000": {"size_mm": [600,340]}},
  "pairs": [{"a":"V000_R000","b":"V001_R000","distance_mm": 705.0, "angle_deg": 12.0}] }
```
**输出:** 每 cabinet 尺寸误差、每 pair 距离/夹角误差 + pass/fail(对阈值)。

- [ ] **Step 1: 写失败测试**

```python
# tests/test_compare_known.py
import json
from lmt_vba_sidecar.compare_known import compare_known

def test_compare_known_computes_errors(tmp_path):
    report = {"schema_version":"visual_pose_report.v1","frame":{},"cabinet_poses":[
        {"cabinet_id":"V000_R000","position_mm":[0,0,0],"normal":[0,0,1],
         "rotation_matrix":[[1,0,0],[0,1,0],[0,0,1]],
         "corners_mm":[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]],
         "reprojection_rms_px":0.4,"observed_views":7,"observed_points":120,"quality":"ok"},
        {"cabinet_id":"V001_R000","position_mm":[702,0,0],"normal":[0.0,0.0,1.0],
         "rotation_matrix":[[1,0,0],[0,1,0],[0,0,1]],
         "corners_mm":[[-300,-170,0],[300,-170,0],[300,170,0],[-300,170,0]],
         "reprojection_rms_px":0.4,"observed_views":7,"observed_points":120,"quality":"ok"}]}
    known = {"cabinets":{"V000_R000":{"size_mm":[600,340]},"V001_R000":{"size_mm":[600,340]}},
             "pairs":[{"a":"V000_R000","b":"V001_R000","distance_mm":700.0,"angle_deg":0.0}]}
    out = compare_known(report, known)
    assert abs(out["pairs"][0]["distance_error_mm"] - 2.0) < 1e-6   # |702-700|
    assert out["pairs"][0]["angle_error_deg"] < 1e-6
```

- [ ] **Step 2: 跑确认失败** → FAIL
- [ ] **Step 3: 实现** `compare_known(report, known)`:cabinet 尺寸从 `corners_mm` 主轴跨度算;pair 距离从两 `position_mm`;pair 夹角从两 `normal`;对比 known,输出误差 + `pass`(对阈值,阈值从入参或默认)。接 sidecar 子命令 + lmt-app + CLI(同 Task 1.8/1.9 模式,read/write_safe)。
- [ ] **Step 4: 跑确认通过** → PASS
- [ ] **Step 5: Commit** — `git commit -m "feat(visual): compare-known geometry tool (size/distance/angle errors)"`

### Task 2.2：显示器台架协议 + screen_mapping + 报告模板

**Files:**
- Create: `docs/poc/2026-XX-XX-monitor-bench-report.md`(模板)
- Create: 示例 `examples/monitor-bench/screen_mapping.json`、`known_geometry.json`、`project.yaml`(两 cabinet)

- [ ] **Step 1**: 写协议(报告模板),含:
  - **显示器规格 → screen_mapping**:量/查每台对角线 + 原生分辨率 → 算 `pixel_pitch_mm` 与 `active_size_mm`(注意:必须**原生分辨率全屏**显示 pattern,关 OS 缩放/HiDPI 放大,否则违反 §2 的 1:1 前提)。
  - **采集 SOP**:两台显示器并排/带夹角摆放;`lmt visual generate-pattern` 出两 cabinet 的 ChArUco;各显示器全屏显示对应 cabinet PNG;相机 12–20 机位(远中近 + 左右斜),每机位 1 张;确保每台显示器在足够多机位里清晰可见(满足 §12 可观测性)。
  - **真值录入**:卷尺量两显示器发光面中心间距、量行可量的夹角(或按摆放角度)→ 填 `known_geometry.json`。
  - **跑**:`lmt visual calibrate`(相机标定,同镜头同焦)→ `reconstruct --capture-manifest` → `compare-known` → 填报告 pass/fail。
- [ ] **Step 2: Commit** — `git commit -m "docs(poc): two-monitor bench protocol + report template + example configs"`

### Task 2.3：执行台架测试（手动，用户）

- [ ] 用户按 Task 2.2 协议实拍 + 跑工具链,填 `docs/poc/2026-MM-DD-monitor-bench-report.md`,提交。
- [ ] **判定**:达标 → Phase 3;不达标 → 按 §11 风险表定位(标定 RMS?检测率?screen_mapping 尺寸?机位覆盖?),修复后重跑。**此为 manual checkpoint,Claude 不自动化。**

---

## Phase 3 — 鲁棒性收紧 + 生产化打包

**Phase 3 出口判据:** `cargo test --workspace` + `pytest` 全过;PyInstaller 在 Windows + macOS 各出单可执行;CI 绿;`lmt visual reconstruct` 长任务可 cancel(< 5s 退出)、所有新错误码都有 E2E 覆盖。

### Task 3.1：LED 像素间距误差扫描 + 报告

**Files:** Modify `eval_cmd.py`(加 `--pitch-sweep`);Test `tests/test_eval_matrix.py`(追加)

- [ ] **Step 1: 写失败测试** — `eval` 在 `pixel_pitch_error_frac ∈ {0, 0.002, 0.005}` 扫描下,报告每档尺度误差随 pitch 误差单调增,且 0.002(典型) 档仍 ≤ 10mm。
- [ ] **Step 2-4**: 实现 pitch sweep + 报告字段;跑绿。
- [ ] **Step 5: Commit** — `git commit -m "feat(sidecar): pixel-pitch error sweep in eval"`

> **执行期锁定:** 典型 LED 处理器缩放误差量级需查实际项目 LED 规格;sweep 档位按真实量级调。

### Task 3.2：per-cabinet 质量门接入 reconstruct 输出

**Files:** Modify `reconstruct.py`(每 cabinet 算 `quality`:`low_observation`(views<min)/`high_residual`(per-cabinet reproj RMS>阈)/`ok`),发 `WarningEvent`;Test `tests/test_reconstruct.py`(追加)

- [ ] **Step 1: 写失败测试** — 构造一个只被 1 个 view 看到的 cabinet → pose_report 里该 cabinet `quality=="low_observation"` 且发了 warning。
- [ ] **Step 2-4**: 实现;跑绿。
- [ ] **Step 5: Commit** — `git commit -m "feat(sidecar): per-cabinet quality gates in reconstruct output"`

### Task 3.3：cancel + 错误完整化

**Files:** Modify `crates/adapter-visual-ba/tests/cancel_test.rs`(reconstruct 长任务 cancel);`crates/lmt-cli/tests/cli_e2e.rs`(每个新错误码各一 E2E)

- [ ] **Step 1: 写失败测试** — `cancel_test`:启动一个 sleep-长的 reconstruct(用一个慢 fixture sidecar 或大数据集),发 cancel,断言 < 5s 内 `VbaError::Cancelled` 且子进程被 kill;cli_e2e 加 `detection_failed`/`ba_diverged`/`observability_failed`/`intrinsics_invalid` 各触发一次(喂构造好的坏输入),断言对应 exit code(13/14/17/16)+ ErrorEnvelope。
- [ ] **Step 2-4**: 补实现(adapter cancel 已有机制 §sidecar.rs,主要补测试 + 错误透传);跑绿。
- [ ] **Step 5: Commit** — `git commit -m "test(visual): cancel + error-code envelope coverage"`

### Task 3.4：PyInstaller 打包脚本 + 定位

**Files:** Create `python-sidecar/build_exe.ps1`、`python-sidecar/build_exe.sh`;Test `crates/adapter-visual-ba/tests/locate_test.rs`(已存在,补 vendor 路径 case)

- [ ] **Step 1**: 写 `build_exe.sh`(macOS arm64)+ `build_exe.ps1`(Windows x86_64):`pyinstaller --onefile --name lmt-vba-sidecar src/lmt_vba_sidecar/__main__.py`,输出到 `target/sidecar-vendor/<platform>/`。opencv-contrib 的 hidden imports / data 按 PyInstaller 报错补 `--hidden-import` / `--collect-data cv2`。
- [ ] **Step 2**: 本地各跑一次,产物存在且 `target/sidecar-vendor/<platform>/lmt-vba-sidecar[.exe] simulate < payload` 能跑(干净环境无 Python)。
- [ ] **Step 3**: `locate_test.rs` 补"vendor 路径优先于 PATH"用例。
- [ ] **Step 4: Commit** — `git commit -m "build(sidecar): PyInstaller onefile scripts (win/mac) + locate vendor path"`

> **执行期锁定:** opencv-contrib + scipy 的 PyInstaller hidden imports / 二进制收集清单按首次打包报错补全(常见 `cv2`、`scipy.optimize`、`scipy.sparse.csgraph`)。

### Task 3.5：CI

**Files:** Create `.github/workflows/visual-ci.yml`

- [ ] **Step 1**: workflow:matrix `{windows-latest, macos-14}` →(a) `pip install -e python-sidecar && pytest`;(b) `cargo test --workspace`(用 `LMT_VBA_SIDECAR_PATH=python -m lmt_vba_sidecar`);(c) `build_exe` 产物 smoke(`simulate` 一次)。
- [ ] **Step 2**: push 分支看 CI 绿。
- [ ] **Step 3: Commit** — `git commit -m "ci: visual branch pytest + cargo + pyinstaller smoke (win/mac)"`

**Phase 3 验收:** CI 双平台绿;cancel < 5s;新错误码 E2E 全覆盖;PyInstaller 双平台产物可在干净环境跑 `simulate`。

---

## 跨阶段说明

- **结构光(OPT-SL)** 不在本 plan;spec §16 已设计,Phase 1/2 达标后另起 plan。其接口已在 `capture_manifest`(`frames` 字段保留)、`detect`(前端可插)、`model_constrained_ba`(同内核)预留。
- **IR 冻结**:全程不改 `crates/core` 的 `MeasuredPoint`;cabinet pose/normal 走 `cabinet_pose_report.json`(spec §9)。
- **CLI 契约**:每个新 op 的"六件套"(helper/CLI/DTO/E2E/agents-cli/manifest)在 Phase 1 Task 1.6–1.11 完成;Phase 2 的 `compare-known` 同样补齐六件套。
- **执行期锁定项汇总**:精度阈值(§10.3,Phase 0 锁)、exit code 已定(13–18)、标定 RMS(0.5px 起步)、pitch sweep 档位(Phase 3.1)、PyInstaller hidden imports(Phase 3.4)、`--images` convenience 转 manifest(Phase 1 留 UNSUPPORTED,后补)。
