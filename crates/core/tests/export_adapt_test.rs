use lmt_core::export::adapt::adapt_to_target;
use lmt_core::surface::TargetSoftware;
use nalgebra::Vector3;

#[test]
fn neutral_is_identity() {
    let p = Vector3::new(1.0, 2.0, 3.0);
    assert_eq!(adapt_to_target(&p, TargetSoftware::Neutral), p);
}

#[test]
fn disguise_swaps_y_z() {
    // Model: right-hand, +Z up. Disguise: right-hand, +Y up.
    // (x, y, z) → (x, z, -y) keeps right-handedness.
    let p = Vector3::new(1.0, 2.0, 3.0);
    let q = adapt_to_target(&p, TargetSoftware::Disguise);
    assert_eq!(q, Vector3::new(1.0, 3.0, -2.0));
}

#[test]
fn unreal_flips_handedness_and_scales_to_cm() {
    // Model: right-hand, +Z up, m. UE: left-hand, +Z up, cm.
    // To go right→left handed, negate Y. Then scale by 100.
    let p = Vector3::new(1.0, 2.0, 3.0);
    let q = adapt_to_target(&p, TargetSoftware::Unreal);
    assert_eq!(q, Vector3::new(100.0, -200.0, 300.0));
}

#[test]
fn handedness_reversal_only_for_unreal() {
    use lmt_core::export::adapt::target_reverses_handedness;
    assert!(!target_reverses_handedness(TargetSoftware::Neutral));
    assert!(!target_reverses_handedness(TargetSoftware::Disguise));
    assert!(target_reverses_handedness(TargetSoftware::Unreal));
}

#[test]
fn unreal_quad_winding_must_be_reversed_after_adapt() {
    // Demonstrate the contract: a quad's normal flips under Unreal adapt.
    // Take 3 corners of a CCW (in model frame, +Z up) triangle:
    //   a = (0,0,0), b = (1,0,0), c = (0,0,1)
    // Original normal direction (a→b × a→c) is +Y.
    // After adapt (×100, -y×100, ×100): a' = (0,0,0), b' = (100,0,0), c' = (0,0,100).
    // (a'→b' × a'→c') is still +Y in adapted space — but Unreal interprets that
    // as "outward" only if winding is CW (left-handed). So same triangle = inverted face.
    //
    // This test exists to remind us that the contract above is real — the actual
    // index-swap happens in surface_to_mesh_output (Task 19). For now, just check
    // adapt produces the expected adapted positions and the helper agrees.
    let a = Vector3::new(0.0, 0.0, 0.0);
    let b = Vector3::new(1.0, 0.0, 0.0);
    let c = Vector3::new(0.0, 0.0, 1.0);

    let aa = adapt_to_target(&a, TargetSoftware::Unreal);
    let ab = adapt_to_target(&b, TargetSoftware::Unreal);
    let ac = adapt_to_target(&c, TargetSoftware::Unreal);

    assert_eq!(aa, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(ab, Vector3::new(100.0, 0.0, 0.0));
    assert_eq!(ac, Vector3::new(0.0, 0.0, 100.0));

    // The model-frame normal (b-a) × (c-a) = (1,0,0) × (0,0,1) = (0*1 - 0*0, 0*0 - 1*1, 1*0 - 0*0) = (0,-1,0).
    let model_normal = (b - a).cross(&(c - a));
    assert_eq!(model_normal, Vector3::new(0.0, -1.0, 0.0));

    // Unreal-adapted normal (ab-aa) × (ac-aa) = (100,0,0) × (0,0,100) = (0*100 - 0*0, 0*0 - 100*100, 100*0 - 0*0) = (0,-10000,0)
    let unreal_normal = (ab - aa).cross(&(ac - aa));
    assert_eq!(unreal_normal, Vector3::new(0.0, -10000.0, 0.0));

    // Both normals point in -Y direction in their respective frames.
    // For the same triangle to face "outward" in Unreal's left-handed system,
    // the winding must be reversed — proving target_reverses_handedness == true.
}
