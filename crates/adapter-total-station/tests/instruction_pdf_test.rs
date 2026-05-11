use lmt_adapter_total_station::instruction_card::pdf::generate_pdf;
use lmt_adapter_total_station::instruction_card::InstructionCard;
use lmt_adapter_total_station::project::{ScreenConfig, ShapePriorConfig};
use tempfile::tempdir;

#[test]
fn pdf_writes_a_nonempty_file_starting_with_pdf_magic() {
    let cfg = ScreenConfig {
        cabinet_count: [4, 2],
        cabinet_size_mm: [500.0, 500.0],
        shape_prior: ShapePriorConfig::Flat,
        bottom_completion: None,
        absent_cells: vec![],
    };
    let card = InstructionCard {
        project_name: "Studio_A".into(),
        screen_id: "MAIN".into(),
        cfg,
        origin_grid_name: "MAIN_V001_R001".into(),
        x_axis_grid_name: "MAIN_V005_R001".into(),
        xy_plane_grid_name: "MAIN_V001_R003".into(),
    };
    let dir = tempdir().unwrap();
    let path = dir.path().join("card.pdf");
    generate_pdf(&card, &path).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.len() > 1000, "PDF too small ({} bytes)", bytes.len());
    assert!(bytes.starts_with(b"%PDF-"), "missing PDF magic header");
}
