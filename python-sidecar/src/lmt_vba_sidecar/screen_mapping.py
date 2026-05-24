"""
screen_mapping.py — scale-trust anchor for the camera-only visual branch.

No total station → metric scale comes entirely from known pixel pitch / active
physical size.  This module:
  - defines the ScreenMapping pydantic model (screen + per-cabinet config),
  - converts charuco_id → local mm (active-surface-center origin, flat z=0),
  - runs a preflight hash/format check before reconstruct.

Physical ChArUco convention
---------------------------
pattern.py builds CharucoBoard(size=(inner+1, inner+1)), so each square side is:
    squareLength = active_size / (inner + 1)

Inner corner k (0-based) sits at distance (k+1)*squareLength from the active-area
edge.  With a center origin the signed coordinate is:

    x = active_w * ((c + 1) / (inner + 1) - 0.5)
    y = active_h * ((r + 1) / (inner + 1) - 0.5)

where  r, c = divmod(charuco_id, inner).

DO NOT use an (inner-1) divisor — that would span edge-to-edge and introduce a
~29 % scale error that would corrupt BA metric scale.
"""
from __future__ import annotations

from typing import Annotated, Literal

import numpy as np
from pydantic import BaseModel, Field

from lmt_vba_sidecar.ipc import PositiveIntPair, PositiveSizePair  # reuse validated types


class ScreenMappingError(Exception):
    """Raised when screen mapping config is invalid or an operation is unsupported."""


# ---------------------------------------------------------------------------
# Validated field aliases (inline to avoid over-abstraction)
# ---------------------------------------------------------------------------

_IntList4 = Annotated[list[int], Field(min_length=4, max_length=4)]


class ScreenMappingCabinet(BaseModel):
    """Per-cabinet geometry and display config."""

    cabinet_id: str

    # Physical pixel dimensions of this cabinet's LED canvas
    resolution_px: PositiveIntPair  # [width_px, height_px]

    # Physical active-area dimensions in millimetres
    active_size_mm: PositiveSizePair  # [width_mm, height_mm]

    # Physical pixel pitch in millimetres (may differ x/y for non-square pixels)
    pixel_pitch_mm: PositiveSizePair  # [pitch_x, pitch_y]

    # Only "center" is supported; non-center values are rejected at model
    # construction time by the Literal type (they never reach preflight).
    active_origin: Literal["center"]

    # Sub-rectangle of the input feed mapped onto this cabinet [x, y, w, h] in px
    input_rect_px: _IntList4

    # Physical rotation of the cabinet relative to the canonical board orientation.
    # Field bounds give a coarse range; model_post_init enforces {0,90,180,270}.
    rotation: Annotated[int, Field(ge=0, le=270)]

    # Horizontal / vertical mirror flags
    mirror_x: bool
    mirror_y: bool

    def model_post_init(self, __context: object) -> None:
        if self.rotation not in {0, 90, 180, 270}:
            raise ValueError(
                f"rotation must be one of {{0, 90, 180, 270}}, got {self.rotation}"
            )


# ---------------------------------------------------------------------------
# Top-level mapping model
# ---------------------------------------------------------------------------

