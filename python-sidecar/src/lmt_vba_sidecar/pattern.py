"""ChArUco pattern generation. Each cabinet gets an independent ChArUco board.

Outputs three artifacts:
  - cabinets/V<col>_R<row>.png    per-cabinet pattern (debug / regenerate)
  - full_screen.png               assembled screen-resolution image (Disguise drop-in)
  - pattern_meta.json             cabinet ↔ ArUco ID range mapping
"""
from __future__ import annotations

import pathlib
import shutil
import tempfile

import cv2
import numpy as np

from lmt_vba_sidecar.io_utils import write_event
from lmt_vba_sidecar.ipc import (
    BaStats,
    ErrorEvent,
    GeneratePatternInput,
    PatternMeta,
    PatternMetaCabinet,
    ProgressEvent,
    ResultData,
    ResultEvent,
)


DEFAULT_ARUCO_DICT = "DICT_6X6_1000"
DEFAULT_INNER_CORNERS = 8  # 8×8 inner corners → 9×9 squares (per spec §5.2)
ABSENT_CELL_FILL = 255  # white block for missing cabinets


def _aruco_dict():
    return cv2.aruco.getPredefinedDictionary(getattr(cv2.aruco, DEFAULT_ARUCO_DICT))


def _markers_per_board(inner_corners: int) -> int:
    """Number of markers a CharucoBoard places on a (n+1)×(n+1) square grid.

    Markers fill alternating squares in a checkerboard pattern; for an
    (n+1)×(n+1) grid that is `((n+1)*(n+1)) // 2` markers.
    """
    squares = inner_corners + 1
    return (squares * squares) // 2


def generate_cabinet_png(
    *,
    out_path: pathlib.Path,
    cabinet_pixel_size: tuple[int, int],
    aruco_id_start: int,
    aruco_dict_name: str = DEFAULT_ARUCO_DICT,
    inner_corners: int = DEFAULT_INNER_CORNERS,
) -> int:
    """Render one cabinet's ChArUco PNG.

    Returns the next free ArUco ID (caller assigns ranges sequentially).
    """
    if aruco_dict_name != DEFAULT_ARUCO_DICT:
        raise ValueError(f"only {DEFAULT_ARUCO_DICT} supported in M2")
    aruco_dict = _aruco_dict()
    n_markers = _markers_per_board(inner_corners)
    if aruco_id_start + n_markers > 1000:
        raise ValueError(
            f"ArUco ID range {aruco_id_start}..{aruco_id_start + n_markers} "
            f"overflows {DEFAULT_ARUCO_DICT} (1000 markers); too many cabinets"
        )

    # Slice the dictionary's bytesList so per-cabinet IDs occupy a contiguous
    # offset and stay unique across cabinets.
    sub_dict = cv2.aruco.Dictionary(
        aruco_dict.bytesList[aruco_id_start:aruco_id_start + n_markers],
        aruco_dict.markerSize,
    )

    board = cv2.aruco.CharucoBoard(
        size=(inner_corners + 1, inner_corners + 1),
        squareLength=1.0,
        markerLength=0.7,
        dictionary=sub_dict,
    )
    img = board.generateImage(cabinet_pixel_size, marginSize=0, borderBits=1)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    cv2.imwrite(str(out_path), img)
    return aruco_id_start + n_markers


def _assemble_screen(
    *,
    out_path: pathlib.Path,
    cabinets_dir: pathlib.Path,
    cabinet_array,
    cabinet_pixel_size: tuple[int, int],
    screen_resolution: tuple[int, int],
) -> None:
    full = np.full(
        (screen_resolution[1], screen_resolution[0]),
        ABSENT_CELL_FILL,
        dtype=np.uint8,
    )
    cw, ch = cabinet_pixel_size
    for col in range(cabinet_array.cols):
        for row in range(cabinet_array.rows):
            tile_path = cabinets_dir / f"V{col:03d}_R{row:03d}.png"
            if not tile_path.exists():
                continue
            tile = cv2.imread(str(tile_path), cv2.IMREAD_GRAYSCALE)
            x0 = col * cw
            y0 = row * ch
            full[y0:y0 + ch, x0:x0 + cw] = tile
    cv2.imwrite(str(out_path), full)


ARUCO_DICT_CAPACITY = 1000  # DICT_6X6_1000 has 1000 markers
ATOMIC_BACKUP_SUFFIX = ".lmt-vba-old"


