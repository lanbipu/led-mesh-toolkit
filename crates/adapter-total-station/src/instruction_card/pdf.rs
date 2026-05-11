use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use printpdf::{Mm, PdfDocument};

use crate::error::AdapterError;
use crate::instruction_card::InstructionCard;
use crate::shape_grid::expected_grid_positions;

/// Render an instruction card to PDF (A4 portrait).
///
/// Rows below `bottom_completion.lowest_measurable_row` are excluded
/// from the table — they're occluded in the field and will be
/// fabricated by the adapter's vertical-extension fallback.
pub fn generate_pdf(card: &InstructionCard, path: &Path) -> Result<(), AdapterError> {
    let grid = expected_grid_positions(&card.screen_id, &card.cfg)
        .map_err(|e| AdapterError::Pdf(format!("grid: {e}")))?;

    let lowest_measurable_row: u32 = card
        .cfg
        .bottom_completion
        .as_ref()
        .map(|bc| bc.lowest_measurable_row)
        .unwrap_or(1);

    let (doc, page1, layer1) = PdfDocument::new(
        format!("LMT — {}", card.project_name),
        Mm(210.0),
        Mm(297.0),
        "Layer 1",
    );
    let layer = doc.get_page(page1).get_layer(layer1);
    let font = doc
        .add_builtin_font(printpdf::BuiltinFont::Helvetica)
        .map_err(|e| AdapterError::Pdf(e.to_string()))?;
    let bold = doc
        .add_builtin_font(printpdf::BuiltinFont::HelveticaBold)
        .map_err(|e| AdapterError::Pdf(e.to_string()))?;

    layer.use_text(
        format!("LED Instruction Card — {}", card.project_name),
        14.0,
        Mm(20.0),
        Mm(280.0),
        &bold,
    );
    layer.use_text(
        format!(
            "Screen: {}    Cabinets: {}x{}    Cabinet size: {} x {} mm    Total points: {}",
            card.screen_id,
            card.cfg.cabinet_count[0],
            card.cfg.cabinet_count[1],
            card.cfg.cabinet_size_mm[0],
            card.cfg.cabinet_size_mm[1],
            grid.len()
        ),
        9.0,
        Mm(20.0),
        Mm(272.0),
        &font,
    );

    layer.use_text(
        "Reference points (instrument ids 1, 2, 3 — measure in order):",
        11.0,
        Mm(20.0),
        Mm(258.0),
        &bold,
    );
    layer.use_text(
        format!("  1) Origin     -> {}", card.origin_grid_name),
        9.0,
        Mm(20.0),
        Mm(250.0),
        &font,
    );
    layer.use_text(
        format!("  2) X-axis     -> {}", card.x_axis_grid_name),
        9.0,
        Mm(20.0),
        Mm(244.0),
        &font,
    );
    layer.use_text(
        format!("  3) XY-plane   -> {}", card.xy_plane_grid_name),
        9.0,
        Mm(20.0),
        Mm(238.0),
        &font,
    );

    if lowest_measurable_row > 1 {
        layer.use_text(
            format!(
                "Note: rows R001..R{:03} are occluded — adapter fabricates via vertical fallback (±5-15mm).",
                lowest_measurable_row - 1
            ),
            8.5,
            Mm(20.0),
            Mm(232.0),
            &font,
        );
    }

    layer.use_text(
        "Grid points to measure (instrument 4+ in any order):",
        11.0,
        Mm(20.0),
        Mm(225.0),
        &bold,
    );
    layer.use_text(
        "Name                 X(m)      Y(m)      Z(m)",
        9.0,
        Mm(20.0),
        Mm(218.0),
        &font,
    );

    let ref_names = [
        card.origin_grid_name.as_str(),
        card.x_axis_grid_name.as_str(),
        card.xy_plane_grid_name.as_str(),
    ];
    let mut y: f32 = 212.0;
    let line_height: f32 = 4.5;
    let bottom_margin: f32 = 25.0;

    let mut current_layer = layer;
    for ge in &grid {
        if ref_names.contains(&ge.name.as_str()) {
            continue;
        }
        if ge.row_zero_based + 1 < lowest_measurable_row {
            continue;
        }
        if y < bottom_margin {
            let (new_page, new_layer) = doc.add_page(Mm(210.0), Mm(297.0), "Layer cont");
            current_layer = doc.get_page(new_page).get_layer(new_layer);
            y = 280.0;
        }
        current_layer.use_text(
            format!(
                "{:20} {:8.3}  {:8.3}  {:8.3}",
                ge.name, ge.model_position.x, ge.model_position.y, ge.model_position.z
            ),
            8.5,
            Mm(20.0),
            Mm(y),
            &font,
        );
        y -= line_height;
    }

    let file = File::create(path)?;
    let mut buf = BufWriter::new(file);
    doc.save(&mut buf)
        .map_err(|e| AdapterError::Pdf(e.to_string()))?;
    // Explicit flush so callers don't see Ok(()) while the final buffered
    // bytes are still pending — important on full-disk or interrupted writes.
    buf.flush()
        .map_err(|e| AdapterError::Pdf(format!("flush: {e}")))?;

    Ok(())
}
