"""CLI run-function for the 'eval' subcommand.

Loads a saved scene.npz dataset, reconstructs the Scene object, runs a
reconstruction method via eval_runner.run_method, and emits an EvalResultEvent.
"""
from __future__ import annotations

import pathlib

import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    ErrorEvent,
    EvalInput,
    EvalResultData,
    EvalResultEvent,
)
from lmt_vba_sidecar.simulate import Scene
from lmt_vba_sidecar.model_constrained_ba import Observation
from lmt_vba_sidecar.eval_runner import run_method


def run_eval(cmd: EvalInput) -> int:
    npz_path = pathlib.Path(cmd.dataset_dir) / "scene.npz"
    if not npz_path.exists():
        write_event(ErrorEvent(
            event="error",
            code="invalid_input",
            message=f"dataset not found: {npz_path}",
            fatal=True,
        ))
        return 1

    data = np.load(npz_path, allow_pickle=False)

    # Rebuild camera poses: list of (R, t) tuples
    cam_R = data["cam_R"]   # (n_cam, 3, 3)
    cam_t = data["cam_t"]   # (n_cam, 3)
    true_camera_poses = [(cam_R[i], cam_t[i]) for i in range(len(cam_R))]

    # Rebuild cabinet poses: dict int -> (R, t)
    cab_ids = data["cab_ids"].tolist()   # list[int] (use int() keys for dict)
    cab_R = data["cab_R"]
    cab_t = data["cab_t"]
    true_cabinet_poses = {int(cab_ids[i]): (cab_R[i], cab_t[i]) for i in range(len(cab_ids))}

    # Rebuild cabinet corners: dict int -> (M,3)
    corners = data["corners"]   # (n_cab, M, 3)
    cabinet_corners_local = {int(cab_ids[i]): corners[i] for i in range(len(cab_ids))}

    # Rebuild observations list
    obs_cam = data["obs_cam"]
    obs_cab = data["obs_cab"]
    obs_plocal = data["obs_plocal"]
    obs_pixel = data["obs_pixel"]
    observations = [
        Observation(
            camera_idx=int(obs_cam[i]),
            cabinet_idx=int(obs_cab[i]),
            p_local=obs_plocal[i],
            pixel=obs_pixel[i],
        )
        for i in range(len(obs_cam))
    ]

    scene = Scene(
        K=data["K"],
        true_camera_poses=true_camera_poses,
        true_cabinet_poses=true_cabinet_poses,
        cabinet_corners_local=cabinet_corners_local,
        observations=observations,
        n_cameras=len(true_camera_poses),
        n_cabinets=len(cab_ids),
    )

    try:
        metrics = run_method(scene, cmd.method)
    except ValueError as exc:
        # Unimplemented-but-enum-valid methods (e.g. structured_light) raise
        # ValueError in run_method; surface as invalid_input, not internal_error.
        write_event(ErrorEvent(
            event="error",
            code="invalid_input",
            message=str(exc),
            fatal=True,
        ))
        return 1

    write_event(EvalResultEvent(
        event="result",
        data=EvalResultData(
            method=cmd.method,
            # `seeds` echoes the requested seed_matrix; the current single
            # saved scene is evaluated exactly once. Per-seed evaluation and
            # multi-seed aggregation are future work (Task 2.x). Field name
            # must stay `seeds` to match the Task 1.6 Rust DTO.
            seeds=list(cmd.seed_matrix),
            max_size_error_mm=metrics["max_size_error_mm"],
            max_distance_error_mm=metrics["max_distance_error_mm"],
            max_angle_error_deg=metrics["max_angle_error_deg"],
        ),
    ))
    return 0