class ScreenMapping(BaseModel):
    """Full screen → cabinet → mm mapping config for a single LED screen."""

    screen_id: str
    cabinets: list[ScreenMappingCabinet]
    expected_pattern_hash: str

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    def _cabinet(self, cabinet_id: str) -> ScreenMappingCabinet:
        """Return the cabinet with the given id, or raise ScreenMappingError."""
        for cab in self.cabinets:
            if cab.cabinet_id == cabinet_id:
                return cab
        raise ScreenMappingError(
            f"Cabinet '{cabinet_id}' not found in screen '{self.screen_id}'. "
            f"Known: {[c.cabinet_id for c in self.cabinets]}"
        )

    # ------------------------------------------------------------------
    # Core geometry
    # ------------------------------------------------------------------

    def charuco_corner_local_mm(
        self,
        cabinet_id: str,
        charuco_id: int,
        inner: int = 8,
    ) -> np.ndarray:
        """
        Return the local-mm coordinate of a ChArUco inner corner as [x, y, 0].

        The origin is the center of the cabinet's active area.  Positive x is
        right, positive y is down (matches image convention).

        Parameters
        ----------
        cabinet_id : str
        charuco_id : int   0-based index into the charuco inner corner list
        inner      : int   number of inner corners per side (default 8 for a 9x9
                           square board).
                           Callers should always pass `inner` explicitly (from
                           `pattern_meta.checkerboard_inner_corners`). The
                           default=8 is a convenience for the monitor-bench
                           fixture only — a wrong `inner` produces wrong mm
                           coordinates (a scale bug, same class as the
                           module-level warning above).

        Raises
        ------
        ScreenMappingError
            - cabinet_id not found
            - cabinet has rotation != 0 or mirror_x/mirror_y set (deferred; MVP
              uses rotation=0 / no-mirror only — fail loud, not silent)
        """
        cab = self._cabinet(cabinet_id)

        # Guard: rotation and mirror handling is deferred.  Return wrong coords
        # silently would corrupt BA metric scale, so we raise instead.
        if cab.rotation != 0 or cab.mirror_x or cab.mirror_y:
            raise ScreenMappingError(
                "rotation/mirror not yet supported in local-mm mapping. "
                f"Cabinet '{cabinet_id}' has rotation={cab.rotation}, "
                f"mirror_x={cab.mirror_x}, mirror_y={cab.mirror_y}. "
                "MVP/monitor-bench requires rotation=0 and no mirror."
            )

        active_w, active_h = cab.active_size_mm  # [width_mm, height_mm]

        # Decompose charuco_id into (row, col) within the inner-corner grid.
        # ChArUco numbers corners left-to-right, top-to-bottom.
        r, c = divmod(charuco_id, inner)

        # Physical ChArUco spacing: board has (inner+1) squares per side.
        #   squareLength = active_w / (inner + 1)
        # Corner (r, c) is at position (c+1)*squareLength from the left edge,
        # (r+1)*squareLength from the top edge.  Subtract half active size to
        # move from edge-origin to center-origin.
        x = active_w * ((c + 1) / (inner + 1) - 0.5)
        y = active_h * ((r + 1) / (inner + 1) - 0.5)

        return np.array([x, y, 0.0], dtype=float)

    # ------------------------------------------------------------------
    # Preflight check
    # ------------------------------------------------------------------

    def preflight(
        self,
        actual_pattern_hash: str,
        image_size: tuple[int, int] | None = None,
    ) -> None:
        """
        Validate config and pattern hash before running reconstruct.

        Parameters
        ----------
        actual_pattern_hash : str
            Hash of the pattern image/PDF that was actually captured.  Must
            match self.expected_pattern_hash exactly (SHA-256 hex recommended).
        image_size : (width_px, height_px) | None
            Optional: if provided, must match at least one cabinet's
            resolution_px.  Best-effort check — a mismatch suggests the camera
            feed was cropped or the wrong cabinet config was loaded.

        Raises
        ------
        ScreenMappingError
            On any of:
            - hash mismatch
            - image_size given and does not match any cabinet resolution_px

        Note
        ----
        active_origin != "center" and rotation not in {0,90,180,270} are
        rejected at model construction time (by the Literal type and
        model_post_init respectively), so a constructed ScreenMapping is
        already guaranteed valid on those fields — preflight does not re-check.
        """
        # 1. Pattern hash check
        if actual_pattern_hash != self.expected_pattern_hash:
            raise ScreenMappingError(
                f"Pattern hash mismatch: expected '{self.expected_pattern_hash}', "
                f"got '{actual_pattern_hash}'. "
                "Re-generate or re-import the expected hash."
            )

        # 2. Optional image-size cross-check
        if image_size is not None:
            w, h = image_size
            for cab in self.cabinets:
                cab_w, cab_h = cab.resolution_px
                if cab_w == w and cab_h == h:
                    return  # matched at least one cabinet
            resolutions = [tuple(cab.resolution_px) for cab in self.cabinets]
            raise ScreenMappingError(
                f"image_size {image_size} does not match any cabinet resolution_px. "
                f"Known resolutions: {resolutions}"
            )
