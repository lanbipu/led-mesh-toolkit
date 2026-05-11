use crate::project::ScreenConfig;

pub mod html;
pub mod pdf;

/// Data needed to render an instruction card (PDF + HTML share this).
#[derive(Debug, Clone)]
pub struct InstructionCard {
    pub project_name: String,
    pub screen_id: String,
    pub cfg: ScreenConfig,
    pub origin_grid_name: String,
    pub x_axis_grid_name: String,
    pub xy_plane_grid_name: String,
}
