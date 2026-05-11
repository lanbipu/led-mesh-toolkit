import { invoke } from "@tauri-apps/api/core";

export interface RecentProject {
  id: number;
  abs_path: string;
  display_name: string;
  last_opened_at: string;
}

export interface ProjectMeta {
  name: string;
  unit: string;
}
export type ShapePriorConfig =
  | { type: "flat" }
  | { type: "curved"; radius_mm: number; fold_seams_at_columns: number[] }
  | { type: "folded"; fold_seams_at_columns: number[] };
export interface BottomCompletionConfig {
  lowest_measurable_row: number;
  fallback_method: string;
  assumed_height_mm: number;
}
export interface ScreenConfig {
  cabinet_count: [number, number];
  cabinet_size_mm: [number, number];
  pixels_per_cabinet?: [number, number];
  shape_prior: ShapePriorConfig;
  shape_mode: "rectangle" | "irregular";
  irregular_mask: [number, number][];
  bottom_completion?: BottomCompletionConfig;
}
export interface CoordinateSystemConfig {
  origin_point: string;
  x_axis_point: string;
  xy_plane_point: string;
}
export interface OutputConfig {
  target: string;
  obj_filename: string;
  weld_vertices_tolerance_mm: number;
  triangulate: boolean;
}
export interface ProjectConfig {
  project: ProjectMeta;
  screens: Record<string, ScreenConfig>;
  coordinate_system: CoordinateSystemConfig;
  output: OutputConfig;
}

export interface ReconstructedSurface {
  screen_id: string;
  topology: { cols: number; rows: number };
  vertices: [number, number, number][];
  uv_coords: [number, number][];
  quality_metrics: QualityMetrics;
}
export interface QualityMetrics {
  method: string;
  middle_max_dev_mm: number;
  middle_mean_dev_mm: number;
  shape_fit_rms_mm: number;
  measured_count: number;
  expected_count: number;
  missing: string[];
  outliers: string[];
  estimated_rms_mm: number;
  estimated_p95_mm: number;
  warnings: string[];
}

export interface ReconstructionResult {
  run_id: number;
  surface: ReconstructedSurface;
  report_json_path: string;
}

export interface ReconstructionRun {
  id: number;
  screen_id: string;
  method: string;
  estimated_rms_mm: number;
  vertex_count: number;
  target: string | null;
  output_obj_path: string | null;
  created_at: string;
}

export interface MeasuredPoints {
  // 简化映射；实际字段以 lmt-core::measured_points::MeasuredPoints 为准
  points: Array<{
    name: string;
    position: [number, number, number];
    uncertainty: { isotropic: number } | { covariance: number[][] };
    source: "total_station" | { visual_ba: { camera_count: number } };
  }>;
  screen_id: string;
}

export type LmtError = { kind: string; message: string };

export const tauriApi = {
  listRecentProjects: () => invoke<RecentProject[]>("list_recent_projects"),
  addRecentProject: (absPath: string, displayName: string) =>
    invoke<RecentProject>("add_recent_project", { absPath, displayName }),
  removeRecentProject: (id: number) => invoke<void>("remove_recent_project", { id }),
  seedExampleProject: (targetDir: string, example: string) =>
    invoke<string>("seed_example_project", { targetDir, example }),
  loadProjectYaml: (absPath: string) => invoke<ProjectConfig>("load_project_yaml", { absPath }),
  saveProjectYaml: (absPath: string, config: ProjectConfig) =>
    invoke<void>("save_project_yaml", { absPath, config }),
  loadMeasurementsYaml: (path: string) =>
    invoke<MeasuredPoints>("load_measurements_yaml", { path }),
  reconstructSurface: (projectPath: string, screenId: string, measurementsPath: string) =>
    invoke<ReconstructionResult>("reconstruct_surface", {
      projectPath,
      screenId,
      measurementsPath,
    }),
  exportObj: (runId: number, target: string) =>
    invoke<string>("export_obj", { runId, target }),
  listRuns: (projectPath: string, screenId?: string) =>
    invoke<ReconstructionRun[]>("list_runs", { projectPath, screenId }),
  getRunReport: (runId: number) => invoke<unknown>("get_run_report", { runId }),
};
