//! lmt-cli E2E 测试 —— 直接 spawn `lmt` binary,模拟 agent 调用。
//!
//! 覆盖维度:
//! - `--version` / `--help` 出口
//! - `schema` 子命令 stdout 是合法 JSON envelope
//! - `total-station import` 完整路径:happy / dry-run / refuse-no-yes / cross-screen 冲突
//! - `project save` + `load` round-trip
//! - `reconstruct surface` → `list-runs` → `get-run-report` → `export obj` 全链路
//! - `--json` 模式下 stderr 只含一条 envelope(`reconstruct` 算法路径有 tracing,
//!   是 JSON 隔离测试的硬条件)
//! - `--timeout` 暴露为 unsupported(v0 未实现)
//! - 任意 destructive 命令在 `--dry-run` 下不创建 DB 文件

use assert_cmd::Command;
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;

/// 重用 workspace 内 `examples/curved-flat`,跟既有 src-tauri 集成测试一致。
fn examples_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// 复制一个 examples/<name> 到给定目录下。返回拷贝出的 project 根。
fn seed_project(into: &Path, example: &str) -> std::path::PathBuf {
    let src = examples_root().join(example);
    let dst = into.join(example);
    copy_dir(&src, &dst);
    dst
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn lmt() -> Command {
    Command::cargo_bin("lmt").expect("lmt binary should build")
}

// ── 版本 / schema ────────────────────────────────────────────────────────────

#[test]
fn version_prints_semver_line() {
    let out = lmt().arg("--version").assert().success().get_output().clone();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("lmt "), "stdout: {s}");
}

#[test]
fn schema_json_envelope_has_known_types() {
    let out = lmt().args(["--json", "schema"]).assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("stdout must be JSON envelope");
    assert_eq!(env["ok"], true);
    assert_eq!(env["meta"]["schema_version"], "1");
    let types = env["data"]["types"].as_object().expect("types map");
    for name in [
        "ProjectConfig",
        "TotalStationImportResult",
        "ReconstructionRun",
        "LmtError",
        "ApiError",
        "Envelope",
        "ErrorEnvelope",
    ] {
        assert!(types.contains_key(name), "missing schema for {name}: keys={:?}", types.keys().collect::<Vec<_>>());
    }
}

// ── version subcommand ────────────────────────────────────────────────────────

#[test]
fn version_subcommand_json_has_version_and_schema() {
    let out = lmt().args(["--json", "version"]).assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("JSON envelope");
    assert_eq!(env["ok"], true);
    assert!(env["data"]["version"].as_str().unwrap().len() > 0);
    assert_eq!(env["data"]["schema_version"], "1");
    assert_eq!(env["data"]["contract_version"], "1.0");
}

// ── manifest ─────────────────────────────────────────────────────────────────

