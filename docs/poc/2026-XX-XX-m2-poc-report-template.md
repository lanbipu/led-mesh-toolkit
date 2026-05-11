# M2 PoC Report (template)

> Rename this file to `2026-MM-DD-m2-poc-report.md` once the on-site
> session is run, fill all `<<...>>` placeholders, attach raw data,
> and commit.

## 1. Test Conditions

- Date: <<YYYY-MM-DD>>
- Location: <<studio name / address>>
- LED panel: <<vendor + model>>; pitch: <<mm>>; cabinet size: <<W×H mm>>
- Test wall: <<cols × rows>> cabinets
- Camera: <<body + lens + ISO + shutter + aperture>>
- Lighting: <<ambient lux + LED brightness %>>

## 2. Procedure

1. Generate ChArUco patterns (`lmt-vba-sidecar generate_pattern …`).
2. Display assembled `full_screen.png` on test wall via Disguise.
3. Capture <<N>> stills following spec §5.3 SOP.
4. Calibrate camera intrinsics with checkerboard set
   (`lmt-vba-sidecar calibrate …`).
5. Run reconstruct in **A mode** (`frame_strategy=nominal_anchoring`).
6. Run reconstruct in **C mode** (`frame_strategy=three_points`); anchor
   IDs and their measured positions listed below.
7. Total-station ground truth: full ChArUco corner set measured.

## 3. C-Mode Anchors

| ArUco ID | Cabinet | Total-station position (m) |
|---|---|---|
| <<id>> | V<<col>>_R<<row>> | (<<x>>, <<y>>, <<z>>) |
| <<id>> | V<<col>>_R<<row>> | (<<x>>, <<y>>, <<z>>) |
| <<id>> | V<<col>>_R<<row>> | (<<x>>, <<y>>, <<z>>) |

> Anchors must be spatially spread (not collinear, ideally near corners).

## 4. Results

### 4.1 A mode

```bash
lmt-poc-compare --ground-truth gt.json --measured visual_a.json \
  --frame-strategy nominal_anchoring > a_report.json
```

| Metric | Value |
|---|---|
| RMS (mm) | <<>> |
| 95th percentile (mm) | <<>> |
| n_compared | <<>> |

### 4.2 C mode

```bash
lmt-poc-compare --ground-truth gt.json --measured visual_c.json \
  --frame-strategy three_points --anchor-ids <<id1,id2,id3>> > c_report.json
```

| Metric | Value |
|---|---|
| Holdout RMS (mm) | <<>> |
| Holdout 95th percentile (mm) | <<>> |
| Anchor residual RMS (mm) | <<>> |

### 4.3 Per-point error

Attach `a_report.json` and `c_report.json`; reproduce per-point error
table here. Highlight cabinets with > 8mm error in C holdout or > 15mm
in A.

## 5. Raw Data Paths

- Images: `<<path>>`
- Total-station CSV: `<<path>>`
- Calibration set: `<<path>>`
- intrinsics.json: `<<path>>`

## 6. Conclusion

- A mode RMS < 10mm? **<<yes/no>>**
- C mode holdout RMS < 5mm? **<<yes/no>>**
- C mode holdout p95 < 8mm? **<<yes/no>>**

### Decision

- [ ] Pass: continue to Part B (productionization)
- [ ] Conditional pass (C only): default to C; A flagged experimental
- [ ] Fail: pause plan; investigate <<pattern / SOP / algorithm>>

## 7. Next Steps

<<bulleted list>>
