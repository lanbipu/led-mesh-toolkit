"""ChArUco marker detection across an image set.

Returns one observation list per image: each observation is a marker corner
pixel coordinate plus its ArUco ID, ready for bundle adjustment.
"""
from __future__ import annotations

import cv2


def _aruco_dict():
    return cv2.aruco.getPredefinedDictionary(cv2.aruco.DICT_6X6_1000)


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