#[test]
fn manifest_json_lists_operations_with_ids() {
    let out = lmt().args(["--json", "manifest"]).assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("stdout must be JSON envelope");
    assert_eq!(env["ok"], true);
    let ops = env["data"]["operations"].as_array().expect("operations array");
    let ids: Vec<&str> = ops.iter().map(|o| o["operation_id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"reconstruct.surface"), "ids: {ids:?}");
    assert!(ids.contains(&"project.list_recent"), "ids: {ids:?}");
    assert_eq!(env["data"]["contract_version"], "1.0");
}

#[test]
fn manifest_human_mode_is_text_not_json() {
    let out = lmt().arg("manifest").assert().success().get_output().clone();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(serde_json::from_str::<Value>(&s).is_err(), "human mode should not be JSON: {s}");
    assert!(s.contains("reconstruct.surface"), "stdout: {s}");
}

// ── --timeout 未实现 ─────────────────────────────────────────────────────────

#[test]
fn timeout_flag_rejected_as_unsupported() {
    let assert = lmt().args(["--timeout", "5", "schema"]).assert().failure();
    let out = assert.get_output();
    // exit code 7 = UNSUPPORTED(见 lmt_shared::exit_codes)
    assert_eq!(out.status.code(), Some(7));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unsupported"), "stderr: {stderr}");
}

// ── total-station import: refuse / dry-run / execute / cross-screen 冲突 ────

#[test]
fn import_refuses_without_yes_or_dry_run() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    let csv = proj.join("measurements").join("raw.csv");

    let assert = lmt()
        .args([
            "total-station",
            "import",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .failure();
    // INVALID_INPUT = 2
    assert_eq!(assert.get_output().status.code(), Some(2));
}

#[test]
fn import_dry_run_does_not_write_files() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    let csv = proj.join("measurements").join("raw.csv");

    lmt()
        .args([
            "--dry-run",
            "total-station",
            "import",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .success();

    // 既有 measured.yaml 是 example 自带的,但 import_report.json 是 import 产物。
    // dry-run 必须不写 import_report.json(也不动 measured.yaml.bak 等)。
    assert!(
        !proj.join("measurements/import_report.json").exists(),
        "dry-run must not write import_report.json"
    );
    assert!(
        !proj.join("measurements/measured.yaml.bak").exists(),
        "dry-run must not create .bak"
    );
}

#[test]
fn import_dry_run_refuses_cross_screen_conflict() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    // 把 measured.yaml 改成另一个 screen 的数据,模拟"被另一个 screen 占用"。
    let stale = "screen_id: FLOOR\ncoordinate_frame:\n  origin_world: [0.0, 0.0, 0.0]\npoints: []\n";
    std::fs::write(proj.join("measurements/measured.yaml"), stale).unwrap();

    let csv = proj.join("measurements").join("raw.csv");
    let assert = lmt()
        .args([
            "--dry-run",
            "total-station",
            "import",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .failure();
    let out = assert.get_output();
    // INVALID_INPUT = 2(checkok 复用 run_import 的 cross-screen guard)
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("FLOOR"), "stderr: {stderr}");
}

#[test]
fn import_yes_writes_artifacts_and_envelope() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    let csv = proj.join("measurements").join("raw.csv");

    let assert = lmt()
        .args([
            "--yes",
            "--json",
            "total-station",
            "import",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .success();
    let env: Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert!(env["data"]["measuredCount"].as_u64().unwrap() >= 3);
    assert!(proj.join("measurements/import_report.json").is_file());
}

// ── --json stderr 隔离:错误时 stderr 单条 envelope,无 tracing 噪音 ───────

#[test]
fn json_error_stderr_contains_only_envelope() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");

    // 拿一个会失败的命令(unknown screen)。
    let assert = lmt()
        .args([
            "--json",
            "total-station",
            "instruction-card",
            proj.to_str().unwrap(),
            "BOGUS_SCREEN",
        ])
        .assert()
        .failure();
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap().trim_end();
    // 只有一行;且整行是合法 JSON envelope(ok=false)。
    assert!(
        !stderr.contains('\n'),
        "stderr must be a single line envelope; got:\n{stderr}"
    );
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false);
    assert!(env["error"]["code"].as_str().unwrap_or("").len() > 0);
}

// ── project save → load round-trip(覆盖 --input + write_safe) ────────────

#[test]
fn project_save_load_roundtrip_via_input_file() {
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path().join("new-project");
    let input = tmp.path().join("input.yaml");
    let yaml = r#"
project:
  name: RoundTrip
  unit: mm
screens:
  S1:
    cabinet_count: [2, 2]
    cabinet_size_mm: [500.0, 500.0]
    shape_prior:
      type: flat
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: S1_V001_R001
  x_axis_point: S1_V003_R001
  xy_plane_point: S1_V001_R003
output:
  target: neutral
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;
    std::fs::write(&input, yaml).unwrap();

    // save 是 destructive,必须 --yes。
    lmt()
        .args([
            "--yes",
            "project",
            "save",
            dst.to_str().unwrap(),
            "--input",
            input.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(dst.join("project.yaml").is_file());

    let load = lmt()
        .args(["--json", "project", "load", dst.to_str().unwrap()])
        .assert()
        .success();
    let env: Value = serde_json::from_slice(&load.get_output().stdout).unwrap();
    assert_eq!(env["data"]["project"]["name"], "RoundTrip");
}

// ── 全链路:import → reconstruct → list-runs → get-run-report → export ────

#[test]
fn full_pipeline_import_reconstruct_export() {
    let tmp = TempDir::new().unwrap();
    let proj = seed_project(tmp.path(), "curved-flat");
    let db = tmp.path().join("lmt.sqlite");
    let csv = proj.join("measurements").join("raw.csv");

    // 1) import
    lmt()
        .args([
            "--yes",
            "--db",
            db.to_str().unwrap(),
            "total-station",
            "import",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .success();

    // 2) reconstruct surface
    let reconstruct = lmt()
        .args([
            "--yes",
            "--json",
            "--db",
            db.to_str().unwrap(),
            "reconstruct",
            "surface",
            proj.to_str().unwrap(),
            "MAIN",
            "measurements/measured.yaml",
        ])
        .assert()
        .success();
    let env: Value = serde_json::from_slice(&reconstruct.get_output().stdout).unwrap();
    let run_id = env["data"]["run_id"].as_i64().expect("run_id");
    assert!(run_id > 0);

    // 3) list-runs(走 readonly DB,跟 reconstruct 同库)
    let list = lmt()
        .args([
            "--json",
            "--db",
            db.to_str().unwrap(),
            "reconstruct",
            "list-runs",
            proj.to_str().unwrap(),
        ])
        .assert()
        .success();
    let listed: Value = serde_json::from_slice(&list.get_output().stdout).unwrap();
    assert_eq!(listed["data"][0]["id"].as_i64(), Some(run_id));

    // 4) get-run-report(返回 report.json 原始 Value)
    let report = lmt()
        .args([
            "--json",
            "--db",
            db.to_str().unwrap(),
            "reconstruct",
            "get-run-report",
            &run_id.to_string(),
        ])
        .assert()
        .success();
    let rep_env: Value = serde_json::from_slice(&report.get_output().stdout).unwrap();
    assert_eq!(rep_env["data"]["screen_id"], "MAIN");

    // 5) export obj(destructive)
    let export = lmt()
        .args([
            "--yes",
            "--json",
            "--db",
            db.to_str().unwrap(),
            "export",
            "obj",
            &run_id.to_string(),
            "neutral",
        ])
        .assert()
        .success();
    let exp_env: Value = serde_json::from_slice(&export.get_output().stdout).unwrap();
    let written = exp_env["data"]["written"].as_str().expect("written path");
    assert!(Path::new(written).is_file(), "OBJ should exist at {written}");
}

// ── read-only / dry-run 不创建 default DB ─────────────────────────────────

#[test]
fn dry_run_remove_recent_does_not_create_db_file() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("nonexistent.sqlite");
    lmt()
        .args([
            "--db",
            db.to_str().unwrap(),
            "--dry-run",
            "project",
            "remove-recent",
            "99",
        ])
        .assert()
        .success();
    assert!(!db.exists(), "dry-run must not create the DB file");
}

#[test]
fn list_recent_against_missing_db_returns_empty_envelope() {
    let tmp = TempDir::new().unwrap();
    let db = tmp.path().join("nonexistent.sqlite");
    let out = lmt()
        .args([
            "--json",
            "--db",
            db.to_str().unwrap(),
            "project",
            "list-recent",
        ])
        .assert()
        .success();
    let env: Value = serde_json::from_slice(&out.get_output().stdout).unwrap();
    assert_eq!(env["ok"], true);
    assert!(
        env["data"].as_array().unwrap().is_empty(),
        "list-recent against missing DB must yield []"
    );
    assert!(!db.exists(), "list-recent must not create the DB file");
}

// ── parse error 走 envelope when --json ──────────────────────────────────────

#[test]
fn parse_error_with_json_yields_envelope_on_stderr() {
    let assert = lmt()
        .args(["--json", "project", "load"]) // 缺 required ABS_PATH
        .assert()
        .failure();
    // INVALID_INPUT = 2
    assert_eq!(assert.get_output().status.code(), Some(2));
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "invalid_input");
}

// ── scatter 模式 E2E ──────────────────────────────────────────────────────────

/// 散点 fixture 的 project.yaml（curved 55×15，radius 9523mm）。
fn write_scatter_project_yaml(dir: &Path) {
    let yaml = r#"project: { name: ScatterArc, unit: mm }
screens:
  MAIN:
    cabinet_count: [55, 15]
    cabinet_size_mm: [500, 500]
    pixels_per_cabinet: [256, 256]
    shape_prior: { type: curved, radius_mm: 9523 }
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V001_R001
  x_axis_point: MAIN_V055_R001
  xy_plane_point: MAIN_V001_R015
output:
  target: neutral
  obj_filename: "{screen_id}.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
"#;
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("project.yaml"), yaml).unwrap();
    std::fs::create_dir_all(dir.join("measurements")).unwrap();
}

/// 散点 CSV fixture 绝对路径（crates/lmt-cli/tests/fixtures/scatter_arc.csv）。
fn scatter_csv_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/scatter_arc.csv")
}

/// 随机噪声散点：点散在 20m 立方体内，inlier < 50% → surface_fit_failed。
fn write_noise_csv(path: &Path) {
    use std::fmt::Write as _;
    let mut s = String::new();
    for i in 0..80 {
        // 随机但确定性的坐标：完全不在圆柱面上
        let x = (i as f64 * 1234.567 + 333.0) % 20000.0 - 10000.0;
        let y = (i as f64 * 987.654 + 111.0) % 20000.0 - 10000.0;
        let z = (i as f64 * 543.21 + 55.0) % 15000.0;
        writeln!(s, "P{i},,{x:.3},{y:.3},{z:.3}").unwrap();
    }
    std::fs::write(path, s).unwrap();
}

/// 1. scatter import → reconstruct surface → list-runs → export obj（全链路 happy）
#[test]
fn scatter_import_reconstruct_export_happy() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("scatter-arc");
    write_scatter_project_yaml(&proj);
    let db = tmp.path().join("lmt.sqlite");
    let csv = scatter_csv_path();

    // import --mode scatter --columns x=3,y=4,z=5,label=1 --yes
    lmt()
        .args([
            "--yes",
            "--json",
            "--db",
            db.to_str().unwrap(),
            "total-station",
            "import",
            "--mode",
            "scatter",
            "--columns",
            "x=3,y=4,z=5,label=1",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(proj.join("measurements/measured.yaml").is_file(), "measured.yaml must exist");

    // reconstruct surface
    let reconstruct = lmt()
        .args([
            "--yes",
            "--json",
            "--db",
            db.to_str().unwrap(),
            "reconstruct",
            "surface",
            proj.to_str().unwrap(),
            "MAIN",
            "measurements/measured.yaml",
        ])
        .assert()
        .success();
    let rec_env: Value = serde_json::from_slice(&reconstruct.get_output().stdout).unwrap();
    let run_id = rec_env["data"]["run_id"].as_i64().expect("run_id in envelope");
    assert!(run_id > 0);

    // list-runs
    let list = lmt()
        .args([
            "--json",
            "--db",
            db.to_str().unwrap(),
            "reconstruct",
            "list-runs",
            proj.to_str().unwrap(),
        ])
        .assert()
        .success();
    let listed: Value = serde_json::from_slice(&list.get_output().stdout).unwrap();
    assert_eq!(listed["data"][0]["id"].as_i64(), Some(run_id));

    // export obj
    let out_obj = tmp.path().join("out.obj");
    let export = lmt()
        .args([
            "--yes",
            "--json",
            "--db",
            db.to_str().unwrap(),
            "export",
            "obj",
            &run_id.to_string(),
            "neutral",
            "--dst",
            out_obj.to_str().unwrap(),
        ])
        .assert()
        .success();
    let exp_env: Value = serde_json::from_slice(&export.get_output().stdout).unwrap();
    assert_eq!(exp_env["ok"], true);
    assert!(out_obj.is_file(), "OBJ file should exist at {:?}", out_obj);
}

/// 2. scatter import 无 --yes → exit 2（refuse）
#[test]
fn scatter_import_refuses_without_yes() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("scatter-arc");
    write_scatter_project_yaml(&proj);
    let csv = scatter_csv_path();

    let assert = lmt()
        .args([
            "total-station",
            "import",
            "--mode",
            "scatter",
            "--columns",
            "x=3,y=4,z=5,label=1",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .failure();
    // INVALID_INPUT = 2（gate_destructive refuse）
    assert_eq!(assert.get_output().status.code(), Some(2));
    // measured.yaml 不能被创建
    assert!(
        !proj.join("measurements/measured.yaml").is_file(),
        "measured.yaml must not be created when refused"
    );
}

/// 3. scatter dry-run + bad columns 格式 → exit 2（invalid_input，列号非数字）
#[test]
fn scatter_import_dryrun_bad_columns() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("scatter-arc");
    write_scatter_project_yaml(&proj);
    let csv = scatter_csv_path();

    let assert = lmt()
        .args([
            "--dry-run",
            "--json",
            "total-station",
            "import",
            "--mode",
            "scatter",
            "--columns",
            "x=abc",  // 列号非数字
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .failure();
    assert_eq!(assert.get_output().status.code(), Some(2));
    let stderr = std::str::from_utf8(&assert.get_output().stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "invalid_input");
}

// ── --output flag + ndjson mode ───────────────────────────────────────────────

#[test]
fn output_json_is_alias_for_legacy_json_flag() {
    let out = lmt().args(["--output", "json", "schema"]).assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("stdout JSON envelope");
    assert_eq!(env["ok"], true);
}

#[test]
fn output_ndjson_schema_emits_result_event() {
    let out = lmt().args(["--output", "ndjson", "schema"]).assert().success().get_output().clone();
    let line = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(line.trim()).expect("one ndjson line");
    assert_eq!(v["type"], "result");
    assert_eq!(v["final"], true);
}

#[test]
fn legacy_json_flag_still_works() {
    lmt().args(["--json", "schema"]).assert().success();
}

#[test]
fn no_color_and_no_input_flags_accepted() {
    lmt().args(["--no-color", "--no-input", "schema"]).assert().success();
}

#[test]
fn output_equals_json_invalid_flag_yields_envelope_on_stderr() {
    // spec §3.1 要求 parser 接受 --key=value;machine 模式检测不能漏 --output=json,
    // 否则 parse error 会 fallback 到 human clap 输出而非 JSON envelope。
    let assert = lmt().args(["--output=json", "--bogus"]).assert().failure();
    let out = assert.get_output();
    assert_eq!(out.status.code(), Some(2));
    let env: Value = serde_json::from_slice(&out.stderr).expect("stderr JSON envelope");
    assert_eq!(env["ok"], false);
}

// ── completion ────────────────────────────────────────────────────────────────

#[test]
fn completion_bash_emits_script_to_stdout() {
    let out = lmt().args(["completion", "bash"]).assert().success().get_output().clone();
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("lmt"), "bash completion should mention lmt: first 80 = {:?}", &s[..s.len().min(80)]);
}

/// 4. 随机噪声散点 → import ok → reconstruct → exit 12 surface_fit_failed
#[test]
fn scatter_reconstruct_fit_failure_surface_fit_failed() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("scatter-noise");
    write_scatter_project_yaml(&proj);
    let db = tmp.path().join("lmt.sqlite");
    let csv = tmp.path().join("noise.csv");
    write_noise_csv(&csv);

    // import（应成功，import 只存原始散点）
    lmt()
        .args([
            "--yes",
            "--db",
            db.to_str().unwrap(),
            "total-station",
            "import",
            "--mode",
            "scatter",
            "--columns",
            "x=3,y=4,z=5",
            proj.to_str().unwrap(),
            "MAIN",
            csv.to_str().unwrap(),
        ])
        .assert()
        .success();

    // reconstruct → 应该失败 exit 12
    let reconstruct = lmt()
        .args([
            "--yes",
            "--json",
            "--db",
            db.to_str().unwrap(),
            "reconstruct",
            "surface",
            proj.to_str().unwrap(),
            "MAIN",
            "measurements/measured.yaml",
        ])
        .assert()
        .failure();
    let out = reconstruct.get_output();
    assert_eq!(out.status.code(), Some(12), "exit code must be 12 (surface_fit_failed)");
    let stderr = std::str::from_utf8(&out.stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "surface_fit_failed");
}

// ── seed-example ──────────────────────────────────────────────────────────────

#[test]
fn seed_example_dry_run_does_not_write() {
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path();
    let out = lmt()
        .args(["--json", "--dry-run", "seed-example", "curved-flat"])
        .arg(dst)
        .assert().success().get_output().clone();
    let env: Value = serde_json::from_slice(&out.stdout).expect("JSON envelope");
    assert_eq!(env["data"]["dry_run"], true);
    assert!(!dst.join("curved-flat/project.yaml").exists(), "dry-run must not write");
}

#[test]
fn seed_example_yes_writes_project_yaml() {
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path();
    lmt().args(["--json", "--yes", "seed-example", "curved-flat"])
        .arg(dst)
        .assert().success();
    assert!(dst.join("curved-flat/project.yaml").is_file(), "expected seeded project.yaml");
    assert!(dst.join("curved-flat/measurements/measured.yaml").is_file(), "subdir file should be seeded recursively");
    assert!(dst.join("curved-flat/measurements/raw.csv").is_file(), "subdir file should be seeded recursively");
}

#[test]
fn seed_example_unknown_name_is_not_found() {
    let tmp = TempDir::new().unwrap();
    let assert = lmt()
        .args(["--json", "--yes", "seed-example", "does-not-exist"])
        .arg(tmp.path())
        .assert().failure();
    // not_found -> exit 3
    assert_eq!(assert.get_output().status.code(), Some(3));
}

#[test]
fn seed_example_dry_run_unknown_name_fails_fast() {
    // dry-run preflight 必须对未知 name 失败,而不是报 ok 让 agent 误以为安全。
    let tmp = TempDir::new().unwrap();
    let assert = lmt()
        .args(["--json", "--dry-run", "seed-example", "does-not-exist"])
        .arg(tmp.path())
        .assert().failure();
    assert_eq!(assert.get_output().status.code(), Some(3));
}

#[test]
fn seed_example_refuses_existing_destination_and_leaves_it_intact() {
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path();
    // 第一次 seed 成功
    lmt().args(["--json", "--yes", "seed-example", "curved-flat"]).arg(dst).assert().success();
    // 在目标里放一个 sentinel,证明第二次 seed 不碰它
    let sentinel = dst.join("curved-flat/SENTINEL.txt");
    std::fs::write(&sentinel, "keep-me").unwrap();
    // 第二次 seed 同目标 -> 拒绝(invalid_input -> exit 2),sentinel 原样保留
    let assert = lmt()
        .args(["--json", "--yes", "seed-example", "curved-flat"]).arg(dst)
        .assert().failure();
    assert_eq!(assert.get_output().status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "keep-me");
}

// ── visual subcommand smoke tests (Task 1.9) ──────────────────────────────────

/// visual reconstruct with neither --capture-manifest nor --images →
/// INVALID_INPUT (exit 2). The method check (charuco) passes first, then
/// manifest resolution fails before gate_destructive is reached.
#[test]
fn visual_reconstruct_missing_manifest_is_invalid_input() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();

    let assert = lmt()
        .args([
            "--json",
            "visual",
            "reconstruct",
            proj.to_str().unwrap(),
            "MAIN",
            // no --capture-manifest, no --images, no --yes / --dry-run
        ])
        .assert()
        .failure();
    let out = assert.get_output();
    // invalid_input → exit 2
    assert_eq!(out.status.code(), Some(2), "expected exit 2 (invalid_input)");
    let stderr = std::str::from_utf8(&out.stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "invalid_input");
}

/// visual reconstruct with --method structured-light → UNSUPPORTED (exit 7)
/// regardless of other flags.
#[test]
fn visual_reconstruct_structured_light_is_unsupported() {
    let tmp = TempDir::new().unwrap();
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    // Create a dummy manifest file so we get past the path check.
    let manifest = tmp.path().join("manifest.json");
    std::fs::write(&manifest, "{}").unwrap();

    let assert = lmt()
        .args([
            "--json",
            "visual",
            "reconstruct",
            proj.to_str().unwrap(),
            "MAIN",
            "--capture-manifest",
            manifest.to_str().unwrap(),
            "--method",
            "structured-light",
            "--yes",
        ])
        .assert()
        .failure();
    let out = assert.get_output();
    // unsupported → exit 7
    assert_eq!(out.status.code(), Some(7), "expected exit 7 (unsupported)");
    let stderr = std::str::from_utf8(&out.stderr).unwrap().trim_end();
    let env: Value = serde_json::from_str(stderr).expect("stderr must be JSON envelope");
    assert_eq!(env["ok"], false);
    assert_eq!(env["error"]["code"], "unsupported");
}

/// Fix 1 regression: name 含路径分量 (e.g. "curved-flat/measurements") 必须被
/// 顶层白名单拒绝,execute 和 dry-run 都走同一个 not_found 路径 → exit 3,
/// 且 dst 目录不写任何内容。
#[test]
fn seed_example_rejects_path_component_name() {
    // -- execute path (--yes) --
    let tmp = TempDir::new().unwrap();
    let dst = tmp.path();
    let assert_yes = lmt()
        .args(["--json", "--yes", "seed-example", "curved-flat/measurements"])
        .arg(dst)
        .assert()
        .failure();
    let out_yes = assert_yes.get_output();
    // not_found -> exit 3
    assert_eq!(out_yes.status.code(), Some(3), "--yes path: expected exit 3");
    let stderr_yes = std::str::from_utf8(&out_yes.stderr).unwrap().trim_end();
    let env_yes: Value = serde_json::from_str(stderr_yes).expect("--yes path: stderr must be JSON envelope");
    assert_eq!(env_yes["ok"], false);
    assert_eq!(env_yes["error"]["code"], "not_found");
    // nothing written
    assert!(
        std::fs::read_dir(dst).unwrap().next().is_none(),
        "--yes path: dst must be empty after rejection"
    );

    // -- dry-run path --
    let tmp2 = TempDir::new().unwrap();
    let dst2 = tmp2.path();
    let assert_dry = lmt()
        .args(["--json", "--dry-run", "seed-example", "curved-flat/measurements"])
        .arg(dst2)
        .assert()
        .failure();
    let out_dry = assert_dry.get_output();
    // dry-run preflight also returns not_found -> exit 3
    assert_eq!(out_dry.status.code(), Some(3), "--dry-run path: expected exit 3");
    let stderr_dry = std::str::from_utf8(&out_dry.stderr).unwrap().trim_end();
    let env_dry: Value = serde_json::from_str(stderr_dry).expect("--dry-run path: stderr must be JSON envelope");
    assert_eq!(env_dry["ok"], false);
    assert_eq!(env_dry["error"]["code"], "not_found");
}
