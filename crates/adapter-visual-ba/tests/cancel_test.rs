//! Cancel + protocol error path coverage.

use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lmt_adapter_visual_ba::sidecar::{run_sidecar, SidecarRequest};
use serde_json::json;
use tokio::sync::oneshot;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

#[tokio::test]
async fn cancel_kills_child_within_5_seconds() {
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var("LMT_VBA_SIDECAR_PATH", fixture("mock_sidecar_slow.sh"));
    let (cancel_tx, cancel_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        run_sidecar(SidecarRequest {
            subcommand: "reconstruct".into(),
            payload: json!({"command":"reconstruct","version":1}),
            progress_tx: None,
            cancel: Some(cancel_rx),
        })
        .await
    });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let start = Instant::now();
    let _ = cancel_tx.send(());
    let result = task.await.unwrap();
    let elapsed = start.elapsed();

    assert!(matches!(result, Err(lmt_adapter_visual_ba::VbaError::Cancelled)));
    assert!(elapsed < Duration::from_secs(5), "cancel took {elapsed:?}");

    env::remove_var("LMT_VBA_SIDECAR_PATH");
}

#[tokio::test]
async fn protocol_error_returns_typed_error() {
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var("LMT_VBA_SIDECAR_PATH", fixture("mock_sidecar_error.sh"));
    let result = run_sidecar(SidecarRequest {
        subcommand: "reconstruct".into(),
        payload: json!({"command":"reconstruct","version":1}),
        progress_tx: None,
        cancel: None,
    })
    .await;
    match result {
        Err(lmt_adapter_visual_ba::VbaError::Protocol { code, .. }) => {
            assert_eq!(code, "detection_failed");
        }
        other => panic!("expected Protocol error, got {other:?}"),
    }
    env::remove_var("LMT_VBA_SIDECAR_PATH");
}
