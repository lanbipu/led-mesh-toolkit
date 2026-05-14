//! GUI `dto::ProjectConfig` → `lmt_adapter_total_station::ProjectConfig` 字段映射。
//!
//! 两边各自的 schema 独立演进（GUI 偏面向 UI，adapter 偏面向算法）。
//! 这个模块是唯一的桥。

use lmt_adapter_total_station::project as m1;

use crate::dto;
use crate::error::{LmtError, LmtResult};

pub fn map_to_adapter(cfg: &dto::ProjectConfig) -> LmtResult<m1::ProjectConfig> {
    use std::collections::HashMap;

    let mut screens: HashMap<String, m1::ScreenConfig> = HashMap::new();
    for (id, s) in &cfg.screens {
        screens.insert(id.clone(), map_screen(s)?);
    }

    let m1_cfg = m1::ProjectConfig {
        project: m1::ProjectMeta { name: cfg.project.name.clone() },
        screens,
        coordinate_system: m1::CoordinateSystemConfig {
            origin_grid_name: cfg.coordinate_system.origin_point.clone(),
            x_axis_grid_name: cfg.coordinate_system.x_axis_point.clone(),
            xy_plane_grid_name: cfg.coordinate_system.xy_plane_point.clone(),
        },
    };

    m1_cfg.validate().map_err(LmtError::from)?;
    Ok(m1_cfg)
}

