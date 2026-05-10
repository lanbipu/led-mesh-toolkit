//! End-to-end test that uses the actual PyInstaller-built sidecar (or a
//! dev wrapper that invokes `python -m lmt_vba_sidecar`) — proves the
//! wire protocol works against a real Python process, not just shell mocks.
//!
//! Skipped by default. Set `LMT_VBA_SIDECAR_PATH` to enable. Useful as a
//! smoke check after PyInstaller builds.

use std::env;

use lmt_adapter_visual_ba::sidecar::{run_sidecar, SidecarRequest};
use serde_json::json;

#[tokio::test]
#[ignore = "requires LMT_VBA_SIDECAR_PATH set to a real sidecar binary or wrapper"]
async fn real_sidecar_handles_invalid_input_gracefully() {
    let exe = match env::var("LMT_VBA_SIDECAR_PATH") {
        Ok(p) => p,
        Err(_) => {
            eprintln!("skipping: LMT_VBA_SIDECAR_PATH not set");
            return;
        }
    };
    env::set_var("LMT_VBA_SIDECAR_PATH", &exe);

    // Send a valid JSON object that fails schema validation (missing fields).
    // The sidecar must respond with an `error` event having code `invalid_input`,
    // exit non-zero, and our adapter must surface it as VbaError::Protocol.
    let result = run_sidecar(SidecarRequest {
        subcommand: "reconstruct".into(),
        payload: json!({"command":"reconstruct","version":1}),
        progress_tx: None,
        cancel: None,
    })
    .await;

    match result {
        Err(lmt_adapter_visual_ba::VbaError::Protocol { code, .. }) => {
            assert_eq!(code, "invalid_input", "expected invalid_input, got {code}");
        }
        other => panic!("expected Protocol(invalid_input), got {other:?}"),
    }
}
