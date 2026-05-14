//! Spawn a mock sidecar (shell script), parse its NDJSON stream.

use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use lmt_adapter_visual_ba::sidecar::{run_sidecar, SidecarRequest};
use serde_json::json;
use tokio::sync::mpsc;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn mock_path() -> PathBuf {
    fixture("mock_sidecar.sh")
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[tokio::test]
async fn mock_sidecar_round_trip() {
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var("LMT_VBA_SIDECAR_PATH", mock_path().to_str().unwrap());
    let (tx, mut rx) = mpsc::channel(16);
    let payload = json!({"command":"reconstruct","version":1});

    let task = tokio::spawn(async move {
        run_sidecar(SidecarRequest {
            subcommand: "reconstruct".into(),
            payload,
            progress_tx: Some(tx),
            cancel: None,
        })
        .await
    });

    let mut events = Vec::new();
    while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        events.push(ev);
    }

    let result = task.await.unwrap().unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, lmt_adapter_visual_ba::Event::Progress(_))));
    assert!(result.frame_strategy_used == lmt_adapter_visual_ba::FrameStrategy::NominalAnchoring);
    env::remove_var("LMT_VBA_SIDECAR_PATH");
}

#[tokio::test]
async fn stderr_is_drained_and_included_in_failure_message() {
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var(
        "LMT_VBA_SIDECAR_PATH",
        fixture("mock_sidecar_stderr.sh").to_str().unwrap(),
    );
    let payload = serde_json::json!({"command":"reconstruct","version":1});

    let result = run_sidecar(SidecarRequest {
        subcommand: "reconstruct".into(),
        payload,
        progress_tx: None,
        cancel: None,
    })
    .await;

    env::remove_var("LMT_VBA_SIDECAR_PATH");

    let err = result.unwrap_err();
    let s = format!("{err}");
    assert!(s.contains("non-zero"), "expected non-zero in error: {s}");
    assert!(
        s.contains("stderr-line-"),
        "expected stderr tail in error: {s}"
    );
}

#[tokio::test]
async fn slow_progress_consumer_does_not_block_stdout() {
    use lmt_adapter_visual_ba::Event;
    let _guard = ENV_LOCK.lock().unwrap();
    env::set_var("LMT_VBA_SIDECAR_PATH", mock_path().to_str().unwrap());
    // Bounded channel, capacity 1 — the mock emits 3 events, so try_send must
    // drop overflow; if read_events did .send(.).await this test would hang.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Event>(1);

    let task = tokio::spawn(async move {
        run_sidecar(SidecarRequest {
            subcommand: "reconstruct".into(),
            payload: serde_json::json!({"command":"reconstruct","version":1}),
            progress_tx: Some(tx),
            cancel: None,
        })
        .await
    });

    // Don't drain rx during the run — simulating a slow consumer.
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("must not hang on slow consumer")
        .expect("task panicked")
        .expect("sidecar should still complete");
    assert_eq!(
        result.frame_strategy_used,
        lmt_adapter_visual_ba::FrameStrategy::NominalAnchoring
    );

    // Drain whatever did make it (≤ capacity).
    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert!(count <= 1, "channel cap should bound buffered events");
    env::remove_var("LMT_VBA_SIDECAR_PATH");
}