def _preflight_capacity(cols: int, rows: int, absent: set, inner_corners: int) -> int | None:
    """Return required marker count if it fits, else emit error + return None."""
    n_present = sum(
        1 for col in range(cols) for row in range(rows) if (col, row) not in absent
    )
    markers_each = _markers_per_board(inner_corners)
    required = n_present * markers_each
    if required > ARUCO_DICT_CAPACITY:
        write_event(ErrorEvent(
            event="error",
            code="invalid_input",
            message=(
                f"grid requires {required} ArUco IDs ({n_present} cabinets × "
                f"{markers_each} markers) which exceeds {DEFAULT_ARUCO_DICT} capacity "
                f"({ARUCO_DICT_CAPACITY}); reduce inner_corners or split into screens"
            ),
            fatal=True,
        ))
        return None
    return required


def run_generate_pattern(cmd: GeneratePatternInput) -> int:
    out_dir = pathlib.Path(cmd.output_dir)

    cols = cmd.project.cabinet_array.cols
    rows = cmd.project.cabinet_array.rows
    absent = set(tuple(c) for c in cmd.project.cabinet_array.absent_cells)
    total_cells = cols * rows
    completed = 0

    sw, sh = cmd.screen_resolution
    if sw % cols != 0 or sh % rows != 0:
        write_event(ErrorEvent(
            event="error",
            code="invalid_input",
            message=f"screen_resolution {sw}x{sh} must divide evenly by cabinet grid {cols}x{rows}",
            fatal=True,
        ))
        return 1
    cabinet_pixel_size = (sw // cols, sh // rows)

    if _preflight_capacity(cols, rows, absent, DEFAULT_INNER_CORNERS) is None:
        return 1

    # Generate into a sibling temp dir and atomically swap on success so a
    # mid-run failure leaves the existing output_dir untouched.
    out_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = pathlib.Path(tempfile.mkdtemp(
        prefix=f".{out_dir.name}-staging-",
        dir=str(out_dir.parent),
    ))
    cabinets_dir = staging / "cabinets"
    cabinets_dir.mkdir(parents=True)

    try:
        cabinets_meta: list[PatternMetaCabinet] = []
        next_id = 0
        for row in range(rows):
            for col in range(cols):
                if (col, row) in absent:
                    completed += 1
                    continue
                tile = cabinets_dir / f"V{col:03d}_R{row:03d}.png"
                id_start = next_id
                next_id = generate_cabinet_png(
                    out_path=tile,
                    cabinet_pixel_size=cabinet_pixel_size,
                    aruco_id_start=id_start,
                )
                cabinets_meta.append(
                    PatternMetaCabinet(col=col, row=row, aruco_id_start=id_start, aruco_id_end=next_id - 1)
                )
                completed += 1
                write_event(ProgressEvent(
                    event="progress",
                    stage="output",
                    percent=completed / total_cells,
                    message=f"cabinet V{col:03d}_R{row:03d}",
                ))

        _assemble_screen(
            out_path=staging / "full_screen.png",
            cabinets_dir=cabinets_dir,
            cabinet_array=cmd.project.cabinet_array,
            cabinet_pixel_size=cabinet_pixel_size,
            screen_resolution=(sw, sh),
        )

        meta = PatternMeta(
            aruco_dict=DEFAULT_ARUCO_DICT,
            markers_per_cabinet=_markers_per_board(DEFAULT_INNER_CORNERS),
            checkerboard_inner_corners=DEFAULT_INNER_CORNERS,
            cabinets=cabinets_meta,
        )
        (staging / "pattern_meta.json").write_text(meta.model_dump_json(indent=2))

        # Atomic publish: move existing out_dir aside, rename staging into place.
        backup: pathlib.Path | None = None
        if out_dir.exists():
            backup = out_dir.with_suffix(out_dir.suffix + ATOMIC_BACKUP_SUFFIX)
            if backup.exists():
                shutil.rmtree(backup)
            out_dir.rename(backup)
        try:
            staging.rename(out_dir)
        except OSError:
            if backup is not None and not out_dir.exists():
                backup.rename(out_dir)
            raise
        if backup is not None:
            shutil.rmtree(backup, ignore_errors=True)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise

    write_event(ResultEvent(
        event="result",
        data=ResultData(
            measured_points=[],
            ba_stats=BaStats(rms_reprojection_px=0.0, iterations=0, converged=True),
            frame_strategy_used="nominal_anchoring",
        ),
    ))
    return 0
