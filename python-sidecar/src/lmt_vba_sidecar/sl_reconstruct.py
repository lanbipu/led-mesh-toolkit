"""Structured-light multi-view reconstruction: N CorrespondenceFiles -> metric
per-cabinet model, via the SAME model_constrained_ba the ChArUco path uses.

The SL path differs from reconstruct.py only in observation SOURCE:
  - cabinet id    : sl_meta.dots[id].cabinet (already tagged at generation)
  - p_local mm    : sl_geometry.sl_local_mm(cabinet_rect, u, v, pitch)
  - camera pixel  : correspondence (x,y), undistorted via reconstruct._undistort_obs
Everything after observation assembly is reconstruct.solve_and_emit (shared).
"""
from __future__ import annotations

from lmt_vba_sidecar.ipc import CorrespondenceFile


def validate_sl_provenance(corr_files: list[CorrespondenceFile], *,
                           expected_sha: str, expected_screen_id: str) -> None:
    """Codex finding 4 gate: every pose file must share ONE screen_id + ONE
    sl_meta_sha256, that sha must equal the sl_meta.json actually being used,
    and the screen_id must match the project/screen. Any mismatch = stale/mixed
    capture -> ValueError (mapped to invalid_input upstream)."""
    screen_ids = {c.screen_id for c in corr_files}
    shas = {c.sl_meta_sha256 for c in corr_files}
    if len(screen_ids) != 1:
        raise ValueError(f"correspondence files disagree on screen_id: {sorted(screen_ids)}")
    if len(shas) != 1:
        raise ValueError(f"correspondence files disagree on sl_meta_sha256: {sorted(shas)}")
    (only_screen,) = screen_ids
    (only_sha,) = shas
    if only_sha != expected_sha:
        raise ValueError(
            f"sl_meta_sha256 mismatch: correspondences were decoded against "
            f"'{only_sha}' but the supplied sl_meta.json hashes to '{expected_sha}'")
    if only_screen != expected_screen_id:
        raise ValueError(
            f"screen_id '{only_screen}' in correspondences != project screen "
            f"'{expected_screen_id}'")
