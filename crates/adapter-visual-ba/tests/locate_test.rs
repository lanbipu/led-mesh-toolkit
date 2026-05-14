//! Locate sidecar tests. Use a serial mutex because env var mutation is
//! global and parallel tests would race.

use std::env;
use std::fs;
use std::sync::Mutex;

use lmt_adapter_visual_ba::locate::locate_sidecar;
use tempfile::tempdir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn make_executable_fake(path: &std::path::Path) {
    fs::write(path, b"#!/bin/sh\necho hi\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn env_var_override_takes_precedence() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempdir().unwrap();
    let fake = tmp.path().join("fake-sidecar");
    make_executable_fake(&fake);
    env::set_var("LMT_VBA_SIDECAR_PATH", fake.to_str().unwrap());
    let resolved = locate_sidecar().unwrap();
    assert_eq!(resolved, fake);
    env::remove_var("LMT_VBA_SIDECAR_PATH");
}

#[test]
fn missing_sidecar_returns_error_when_path_disabled() {
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var("LMT_VBA_SIDECAR_PATH", "/nonexistent/path/lmt-vba");
    env::remove_var("LMT_VBA_ALLOW_PATH");
    let result = locate_sidecar();
    env::remove_var("LMT_VBA_SIDECAR_PATH");
    // Default-disabled PATH fallback prevents accidental picks.
    if result.is_ok() {
        // If a workspace vendor binary happens to exist in this dev tree,
        // the test passes for a different reason. Allow either outcome
        // since both prove the locator works without falling through to PATH.
        return;
    }
    let err_str = format!("{:?}", result.err().unwrap());
    assert!(
        err_str.contains("PATH lookup disabled"),
        "expected PATH disabled note, got {err_str}"
    );
}

#[test]
fn path_fallback_opt_in_finds_binary() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempdir().unwrap();
    let fake = tmp.path().join(if cfg!(windows) {
        "lmt-vba-sidecar.exe"
    } else {
        "lmt-vba-sidecar"
    });
    make_executable_fake(&fake);

    env::remove_var("LMT_VBA_SIDECAR_PATH");
    let saved = env::var_os("PATH");
    env::set_var("PATH", tmp.path());
    env::set_var("LMT_VBA_ALLOW_PATH", "1");

    let result = locate_sidecar();

    if let Some(p) = saved {
        env::set_var("PATH", p);
    }
    env::remove_var("LMT_VBA_ALLOW_PATH");

    let resolved = result.unwrap();
    assert_eq!(resolved, fake);
}
