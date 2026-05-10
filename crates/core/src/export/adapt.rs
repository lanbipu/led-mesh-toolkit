use nalgebra::Vector3;

use crate::surface::TargetSoftware;

/// Adapt a model-frame vertex to the target software's coordinate
/// system + units.
///
/// Model frame: right-handed, +Z up, meters.
/// - Disguise: right-handed, +Y up, meters.   (x, y, z) → (x, z, -y)
/// - Unreal:   left-handed,  +Z up, cm.       (x, y, z) → (100x, -100y, 100z)
/// - Neutral:  identity (debugging).
///
/// **Important**: this function only transforms vertex coordinates. If
/// `target_reverses_handedness(target)` is true (Unreal), callers MUST
/// also reverse the winding of every triangle (e.g. swap indices `[a,b,c]`
/// → `[a,c,b]`) — otherwise face normals will be flipped and the imported
/// mesh will exhibit inverted backface culling / lighting.
pub fn adapt_to_target(p: &Vector3<f64>, target: TargetSoftware) -> Vector3<f64> {
    match target {
        TargetSoftware::Neutral => *p,
        TargetSoftware::Disguise => Vector3::new(p.x, p.z, -p.y),
        TargetSoftware::Unreal => Vector3::new(p.x * 100.0, -p.y * 100.0, p.z * 100.0),
    }
}

/// Returns `true` if the coordinate transform for `target` is
/// handedness-reversing (negative determinant). When this is true,
/// triangle winding must be reversed by callers building a mesh.
///
/// - Disguise: handedness-preserving (right-hand → right-hand).
/// - Unreal: handedness-reversing (right-hand → left-hand via Y negation).
/// - Neutral: identity.
pub fn target_reverses_handedness(target: TargetSoftware) -> bool {
    match target {
        TargetSoftware::Neutral => false,
        TargetSoftware::Disguise => false,
        TargetSoftware::Unreal => true,
    }
}