fn map_screen(s: &dto::ScreenConfig) -> LmtResult<m1::ScreenConfig> {
    let shape_prior = match &s.shape_prior {
        dto::ShapePriorConfig::Flat => m1::ShapePriorConfig::Flat,
        dto::ShapePriorConfig::Curved { radius_mm, fold_seams_at_columns } => {
            if fold_seams_at_columns.is_empty() {
                m1::ShapePriorConfig::Curved { radius_mm: *radius_mm }
            } else {
                return Err(LmtError::InvalidInput(
                    "shape_prior Curved with non-empty fold_seams_at_columns is not supported \
                     by M1 adapter (radius would be lost); pick pure Curved (drop seams) or \
                     switch to Folded".to_string(),
                ));
            }
        }
        dto::ShapePriorConfig::Folded { fold_seams_at_columns } => {
            m1::ShapePriorConfig::Folded {
                fold_seam_columns: fold_seams_at_columns.clone(),
            }
        }
    };

    let bottom_completion = s.bottom_completion.as_ref().map(|bc| m1::BottomCompletion {
        lowest_measurable_row: bc.lowest_measurable_row,
        fallback_method: m1::FallbackMethod::Vertical,
    });

    let absent_cells = s
        .irregular_mask
        .iter()
        .map(|c| (c[0], c[1]))
        .collect::<Vec<_>>();

    Ok(m1::ScreenConfig {
        cabinet_count: s.cabinet_count,
        cabinet_size_mm: s.cabinet_size_mm,
        shape_prior,
        bottom_completion,
        absent_cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn flat_screen() -> dto::ScreenConfig {
        dto::ScreenConfig {
            cabinet_count: [4, 2],
            cabinet_size_mm: [500.0, 500.0],
            pixels_per_cabinet: Some([256, 256]),
            shape_prior: dto::ShapePriorConfig::Flat,
            shape_mode: dto::ShapeMode::Rectangle,
            irregular_mask: vec![],
            bottom_completion: None,
        }
    }

    fn base_cfg(screen: dto::ScreenConfig) -> dto::ProjectConfig {
        let mut screens = BTreeMap::new();
        screens.insert("MAIN".into(), screen);
        dto::ProjectConfig {
            project: dto::ProjectMeta { name: "T".into(), unit: "mm".into() },
            screens,
            coordinate_system: dto::CoordinateSystemConfig {
                origin_point: "MAIN_V001_R001".into(),
                x_axis_point: "MAIN_V005_R001".into(),
                xy_plane_point: "MAIN_V001_R003".into(),
            },
            output: dto::OutputConfig {
                target: "disguise".into(),
                obj_filename: "{screen_id}.obj".into(),
                weld_vertices_tolerance_mm: 1.0,
                triangulate: true,
            },
        }
    }

    #[test]
    fn flat_screen_maps_minimal_fields() {
        let cfg = base_cfg(flat_screen());
        let m = map_to_adapter(&cfg).unwrap();

        assert_eq!(m.project.name, "T");
        assert_eq!(m.screens.len(), 1);
        let s = m.screens.get("MAIN").unwrap();
        assert_eq!(s.cabinet_count, [4, 2]);
        assert_eq!(s.cabinet_size_mm, [500.0, 500.0]);
        assert!(matches!(s.shape_prior, m1::ShapePriorConfig::Flat));
        assert!(s.absent_cells.is_empty());

        assert_eq!(m.coordinate_system.origin_grid_name, "MAIN_V001_R001");
        assert_eq!(m.coordinate_system.x_axis_grid_name, "MAIN_V005_R001");
        assert_eq!(m.coordinate_system.xy_plane_grid_name, "MAIN_V001_R003");
    }

    #[test]
    fn irregular_mask_to_absent_cells() {
        let mut s = flat_screen();
        s.shape_mode = dto::ShapeMode::Irregular;
        s.irregular_mask = vec![[0, 0], [3, 1]];
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        let cells = &m.screens.get("MAIN").unwrap().absent_cells;
        assert_eq!(cells, &vec![(0u32, 0u32), (3u32, 1u32)]);
    }

    #[test]
    fn curved_without_folds_maps_to_curved() {
        let mut s = flat_screen();
        s.shape_prior = dto::ShapePriorConfig::Curved {
            radius_mm: 6000.0,
            fold_seams_at_columns: vec![],
        };
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        match &m.screens.get("MAIN").unwrap().shape_prior {
            m1::ShapePriorConfig::Curved { radius_mm } => assert_eq!(*radius_mm, 6000.0),
            other => panic!("expected Curved, got {other:?}"),
        }
    }

    #[test]
    fn folded_renames_seam_field() {
        let mut s = flat_screen();
        s.shape_prior = dto::ShapePriorConfig::Folded { fold_seams_at_columns: vec![2, 4] };
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        match &m.screens.get("MAIN").unwrap().shape_prior {
            m1::ShapePriorConfig::Folded { fold_seam_columns } => {
                assert_eq!(fold_seam_columns, &vec![2u32, 4u32]);
            }
            other => panic!("expected Folded, got {other:?}"),
        }
    }

    #[test]
    fn curved_with_folds_returns_error() {
        // Curved + 非空 fold_seams 在 M1 那边没有保留 radius 的表达；
        // 与其静默丢 radius 升级成 Folded，不如让用户显式选 shape_prior。
        let mut s = flat_screen();
        s.shape_prior = dto::ShapePriorConfig::Curved {
            radius_mm: 6000.0,
            fold_seams_at_columns: vec![3],
        };
        let cfg = base_cfg(s);
        let err = map_to_adapter(&cfg).unwrap_err();
        let msg = format!("{err}").to_lowercase();
        assert!(msg.contains("curved") && msg.contains("fold"), "got: {err}");
    }

    #[test]
    fn validate_propagates() {
        let mut cfg = base_cfg(flat_screen());
        cfg.screens.clear();
        let err = map_to_adapter(&cfg).unwrap_err();
        assert!(format!("{err}").to_lowercase().contains("no screens"));
    }

    #[test]
    fn bottom_completion_passes_through() {
        let mut s = flat_screen();
        s.bottom_completion = Some(dto::BottomCompletionConfig {
            lowest_measurable_row: 2,
            fallback_method: "vertical".into(),
            assumed_height_mm: 500.0,
        });
        let cfg = base_cfg(s);
        let m = map_to_adapter(&cfg).unwrap();
        let bc = m
            .screens
            .get("MAIN")
            .unwrap()
            .bottom_completion
            .as_ref()
            .unwrap();
        assert_eq!(bc.lowest_measurable_row, 2);
    }
}
