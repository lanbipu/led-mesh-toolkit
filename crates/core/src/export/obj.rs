use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::error::CoreError;
use crate::surface::{MeshOutput, TargetSoftware};

fn target_label(t: TargetSoftware) -> &'static str {
    match t {
        TargetSoftware::Disguise => "disguise (right-hand, +Y up, m)",
        TargetSoftware::Unreal => "unreal (left-hand, +Z up, cm)",
        TargetSoftware::Neutral => "neutral (right-hand, +Z up, m)",
    }
}

/// Serialize a `MeshOutput` to a Wavefront OBJ file.
///
/// Validates `mesh` before opening the file — invalid mesh data
/// returns `CoreError::InvalidInput` without touching the destination.
///
/// Format:
/// - 1-based indices
/// - Vertex / UV pairs in `f` lines
/// - No normals (renderers compute them, OBJ allows omitting)
/// - Single mesh group
pub fn write_obj(mesh: &MeshOutput, path: &Path) -> Result<(), CoreError> {
    // Validate before opening — don't corrupt an existing valid file
    // when the caller hands us malformed data.
    mesh.validate()?;

    let file = File::create(path)?;
    let mut w = BufWriter::new(file);

    writeln!(w, "# LED Mesh Toolkit OBJ export")?;
    writeln!(w, "# Target: {}", target_label(mesh.target))?;
    writeln!(w, "# Vertices: {}", mesh.vertices.len())?;
    writeln!(w, "# Triangles: {}", mesh.triangles.len())?;
    writeln!(w)?;

    for v in &mesh.vertices {
        writeln!(w, "v {} {} {}", trim_zero(v.x), trim_zero(v.y), trim_zero(v.z))?;
    }
    for uv in &mesh.uv_coords {
        writeln!(w, "vt {} {}", trim_zero(uv.x), trim_zero(uv.y))?;
    }

    writeln!(w, "g screen_mesh")?;
    for t in &mesh.triangles {
        let a = t[0] + 1;
        let b = t[1] + 1;
        let c = t[2] + 1;
        writeln!(w, "f {a}/{a} {b}/{b} {c}/{c}")?;
    }

    w.flush()?;
    Ok(())
}

fn trim_zero(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" { "0".to_string() } else { s }
}
