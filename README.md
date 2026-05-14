# LED Mesh Toolkit

LED 屏几何建模工具集。

## Status

- M0.1 Rust core — done (tag `m0.1-complete`)
- M1.1 Total-station adapter — done (tag `m1.1-complete`)
- M0.2 GUI shell + Tauri integration — done (tag `m0.2-complete`)
- M1 in GUI — Trimble CSV import + instruction card in Import.vue / Instruct.vue
- M2 Visual photogrammetry adapter — Part A done, Part B blocked on field PoC

## Structure

```
crates/
├── core/                       # IR + reconstruct + UV + export (frozen after M0.1)
├── adapter-total-station/      # M1 — CSV/YAML → MeasuredPoints + HTML/PDF instruction card
└── adapter-visual-ba/          # M2 — Python sidecar (ChArUco + BA); Part A merged, Part B blocked on PoC
src-tauri/                      # Tauri 2.x backend; commands map adapters to GUI views
src/                            # Vue 3 + Pinia GUI (Design / Import / Instruct / Preview / Runs)
```

## Build & test

```bash
cargo build --workspace
cargo test --workspace
```

## Spec

See `docs/superpowers/specs/2026-05-10-led-mesh-toolkit-design.md`.
