"""Pydantic models for sidecar IPC. Mirrors python-sidecar/schema/ipc.schema.json."""
from __future__ import annotations

from typing import Annotated, Any, Literal, Union

from pydantic import BaseModel, Field, model_serializer, model_validator

# Vec3 / Mat3 enforce proper nesting + length so ragged / short arrays are
# rejected at the IPC boundary, not later inside BA / projection code.
Vec3 = Annotated[list[float], Field(min_length=3, max_length=3)]
Mat3 = Annotated[list[Vec3], Field(min_length=3, max_length=3)]


class CoordinateFrame(BaseModel):
    origin_world: Vec3
    basis: Mat3


PositiveSizePair = Annotated[
    list[Annotated[float, Field(gt=0.0)]],
    Field(min_length=2, max_length=2),
]


class CabinetArray(BaseModel):
    cols: int = Field(ge=1)
    rows: int = Field(ge=1)
    cabinet_size_mm: PositiveSizePair
    absent_cells: list[tuple[int, int]] = Field(default_factory=list)


class ShapePriorCurvedBody(BaseModel):
    radius_mm: float


class ShapePriorCurved(BaseModel):
    """`{"curved": {"radius_mm": ...}}`"""
    curved: ShapePriorCurvedBody


class ShapePriorFoldedBody(BaseModel):
    fold_seam_columns: list[Annotated[int, Field(ge=0)]]


class ShapePriorFolded(BaseModel):
    """`{"folded": {"fold_seam_columns": [...]}}`"""
    folded: ShapePriorFoldedBody


ShapePrior = Union[Literal["flat"], ShapePriorCurved, ShapePriorFolded]


class FrameAnchor(BaseModel):
    cabinet_col: int = Field(ge=0)
    cabinet_row: int = Field(ge=0)
    aruco_id: int = Field(ge=0)
    position_world: Vec3


PositiveIntPair = Annotated[
    list[Annotated[int, Field(gt=0)]],
    Field(min_length=2, max_length=2),
]


class Intrinsics(BaseModel):
    K: Mat3
    dist_coeffs: Annotated[list[float], Field(min_length=4, max_length=8)]
    image_size: PositiveIntPair


class PatternMetaCabinet(BaseModel):
    col: int
    row: int
    aruco_id_start: int
    aruco_id_end: int


class PatternMeta(BaseModel):
    aruco_dict: str
    markers_per_cabinet: int
    checkerboard_inner_corners: int
    cabinets: list[PatternMetaCabinet]


class ReconstructProject(BaseModel):
    screen_id: str
    coordinate_frame: CoordinateFrame
    cabinet_array: CabinetArray
    shape_prior: ShapePrior
    frame_strategy: Literal["nominal_anchoring", "three_points"]
    frame_anchors: list[FrameAnchor] | None = None

    @model_validator(mode="after")
    def _check_anchors_match_strategy(self) -> "ReconstructProject":
        if self.frame_strategy == "three_points":
            if self.frame_anchors is None or len(self.frame_anchors) != 3:
                raise ValueError(
                    "frame_strategy=three_points requires exactly 3 frame_anchors"
                )
        else:  # nominal_anchoring
            if self.frame_anchors is not None:
                raise ValueError(
                    "frame_strategy=nominal_anchoring forbids frame_anchors (must be null)"
                )
        return self


class ReconstructInput(BaseModel):
    command: Literal["reconstruct"]
    version: Literal[1]
    project: ReconstructProject
    images: Annotated[list[str], Field(min_length=1)]
    intrinsics: Intrinsics
    pattern_meta: PatternMeta


class CalibrateInput(BaseModel):
    command: Literal["calibrate"]
    version: Literal[1]
    checkerboard_images: Annotated[list[str], Field(min_length=5)]
    inner_corners: Annotated[list[int], Field(min_length=2, max_length=2)]
    square_size_mm: float = Field(gt=0.0)
    output_path: str


class GeneratePatternProject(BaseModel):
    screen_id: str
    cabinet_array: CabinetArray


class GeneratePatternInput(BaseModel):
    command: Literal["generate_pattern"]
    version: Literal[1]
    project: GeneratePatternProject
    output_dir: str
    screen_resolution: PositiveIntPair


class Uncertainty(BaseModel):
    """Externally tagged: exactly one of {isotropic, covariance} must be set.

    JSON form mirrors `lmt_core::uncertainty::Uncertainty`:
      {"isotropic": 0.005} or {"covariance": [[...], [...], [...]]}.
    """

    isotropic: float | None = None
    covariance: Mat3 | None = None

    @model_validator(mode="after")
    def _exactly_one(self) -> "Uncertainty":
        provided = sum(v is not None for v in (self.isotropic, self.covariance))
        if provided != 1:
            raise ValueError("Uncertainty must set exactly one of {isotropic, covariance}")
        return self

    @model_serializer
    def _serialize(self) -> dict[str, Any]:
        if self.isotropic is not None:
            return {"isotropic": self.isotropic}
        return {"covariance": self.covariance}


class PointSourceVisualBa(BaseModel):
    camera_count: int = Field(ge=1)


class PointSource(BaseModel):
    visual_ba: PointSourceVisualBa


class MeasuredPoint(BaseModel):
    name: str
    position: Vec3
    uncertainty: Uncertainty
    source: PointSource


class BaStats(BaseModel):
    rms_reprojection_px: float
    iterations: int
    converged: bool


class ResultData(BaseModel):
    measured_points: list[MeasuredPoint]
    ba_stats: BaStats
    frame_strategy_used: Literal["nominal_anchoring", "three_points"]


class ProgressEvent(BaseModel):
    event: Literal["progress"]
    stage: Literal[
        "load",
        "detect_charuco",
        "subpixel_refine",
        "bundle_adjustment",
        "procrustes_align",
        "output",
    ]
    percent: float = Field(ge=0.0, le=1.0)
    message: str | None = None


class WarningEvent(BaseModel):
    event: Literal["warning"]
    code: str
    message: str
    cabinet: str | None = None


class ResultEvent(BaseModel):
    event: Literal["result"]
    data: ResultData


class ErrorEvent(BaseModel):
    event: Literal["error"]
    code: Literal[
        "invalid_input",
        "image_load_failed",
        "detection_failed",
        "ba_diverged",
        "procrustes_failed",
        "intrinsics_invalid",
        "internal_error",
    ]
    message: str
    fatal: bool
