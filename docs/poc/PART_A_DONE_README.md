# M2 Part A Done — PoC Manual Gate Instructions

This file marks the boundary between **Part A (MVP, headless adapter
implementation)** and **Part B (productionization: PyInstaller, CI,
e2e/cancel/error coverage)**.

Part A is complete when:

- [x] `python-sidecar` has the three subcommands (`calibrate`,
      `generate_pattern`, `reconstruct`) and `pytest` passes
- [x] `lmt-adapter-visual-ba` Rust crate end-to-end exercises the
      sidecar via mock fixtures, with covariance unit conversion
      verified
- [x] `lmt-poc-compare` bin computes A-mode RMS and C-mode holdout
      RMS / p95 / anchor residuals from two `MeasuredPoints` files

## Manual gate

The user runs the PoC field session externally. The plan treats this as
a **manual checkpoint** — Claude does not automate it. After the
session:

1. Calibrate the camera and capture the test set (see report template
   §1, §2).
2. Run sidecar `generate_pattern`, `calibrate`, then `reconstruct` in
   both A and C modes.
3. Acquire total-station ground truth for the entire ChArUco set.
4. Run `lmt-poc-compare` for both modes and write the report from
   `2026-XX-XX-m2-poc-report-template.md`.

## Gate criteria

From spec `2026-05-11-m2-visual-ba-design.md` §9.3:

| Mode | Threshold |
|---|---|
| A (nominal_anchoring) | RMS < 10mm |
| C (three_points) | Holdout RMS < 5mm |
| C (three_points) | Holdout p95 < 8mm |

Decisions:

- **Pass**: proceed to Part B.
- **Conditional pass (C only)**: proceed to Part B; mark A as
  experimental in user docs.
- **Fail**: stop the plan; investigate root cause (pattern, SOP, or
  algorithm) and revise spec.

## After the gate

Commit the filled-out report under `docs/poc/2026-MM-DD-m2-poc-report.md`,
then Part B tasks (PyInstaller scripts, CI, e2e tests, cancel/error
coverage) are unblocked.
