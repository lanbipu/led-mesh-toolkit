# IR Freeze Notice (M0.1 → M1/M2 handoff)

After M0.1 completes, the public API of `crates/core` is **frozen**:

**IR types**

- `MeasuredPoint`, `MeasuredPoints`
- `Uncertainty::{Isotropic, Covariance3x3}` (YAML key: `isotropic` / `covariance`)
- `PointSource::{TotalStation, VisualBA}`

**Coordinate / shape**

- `CoordinateFrame::{from_three_points, world_to_model, model_to_world}` (validated Deserialize)
- `CabinetArray::{rectangle, irregular, is_present, total_size_mm}` (deserialize bounded by MAX_GRID_DIM, rejects zero dims and non-positive sizes)
- `ShapePrior::{Flat, Curved, Folded}`

**Reconstruction**

- `Reconstructor` trait
- `auto_reconstruct` dispatcher (direct_link → radial_basis → boundary_interp → nominal)
- Concrete reconstructors: `DirectLinkReconstructor`, `BoundaryInterpReconstructor`,
  `RadialBasisReconstructor` (≥5 anchors + 4 corners + ≥1 interior, dedupe + bounds),
  `NominalReconstructor` (Flat prior only)

**Surface + UV**

- `ReconstructedSurface`, `GridTopology` (MAX_GRID_DIM bound), `QualityMetrics`,
  `MeshOutput`, `TargetSoftware`
- `compute_grid_uv`
- `MAX_GRID_DIM` constant

**Geometry processing**

- `weld_vertices` (model frame, meters; panics on bad tolerance)
- `triangulate_grid(topology, vertices, cabinet_array)` — picks shorter diagonal,
  skips absent cells; panics on dim mismatch / short buffer

**Export**

- `surface_to_mesh_output(surface, cabinet_array, target, weld_tolerance_m) -> Result<MeshOutput, _>`
  (preflight validation, 200k early-reject for Disguise, winding reversal for Unreal)
- `write_obj(mesh, path)` (atomic temp + rename, validates before opening)
- `OutputTarget` trait + `DisguiseTarget`, `UnrealTarget`, `NeutralTarget` impls
- `DISGUISE_VERTEX_LIMIT` constant
- `adapt_to_target` + `target_reverses_handedness` helpers

---

M1 and M2 sessions consume this API to produce `MeasuredPoints`. Any
breaking change requires a coordinated PR (touched by both sessions).
Internal implementation is free to evolve.

**Known M0.1 limitations** (deferred to later milestones):

- `NominalReconstructor` only handles `ShapePrior::Flat`. Curved / folded
  screens with only 4 corners are unreachable; needs shape-aware nominal
  generator.
- `BoundaryInterpReconstructor` is effectively unreachable in `auto_reconstruct`
  (RBF preferred when corners are present); still useful as a manual API call
  with interior-residual validation.
- `surface_to_mesh_output` does *not* prune isolated vertices left orphaned
  by absent-cell skipping; resulting OBJ may contain unreferenced vertices.
- DoS risk: malformed YAML with huge `vertices` arrays consumes memory before
  validation triggers. Acceptable for internal trust boundary; revisit if
  external untrusted YAML becomes a use case.
- `parse_anchors` in radial_basis dedupes by `(col, row)` keeping the first
  occurrence — silent on duplicates beyond the first. Document if unexpected.
