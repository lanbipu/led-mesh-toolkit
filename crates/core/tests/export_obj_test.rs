use lmt_core::export::obj::write_obj;
use lmt_core::surface::{MeshOutput, TargetSoftware};
use nalgebra::{Vector2, Vector3};
use tempfile::tempdir;

#[test]
fn obj_contains_vertices_uvs_and_faces() {
    let mo = MeshOutput {
        target: TargetSoftware::Neutral,
        vertices: vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ],
        triangles: vec![[0, 1, 2]],
        uv_coords: vec![
            Vector2::new(0.0, 0.0),
            Vector2::new(1.0, 0.0),
            Vector2::new(0.0, 1.0),
        ],
    };

    let dir = tempdir().unwrap();
    let path = dir.path().join("test.obj");
    write_obj(&mo, &path).unwrap();

    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("v 0 0 0"));
    assert!(contents.contains("v 1 0 0"));
    assert!(contents.contains("vt 0 0"));
    assert!(contents.contains("vt 1 0"));
    // 1-based indices; with UVs both vertex and UV indices are paired.
    assert!(contents.contains("f 1/1 2/2 3/3"));
}

#[test]
fn obj_header_mentions_target() {
    let mo = MeshOutput {
        target: TargetSoftware::Disguise,
        vertices: vec![Vector3::zeros()],
        triangles: vec![],
        uv_coords: vec![Vector2::zeros()],
    };

    let dir = tempdir().unwrap();
    let path = dir.path().join("d.obj");
    write_obj(&mo, &path).unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.to_lowercase().contains("disguise"));
}

#[test]
fn write_obj_rejects_uv_length_mismatch() {
    let mo = MeshOutput {
        target: TargetSoftware::Neutral,
        vertices: vec![Vector3::zeros(); 3],
        uv_coords: vec![Vector2::zeros(); 2], // mismatch
        triangles: vec![[0, 1, 2]],
    };
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.obj");
    let result = write_obj(&mo, &path);
    assert!(result.is_err());
    // File should NOT have been created (validation runs before File::create)
    assert!(!path.exists(), "validation must reject before file creation");
}

#[test]
fn write_obj_rejects_oob_triangle_index() {
    let mo = MeshOutput {
        target: TargetSoftware::Neutral,
        vertices: vec![Vector3::zeros(); 3],
        uv_coords: vec![Vector2::zeros(); 3],
        triangles: vec![[0, 1, 99]], // index 99 OOB
    };
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.obj");
    let result = write_obj(&mo, &path);
    assert!(result.is_err());
    assert!(!path.exists());
}

#[test]
fn write_obj_rejects_non_finite_vertex() {
    let mo = MeshOutput {
        target: TargetSoftware::Neutral,
        vertices: vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(f64::NAN, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ],
        uv_coords: vec![Vector2::zeros(); 3],
        triangles: vec![[0, 1, 2]],
    };
    let dir = tempdir().unwrap();
    let path = dir.path().join("bad.obj");
    let result = write_obj(&mo, &path);
    assert!(result.is_err());
}
