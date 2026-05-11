# LED Mesh Toolkit

LED 屏几何建模工具集。

## Status

- M0.1 Rust core — done (tag `m0.1-complete`)
- M1.1 Total-station adapter — done (tag `m1.1-complete`)
- M0.2 GUI shell + Tauri integration — pending
- M2 视觉反算 adapter — pending

## Structure

```
crates/
├── core/                       # IR + reconstruct + UV + export (frozen after M0.1)
├── adapter-total-station/      # M1 — placeholder
└── adapter-visual-ba/          # M2 — placeholder
```

## Build & test

```bash
cargo build --workspace
cargo test --workspace
```

## Spec

See `docs/superpowers/specs/2026-05-10-led-mesh-toolkit-design.md`.
