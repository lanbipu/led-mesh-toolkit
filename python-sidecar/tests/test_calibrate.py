"""Calibration tests using OpenCV-rendered synthetic checkerboard frames."""
from __future__ import annotations

import json
import pathlib

import cv2
import numpy as np

from lmt_vba_sidecar.calibrate import run_calibrate
from lmt_vba_sidecar.ipc import CalibrateInput


def _render_3d_checker_view(
    image_size: tuple[int, int],
    inner: tuple[int, int],
    square_mm: float,
    K: np.ndarray,
    R: np.ndarray,
    t: np.ndarray,
) -> np.ndarray:
    """Render a checkerboard as projected by a real camera (K, R, t).

    Iterates pixels in the image; for each, ray-casts back to the board's
    z=0 plane in world coordinates and decides black/white based on the
    checker pattern. Outside the printed area returns 255 (background).
    """
    img_w, img_h = image_size
    cols, rows = inner
    # Board printed area in world frame (z=0). One extra square margin.
    board_w = (cols + 1) * square_mm
    board_h = (rows + 1) * square_mm

    # Projection matrix P = K [R | t]
    Rt = np.hstack([R, t.reshape(3, 1)])
    P = K @ Rt

    img = np.full((img_h, img_w), 255, dtype=np.uint8)
    # Inverse approach: render via per-square polygon fill (much faster than
    # per-pixel ray cast and exact for this synthetic case).
    for r in range(rows + 1):
        for c in range(cols + 1):
            if (r + c) % 2 == 0:
                continue
            world_corners = np.array([
                [c * square_mm, r * square_mm, 0.0, 1.0],
                [(c + 1) * square_mm, r * square_mm, 0.0, 1.0],
                [(c + 1) * square_mm, (r + 1) * square_mm, 0.0, 1.0],
                [c * square_mm, (r + 1) * square_mm, 0.0, 1.0],
            ])
            pix = (P @ world_corners.T).T
            if (pix[:, 2] <= 0).any():
                continue  # behind camera
            pts = (pix[:, :2] / pix[:, 2:3]).astype(np.int32)
            cv2.fillConvexPoly(img, pts, 0)
    return img


def test_calibrate_round_trip_with_varied_views(tmp_out: pathlib.Path) -> None:
    inner = (8, 6)
    image_size = (1920, 1080)
    square_mm = 20.0
    K_true = np.array([[1500, 0, 960], [0, 1500, 540], [0, 0, 1]], dtype=float)

    images: list[str] = []
    for i in range(12):
        # Vary rotation around X / Y / Z and translation per view.
        ang_x = np.deg2rad(-15 + (i % 5) * 8)
        ang_y = np.deg2rad(-10 + (i % 4) * 6)
        ang_z = np.deg2rad((i % 3) * 5)
        Rx = cv2.Rodrigues(np.array([ang_x, 0, 0]))[0]
        Ry = cv2.Rodrigues(np.array([0, ang_y, 0]))[0]
        Rz = cv2.Rodrigues(np.array([0, 0, ang_z]))[0]
        R = Rz @ Ry @ Rx
        t = np.array([
            -square_mm * inner[0] / 2 + (i % 3 - 1) * 30,
            -square_mm * inner[1] / 2 + (i // 3 - 1) * 30,
            500.0 + (i % 4) * 50,
        ])
        img = _render_3d_checker_view(image_size, inner, square_mm, K_true, R, t)
        p = tmp_out / f"chk_{i}.png"
        cv2.imwrite(str(p), img)
        images.append(str(p))

    out_path = tmp_out / "intrinsics.json"
    cmd = CalibrateInput(
        command="calibrate",
        version=1,
        checkerboard_images=images,
        inner_corners=list(inner),
        square_size_mm=20.0,
        output_path=str(out_path),
    )
    rc = run_calibrate(cmd)
    assert rc == 0, f"calibrate failed: rc={rc}"
    data = json.loads(out_path.read_text())
    assert data["image_size"] == [1920, 1080]
    assert isinstance(data["reproj_error_px"], float)
    fx = data["K"][0][0]
    fy = data["K"][1][1]
    assert fx > 0 and fy > 0
    cx = data["K"][0][2]
    cy = data["K"][1][2]
    assert 0 < cx < 1920 and 0 < cy < 1080


def test_calibrate_too_few_detections_emits_error(tmp_out: pathlib.Path) -> None:
    blank_path = tmp_out / "blank.png"
    blank = np.full((1080, 1920), 255, dtype=np.uint8)
    cv2.imwrite(str(blank_path), blank)
    images = [str(blank_path)] * 5
    cmd = CalibrateInput(
        command="calibrate", version=1,
        checkerboard_images=images, inner_corners=[8, 6],
        square_size_mm=20.0, output_path=str(tmp_out / "ix.json"),
    )
    rc = run_calibrate(cmd)
    assert rc != 0
    assert not (tmp_out / "ix.json").exists()


def test_calibrate_identical_frames_rejected_for_pose_diversity(tmp_out: pathlib.Path) -> None:
    """All-identical frames must be rejected before solving — calibration
    on a no-baseline set silently produces meaningless intrinsics."""
    inner = (8, 6)
    image_size = (1920, 1080)
    square_mm = 20.0
    K_true = np.array([[1500, 0, 960], [0, 1500, 540], [0, 0, 1]], dtype=float)
    R = np.eye(3)
    t = np.array([-square_mm * inner[0] / 2, -square_mm * inner[1] / 2, 500.0])
    same_img = _render_3d_checker_view(image_size, inner, square_mm, K_true, R, t)

    images: list[str] = []
    for i in range(8):
        p = tmp_out / f"chk_{i}.png"
        cv2.imwrite(str(p), same_img)
        images.append(str(p))
    out_path = tmp_out / "intrinsics.json"
    cmd = CalibrateInput(
        command="calibrate", version=1,
        checkerboard_images=images, inner_corners=list(inner),
        square_size_mm=square_mm, output_path=str(out_path),
    )
    rc = run_calibrate(cmd)
    assert rc != 0
    assert not out_path.exists()
