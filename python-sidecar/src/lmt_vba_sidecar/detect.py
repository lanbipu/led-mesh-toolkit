"""ChArUco marker detection across an image set.

Returns one observation list per image: each observation is a marker corner
pixel coordinate plus its ArUco ID, ready for bundle adjustment.
"""
from __future__ import annotations

import cv2


def _aruco_dict():
    return cv2.aruco.getPredefinedDictionary(cv2.aruco.DICT_6X6_1000)


def _charuco_board(aruco_id_start: int, inner_corners: int) -> cv2.aruco.CharucoBoard:
    """Reconstruct the same CharucoBoard that generate_cabinet_png() produced.

    Mirrors pattern.py's construction exactly:
      - sub-dict slice: bytesList[start : start + n_markers]
      - board size: (inner+1, inner+1)
      - squareLength=1.0, markerLength=0.7
    """
    aruco_dict = _aruco_dict()
    squares = inner_corners + 1
    n_markers = (squares * squares) // 2  # _markers_per_board formula
    sub_dict = cv2.aruco.Dictionary(
        aruco_dict.bytesList[aruco_id_start:aruco_id_start + n_markers],
        aruco_dict.markerSize,
    )
    return cv2.aruco.CharucoBoard(
        size=(squares, squares),
        squareLength=1.0,
        markerLength=0.7,
        dictionary=sub_dict,
    )


def detect_charuco_corners(
    image_paths: list[str],
    *,
    boards: list[dict] | None = None,
    board_lookup_for_test: bool = False,
) -> dict[str, list[dict]]:
    """Detect ChArUco board corners across a set of images.

    Each returned observation:
      {"cabinet": (col, row), "charuco_id": int, "corner_px": [x, y]}

    Parameters
    ----------
    image_paths:
        Paths to input images (grayscale or colour; read as grayscale internally).
    boards:
        List of board descriptors used in real operation (Task 1.4 reconstruct):
          {"cabinet": (col, row), "aruco_id_start": int, "inner_corners": int}
        Each descriptor recreates the CharucoBoard that generate_cabinet_png()
        produced for that cabinet.
    board_lookup_for_test:
        When True, ignore `boards` and substitute a single default board
        {"cabinet": (0,0), "aruco_id_start": 0, "inner_corners": 8}
        for unit-test use without a real pattern_meta.json.

    Unreadable images yield an empty list (not an exception), matching the
    tolerance of detect_charuco_observations.
    """
    if board_lookup_for_test:
        boards = [{"cabinet": (0, 0), "aruco_id_start": 0, "inner_corners": 8}]
    if not boards:
        return {path: [] for path in image_paths}

    # Pre-build (board_obj, detector, cabinet) tuples once — reused per image.
    board_detectors: list[tuple[cv2.aruco.CharucoBoard, cv2.aruco.CharucoDetector, tuple]] = []
    for desc in boards:
        board = _charuco_board(desc["aruco_id_start"], desc["inner_corners"])
        detector = cv2.aruco.CharucoDetector(board)
        board_detectors.append((board, detector, tuple(desc["cabinet"])))

    out: dict[str, list[dict]] = {}
    for path in image_paths:
        img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
        if img is None:
            out[path] = []
            continue

        observations: list[dict] = []
        for _board, detector, cabinet in board_detectors:
            charuco_corners, charuco_ids, _marker_corners, _marker_ids = detector.detectBoard(img)
            if charuco_ids is None:
                continue
            # charucoCorners: (N,1,2) float32; charucoIds: (N,1) int32
            flat_ids = charuco_ids.flatten()
            flat_corners = charuco_corners.reshape(-1, 2)
            for cid, (cx, cy) in zip(flat_ids, flat_corners):
                observations.append({
                    "cabinet": cabinet,
                    "charuco_id": int(cid),
                    "corner_px": [float(cx), float(cy)],
                })
        out[path] = observations
    return out


def detect_charuco_observations(
    image_paths: list[str],
) -> dict[str, list[dict]]:
    """For each image, return per-marker observations.

    Each observation:
      {"aruco_id": int, "corners_px": [[x0,y0],[x1,y1],[x2,y2],[x3,y3]]}

    Missing or unreadable images yield an empty list (not an exception);
    callers aggregate across the full set and decide via thresholds whether
    detection is sufficient.
    """
    dictionary = _aruco_dict()
    detector_params = cv2.aruco.DetectorParameters()
    detector = cv2.aruco.ArucoDetector(dictionary, detector_params)

    out: dict[str, list[dict]] = {}
    for path in image_paths:
        img = cv2.imread(path, cv2.IMREAD_GRAYSCALE)
        if img is None:
            out[path] = []
            continue
        corners, ids, _ = detector.detectMarkers(img)
        observations: list[dict] = []
        if ids is not None:
            criteria = (cv2.TERM_CRITERIA_EPS + cv2.TERM_CRITERIA_MAX_ITER, 30, 1e-3)
            for marker_corners, marker_id in zip(corners, ids.flatten()):
                refined = cv2.cornerSubPix(
                    img, marker_corners, (5, 5), (-1, -1), criteria,
                )
                observations.append({
                    "aruco_id": int(marker_id),
                    "corners_px": refined.reshape(-1, 2).tolist(),
                })
        out[path] = observations
    return out
