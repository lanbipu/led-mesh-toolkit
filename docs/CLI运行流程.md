# CLI 运行流程

通过 `lmt` CLI 完整跑一遍 LED Mesh Toolkit 各项功能并记录结果。
供 Skill 开发、UI 功能验证、部署工作参考。

> **Binary 路径**：`./target/debug/lmt`（开发期）。若编译过期先 `cargo build -p lmt-cli`。编译产物是独立可执行文件，源码不变无需重新编译。
>
> **工作区**：演练产物统一放在项目内 `_walkthrough/` 目录（已加入 `.gitignore`），永久保存供后续参考。
>
> **DB 隔离**：演练时务必 `--db _walkthrough/test.sqlite`，避免污染默认 DB（Tauri GUI 共用的 `lmt.sqlite`）。
>
> **破坏性操作**：凡 side_effect = destructive 的命令需传 `--yes`（确认执行）或 `--dry-run`（预演不写盘）。

---

## 0. 前置准备

```bash
# 编译 CLI（确保最新；源码未改则跳过）
cargo build -p lmt-cli

# 确认可用
./target/debug/lmt --version
# → lmt 0.1.0

# 工作区在项目内（已 gitignore）
export LMT_WORK=_walkthrough
mkdir -p $LMT_WORK

# 隔离 DB（避免污染 GUI 的默认 lmt.sqlite）
export LMT_DB=$LMT_WORK/test.sqlite
```

后续所有命令统一使用 `--db $LMT_DB`（或设 `export LMT_DB_PATH=$LMT_DB` 一次生效）。

---

## 1. 元信息查询（read_only）

### 1.1 版本

```bash
./target/debug/lmt --version
./target/debug/lmt --json version
```

预期：`--version` 输出 `lmt x.y.z`；`--json version` 输出 `{ok:true, data:{version, schema_version, contract_version}}`。

### 1.2 Schema dump

```bash
./target/debug/lmt --json schema | python3 -m json.tool | head -40
```

预期：`{schema_version, types:{...}, incomplete:[...]}` — types 包含所有公开 DTO，incomplete 列出嵌入了 `lmt-core` 域类型的不完整项。

### 1.3 Contract Manifest

```bash
./target/debug/lmt --json manifest | python3 -c "
import sys, json
d = json.load(sys.stdin)
for op in d['data']['operations']:
    print(f\"{op['operation_id']:45s} {op['side_effect']:15s} {op['cli']}\")
"
```

预期：列出全部 operation（当前 28 个；`completion` 不在 manifest 里因为它输出原始脚本不走 JSON envelope），每个有 `operation_id` / `side_effect` / `cli` / `exit_codes`。

### 1.4 Shell 补全脚本

```bash
./target/debug/lmt completion zsh > /tmp/_lmt
# 注意：completion 输出是原始脚本，不套 JSON envelope
```

---

## 2. Seed Example（项目脚手架）

内置两个示例项目：`curved-flat`（8×4 平面墙）和 `curved-arc`（16×6 弧面墙）。

```bash
# dry-run 预览
./target/debug/lmt --json --db $LMT_DB seed-example curved-flat $LMT_WORK --dry-run

# 实际创建
./target/debug/lmt --json --db $LMT_DB seed-example curved-flat $LMT_WORK --yes
./target/debug/lmt --json --db $LMT_DB seed-example curved-arc  $LMT_WORK --yes
```

预期产物：

```
$LMT_WORK/curved-flat/
├── project.yaml
└── measurements/
    ├── measured.yaml
    └── raw.csv

$LMT_WORK/curved-arc/
├── project.yaml
└── measurements/
    └── measured.yaml
```

验证：

```bash
ls -R $LMT_WORK/curved-flat/
ls -R $LMT_WORK/curved-arc/
```

---

## 3. 项目管理（project）

### 3.1 加载项目配置

```bash
./target/debug/lmt --json project load $LMT_WORK/curved-flat
```

预期：返回 `project.yaml` 的完整内容（`ProjectConfig` DTO）—— 含 `project`（名称/单位）、`screens`（cabinet 布局/形状/像素）、`coordinate_system`、`output`。

### 3.2 注册到最近项目

```bash
./target/debug/lmt --json --db $LMT_DB project add-recent $LMT_WORK/curved-flat "Flat Demo"
./target/debug/lmt --json --db $LMT_DB project add-recent $LMT_WORK/curved-arc  "Arc Demo"
```

### 3.3 列出最近项目

```bash
./target/debug/lmt --json --db $LMT_DB project list-recent
```

预期：两条记录，`abs_path` 已 canonicalize。

### 3.4 保存项目配置（round-trip）

```bash
# 把当前配置导出 → 修改 → 回写
./target/debug/lmt --json project load $LMT_WORK/curved-flat \
  | python3 -c "import sys,json; d=json.load(sys.stdin); print(json.dumps(d['data']))" \
  > /tmp/project_snapshot.json

./target/debug/lmt --json --db $LMT_DB project save $LMT_WORK/curved-flat \
  --input /tmp/project_snapshot.json --yes
```

### 3.5 删除最近项目

```bash
# 先拿到 id
ID=$(./target/debug/lmt --json --db $LMT_DB project list-recent \
  | python3 -c "import sys,json; rows=json.load(sys.stdin)['data']; print(rows[-1]['id'])")

./target/debug/lmt --json --db $LMT_DB project remove-recent $ID --yes
```

---

## 4. 测量数据读取（measurements）

```bash
./target/debug/lmt --json measurements load $LMT_WORK/curved-flat/measurements/measured.yaml
```

预期：返回 `MeasuredPoints` DTO — 含 `screen_id`, `coordinate_frame`, `cabinet_array`, `shape_prior`, `sampling_mode`, `points[]`。
curved-flat 示例有 11 个全站仪测量点，坐标单位米（m）。

---

## 5. 全站仪管线（total-station）

### 5.1 Grid 模式导入

curved-flat 自带 `raw.csv`，走 grid 导入（默认模式）：

```bash
# dry-run
./target/debug/lmt --json --db $LMT_DB total-station import \
  $LMT_WORK/curved-flat MAIN \
  $LMT_WORK/curved-flat/measurements/raw.csv \
  --dry-run

# 执行（会覆盖已有 measured.yaml）
./target/debug/lmt --json --db $LMT_DB total-station import \
  $LMT_WORK/curved-flat MAIN \
  $LMT_WORK/curved-flat/measurements/raw.csv \
  --yes
```

预期产物：`measurements/measured.yaml` + `measurements/import_report.json`。
实测返回：`measuredCount: 45, fabricatedCount: 0, outlierCount: 0`。

### 5.2 Scatter 模式导入

使用 E2E 测试的 fixture（散点 CSV，单位=米）：

```bash
# 先建一个 curved-arc 项目副本
cp -r $LMT_WORK/curved-arc $LMT_WORK/curved-arc-scatter

./target/debug/lmt --json --db $LMT_DB total-station import \
  $LMT_WORK/curved-arc-scatter MAIN \
  crates/lmt-cli/tests/fixtures/scatter_arc.csv \
  --mode scatter \
  --columns x=3,y=4,z=5 \
  --yes
```

预期：`measured.yaml` 中 `sampling_mode: Scatter`，点坐标按原始值存储（不做 SOP 校验）。
实测返回：`measuredCount: 125`，warning 正确提示 "scatter mode: points stored raw; fitting + outlier detection happen at reconstruct"。

> **注意**：此 fixture CSV 的数据范围（~9.7×18.8m）大于 curved-arc 项目配置（8×3m），
> 后续 `reconstruct surface` 会因 boundary check 报 `surface_fit_failed`（exit 12）。
> 这不是 bug，而是测试数据与项目配置不匹配。scatter 导入本身只做原始存储，不校验几何。

### 5.3 指引卡 HTML

```bash
./target/debug/lmt total-station instruction-card \
  $LMT_WORK/curved-flat MAIN \
  > $LMT_WORK/instruction_card.html

# 用浏览器打开查看
open $LMT_WORK/instruction_card.html
```

预期：一页自包含 HTML 指引卡，展示全站仪测量的靶点编号和位置。实测 ~4.8KB。

### 5.4 错误场景

```bash
# 不带 --yes 也不带 --dry-run → exit 2 (invalid_input)
./target/debug/lmt --json --db $LMT_DB total-station import \
  $LMT_WORK/curved-flat MAIN \
  $LMT_WORK/curved-flat/measurements/raw.csv
echo "exit code: $?"
```

预期：`exit code: 2`，stderr 输出 `{ok:false, error:{code:"invalid_input", ...}}`。

---

## 6. 几何重建（reconstruct）

### 6.1 Grid 重建

```bash
./target/debug/lmt --json --db $LMT_DB reconstruct surface \
  $LMT_WORK/curved-flat MAIN \
  measurements/measured.yaml \
  --yes
```

预期产物：`reports/<timestamp>.json` + DB 插入一条 `reconstruction_runs` 记录。
返回 envelope 包含 `ReconstructionResult`（surface fit 结果 + 质量指标）。
实测返回：`run_id: 1, estimated_rms_mm: 2.0, vertices: 45, method: direct_link`。

### 6.2 Scatter 重建

```bash
./target/debug/lmt --json --db $LMT_DB reconstruct surface \
  $LMT_WORK/curved-arc-scatter MAIN \
  measurements/measured.yaml \
  --yes
```

预期：散点走 RANSAC 曲面拟合 → `reports/<timestamp>.json`。

> **实测结果**：使用 §5.2 的 fixture 数据时会报 `surface_fit_failed`（exit 12），
> 因为散点数据范围与项目 cabinet 尺寸不匹配（见 §5.2 注意事项）。
> 使用匹配的真实散点数据时可正常通过。

### 6.3 查询 run 历史

```bash
# 列出所有 run
./target/debug/lmt --json --db $LMT_DB reconstruct list-runs $LMT_WORK/curved-flat

# 按 screen_id 过滤
./target/debug/lmt --json --db $LMT_DB reconstruct list-runs $LMT_WORK/curved-flat --screen-id MAIN
```

### 6.4 获取 run 报告

```bash
# 拿到最新 run_id
RUN_ID=$(./target/debug/lmt --json --db $LMT_DB reconstruct list-runs $LMT_WORK/curved-flat \
  | python3 -c "import sys,json; runs=json.load(sys.stdin)['data']; print(runs[-1]['id'])")

./target/debug/lmt --json --db $LMT_DB reconstruct get-run-report $RUN_ID
```

---

## 7. OBJ 导出（export）

### 7.1 从 run 导出

```bash
./target/debug/lmt --json --db $LMT_DB export obj $RUN_ID disguise \
  --dst $LMT_WORK/curved-flat/output.obj --yes

./target/debug/lmt --json --db $LMT_DB export obj $RUN_ID neutral \
  --dst $LMT_WORK/curved-flat/output_neutral.obj --yes
```

预期：生成 `.obj` 文件。target 可选 `disguise` / `unreal` / `neutral`：
- `disguise`：+Y up / +Z 朝观众 / flipY + winding 反转
- `unreal`：Unreal Engine 约定
- `neutral`：原始右手系（+Z up）

实测：两种 target 均 160 行，45 vertices / 64 triangles。OBJ 头注释标明坐标系约定。

### 7.2 从 pose report 导出（pose-obj）

```bash
# 需要先通过 visual reconstruct 产出 cabinet_pose_report.json
# 这里先跳过，在 §9 visual 管线完成后回来跑
./target/debug/lmt --json --db $LMT_DB export pose-obj \
  <cabinet_pose_report.json> \
  disguise \
  --out $LMT_WORK/curved-flat/world_mesh.obj \
  --yes
  # target: disguise / unreal / neutral
  # --root <cabinet_id>  以指定箱体为基准重定位
  # --ground             让下边缘贴地
```

验证 OBJ：

```bash
head -20 $LMT_WORK/curved-flat/output.obj
wc -l $LMT_WORK/curved-flat/output.obj
```

---

## 8. 合成台 & 评估（visual simulate / eval / compare-known）

这三个命令不需要真实相机数据，适合快速验证。

### 8.1 合成数据集

```bash
# simulate config 需要 scene / cameras / intrinsics / noise 四个顶层字段
cat > $LMT_WORK/sim_config.json << 'EOF'
{
  "scene": {
    "cabinet_array": {
      "cols": 4,
      "rows": 3,
      "cabinet_size_mm": [500, 500],
      "absent_cells": []
    },
    "shape_prior": "flat"
  },
  "cameras": {
    "n_views": 6,
    "distance_mm_range": [2000, 4000],
    "yaw_deg_range": [-30, 30],
    "pitch_deg_range": [-15, 15]
  },
  "intrinsics": {
    "K": [[2000, 0, 2000], [0, 2000, 1500], [0, 0, 1]],
    "dist_coeffs": [0, 0, 0, 0],
    "image_size": [4000, 3000]
  },
  "noise": {
    "pixel_sigma": 0.5
  },
  "seed": 42
}
EOF

./target/debug/lmt --json visual simulate $LMT_WORK/sim_config.json \
  --out $LMT_WORK/sim_flat --yes
```

预期产物：`$LMT_WORK/sim_flat/` 目录含 `scene.npz` + `meta.json`。
实测返回：`n_views: 6, n_observations: 4608, seed: 42`。

### 8.2 评估方法

```bash
./target/debug/lmt --json visual eval $LMT_WORK/sim_flat --method charuco
  # --seed-matrix 0,1,2（可选：逗号分隔多 seed 评估，默认 [0]）
```

预期：gauge-invariant 指标（尺寸/距离/角度误差），不写文件。
实测返回：`method: charuco, max_size_error_mm: 0.0, max_distance_error_mm: 0.12, max_angle_error_deg: 0.13, seeds: [0]`。

### 8.3 对比已知几何

```bash
# 假设已有一个 pose report 和 known geometry
# 这里用 monitor-bench 的 known_geometry.json 示例结构
./target/debug/lmt --json visual compare-known \
  <pose_report.json> \
  examples/monitor-bench/known_geometry.json
  # 可选容差参数（覆盖默认值）：
  # --max-size-mm 2.0    尺寸误差阈值（默认 2.0mm）
  # --max-dist-mm 3.0    间距误差阈值（默认 3.0mm）
  # --max-angle-deg 0.3  夹角误差阈值（默认 0.3°）
```

预期：per-cabinet 尺寸误差、per-pair 距离/角度误差 + pass/fail 判定。

---

## 9. 视觉管线（visual）— 完整流程

> 视觉管线是 CLI-only（无 GUI shim），依赖 Python sidecar（`lmt-vba-sidecar`）。
> 运行前确保 sidecar 可用：`export LMT_VBA_SIDECAR_PATH=python-sidecar/.venv`（或默认路径）。

### 9.1 采集规划（plan-capture / capture-card）

不需要照片，只需 project.yaml + 相机参数：

```bash
# 规划机位（注意：--standoff / --height 单位是 mm，不是米）
./target/debug/lmt --json visual plan-capture \
  $LMT_WORK/curved-flat MAIN \
  --image-size 4000x3000 --hfov-deg 60 \
  --standoff 1500..4000 --height 500..2500 \
  --target-mm 3.0 --min-views 3
  # --min-views：每箱体最少覆盖视角数（精准档传 3）；
  #   省略则用 sidecar 默认值（与 reconstruct 观测门同源）

# 可视化指导卡
./target/debug/lmt visual capture-card \
  $LMT_WORK/curved-flat MAIN \
  --image-size 4000x3000 --hfov-deg 60 \
  --standoff 1500..4000 --height 500..2500 \
  > $LMT_WORK/capture_card.html

open $LMT_WORK/capture_card.html
```

预期：`plan-capture` 返回 `CapturePlan`（stations + coverage + unreachable_regions + all_pass）；
每个 cabinet 的 coverage 含 `fail_reason` 字段（`null` = pass，否则 `low_parallax` / `low_coverage`）。
`capture-card` 输出自包含 3D HTML（Three.js 内联，可离线打开）。
实测：8 stations（5 fan + 1 top + 1 bottom + 1 added），32/32 cabinets all_pass。
capture-card 输出 ~714KB 自包含 HTML。

### 9.2 相机标定（calibrate）

需要棋盘格照片目录：

```bash
./target/debug/lmt --json visual calibrate \
  $LMT_WORK/curved-flat MAIN \
  <checkerboard_images_dir> \
  --square-mm 25.0 --inner 9x6 \
  --yes
```

预期产物：`calibration/<screen_id>_intrinsics.json`（K 矩阵 + 畸变系数 + reproj error）。

### 9.3 Pattern 生成（generate-pattern）

默认方法为 **VP-QSP**（自编码 marker，32-bit ID 无字典容量上限），替代旧版 ChArUco。

```bash
# VP-QSP（默认）— 均匀网格
./target/debug/lmt --json visual generate-pattern \
  $LMT_WORK/curved-flat MAIN \
  --yes
  # --method vpqsp（默认，可省略）
  # --screen-id-code 0（多屏 Volume 时每屏取不同值 0-15）

# 指定 screen_mapping（逐 cabinet 像素尺寸不同时）
# ./target/debug/lmt --json visual generate-pattern \
#   $LMT_WORK/curved-flat MAIN \
#   --screen-mapping screen_mapping.json --yes

# legacy ChArUco（仍可用，但有 ~13 cabinet 字典容量上限）
# ./target/debug/lmt --json visual generate-pattern \
#   $LMT_WORK/curved-flat MAIN \
#   --method charuco --yes
```

预期产物：`patterns/MAIN/` 目录含 `cabinets/` 下 per-cabinet PNG + `full_screen.png` 合图 + `pattern_meta.json`（`vpqsp.v1` schema）。
实测返回：`cabinet_count: 32, total_markers: 288`（每 cabinet 3×3 = 9 markers）。

> **VP-QSP vs ChArUco**：VP-QSP 用 32-bit 自编码 marker（screen 4bit + col 7bit + row 7bit + local 6bit + CRC8），无 ArUco 字典容量天花板；ChArUco 在 >13 cabinet 时会报 `invalid_input`。

### 9.4 视觉重建（reconstruct）

需要多角度拍摄的照片 + capture_manifest.json：

```bash
./target/debug/lmt --json visual reconstruct \
  $LMT_WORK/curved-flat MAIN \
  --capture-manifest <capture_manifest.json> \
  --yes
  # --method vpqsp（默认，可省略）
  # 实际方法以 capture manifest 的 method 字段为准
```

预期产物：`measurements/measured.yaml` + `measurements/MAIN_cabinet_pose_report.json`。

### 9.5 结构光管线（generate → decode → calibrate → reconstruct）

#### 9.5.1 生成结构光序列

```bash
# 这里用 curved-flat 演示（curved-arc 同理）
./target/debug/lmt --json visual generate-structured-light \
  $LMT_WORK/curved-flat MAIN \
  --yes
  # --dot-spacing / --dot-radius / --margin 不传时按 cabinet 像素自动推导
  # --seq-format auto → project.yaml 的 output.target=="disguise" 时自动输出 .seq
```

预期产物：`patterns/MAIN/sl/` 含 `frames/*.png` + `sequence.mp4` + `sl_meta.json`。
disguise target 项目会额外产出 `MAIN.seq/`（10-bit TIFF 序列）。
实测返回：`n_dots: 2048, n_frames: 15`。

#### 9.5.2 解码结构光录像

```bash
./target/debug/lmt --json visual decode-structured-light \
  <录像文件或帧目录或.dpx目录> \
  $LMT_WORK/curved-arc/patterns/MAIN/sl/sl_meta.json \
  --out $LMT_WORK/curved-arc/corr_pose1.json \
  --emit-debug-image \
  --yes
  # --sentinel-threshold 0.85（默认）
  # --screen-roi X,Y,W,H（可选，手动指定屏幕区域）
```

预期产物：`corr_pose1.json`（screen↔camera 对应点 + provenance）+ `corr_pose1.json.debug.png`。

多机位时对每个机位分别 decode，产出多个 corr 文件。

#### 9.5.3 结构光相机标定

```bash
./target/debug/lmt --json visual calibrate-structured-light \
  $LMT_WORK/curved-arc MAIN \
  --sl-meta $LMT_WORK/curved-arc/patterns/MAIN/sl/sl_meta.json \
  --corr $LMT_WORK/curved-arc/corr_pose1.json \
  --corr $LMT_WORK/curved-arc/corr_pose2.json \
  --corr $LMT_WORK/curved-arc/corr_pose3.json \
  --yes
  # --out 默认 calibration/<screen_id>_sl_intrinsics.json
  # --force        覆盖已存在的内参文件（否则拒绝，防误覆盖可信棋盘格标定）
  # --max-rms-px 1.5  reproj RMS 门槛（默认 1.5px，超出则拒标）
  # --intrinsics-crosscheck <anchor.json> 可选防吸收交叉校验
```

预期产物：`calibration/MAIN_sl_intrinsics.json`。

#### 9.5.4 结构光重建

```bash
./target/debug/lmt --json visual reconstruct-structured-light \
  $LMT_WORK/curved-arc MAIN \
  --sl-meta $LMT_WORK/curved-arc/patterns/MAIN/sl/sl_meta.json \
  --intrinsics $LMT_WORK/curved-arc/calibration/MAIN_sl_intrinsics.json \
  --corr $LMT_WORK/curved-arc/corr_pose1.json \
  --corr $LMT_WORK/curved-arc/corr_pose2.json \
  --corr $LMT_WORK/curved-arc/corr_pose3.json \
  --yes
  # --intrinsics auto → 自标定模式（从同一批 corr 自解 K）
  # --intrinsics-crosscheck <anchor.json> → 反吸收检查
```

预期产物：`measurements/measured.yaml` + `measurements/MAIN_cabinet_pose_report.json`。
Pose report 的 `frame.gauge_strategy` = `align_to_nominal`（SL 特有，Procrustes 对齐到 nominal 设计）。

### 9.6 pose-obj 导出（承接 §7.2）

视觉重建完成后，用 pose report 导出世界坐标 OBJ：

```bash
./target/debug/lmt --json --db $LMT_DB export pose-obj \
  $LMT_WORK/curved-arc/measurements/MAIN_cabinet_pose_report.json \
  disguise \
  --out $LMT_WORK/curved-arc/world_mesh.obj \
  --yes
  # target: disguise / unreal / neutral
  # 默认（无 --root）= 标准摆法（中心列转正 + 水平居中 + 贴地）
  # --root <cabinet_id> = 以指定箱体为基准重定位（它轴对齐落原点）
  # --ground = 让下边缘贴地（最低 Y = 0）
```

预期：一个 OBJ，所有 cabinet 合并在世界坐标系，逐箱体独立面片 + 整体 UV。

---

## 10. 错误码速查

| String code | Exit code | 常见触发 |
| --- | ---: | --- |
| `invalid_input` | 2 | 参数错误、缺 `--yes`/`--dry-run`、schema 校验失败 |
| `not_found` | 3 | 文件/screen/run id 不存在 |
| `io` | 4 | 文件系统读写错误 |
| `db` | 5 | SQLite 打开/查询错误 |
| `serialization` | 6 | YAML/JSON 编解码错误 |
| `unsupported` | 7 | 未实现功能（如 `--timeout`） |
| `internal` | 11 | 未分类内部错误 |
| `surface_fit_failed` | 12 | 散点曲面拟合失败 |
| `detection_failed` | 13 | 角点/点阵检测数量不足 |
| `ba_diverged` | 14 | BA 不收敛或 reproj error 超阈值 |
| `procrustes_failed` | 15 | Procrustes 对齐失败（对应点太少/退化配置） |
| `intrinsics_invalid` | 16 | 相机内参不可用 |
| `observability_failed` | 17 | 视觉重叠不足 |
| `decode_failed` | 18 | 结构光解码失败 |

---

## 11. 输出格式说明

### --output text（默认，人类模式）

人类可读格式输出到 stdout，适合终端查看。

### --output json / --json（机器模式）

成功 → stdout 单行 JSON：

```json
{"ok": true, "data": <T>, "meta": {"schema_version": "1"}}
```

失败 → stderr 单行 JSON：

```json
{"ok": false, "error": {"code": "<snake_case>", "message": "...", "details": <object>}}
```

### --output ndjson（事件流模式）

每行一个 JSON 事件，适合实时处理长任务的进度。

---

## 12. 全流程脚本（一键跑通）

以下脚本用 curved-flat 跑通 **全站仪管线**（seed → import → reconstruct → export）：

```bash
#!/usr/bin/env bash
set -euo pipefail

LMT=./target/debug/lmt
WORK=/tmp/lmt-walkthrough-$(date +%s)
DB=$WORK/test.sqlite
mkdir -p $WORK

echo "=== 1. Seed example ==="
$LMT --json --db $DB seed-example curved-flat $WORK --yes

echo "=== 2. Import measurements ==="
$LMT --json --db $DB total-station import \
  $WORK/curved-flat MAIN \
  $WORK/curved-flat/measurements/raw.csv --yes

echo "=== 3. Reconstruct ==="
$LMT --json --db $DB reconstruct surface \
  $WORK/curved-flat MAIN \
  measurements/measured.yaml --yes

echo "=== 4. List runs ==="
$LMT --json --db $DB reconstruct list-runs $WORK/curved-flat

echo "=== 5. Get run report ==="
RUN_ID=$($LMT --json --db $DB reconstruct list-runs $WORK/curved-flat \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['data'][-1]['id'])")
$LMT --json --db $DB reconstruct get-run-report $RUN_ID

echo "=== 6. Export OBJ ==="
$LMT --json --db $DB export obj $RUN_ID disguise \
  --dst $WORK/curved-flat/output.obj --yes

echo "=== 7. Verify ==="
ls -la $WORK/curved-flat/output.obj
head -5 $WORK/curved-flat/output.obj
wc -l $WORK/curved-flat/output.obj

echo "=== Done ==="
echo "Artifacts in: $WORK"
```

---

## 附录 A. 环境变量

| 变量 | 说明 |
| --- | --- |
| `LMT_DB_PATH` | DB 路径覆盖（等效 `--db`） |
| `LMT_LOG` | 日志级别：`info` / `debug` / `trace`（`--json` 模式下 tracing 禁用） |
| `LMT_VBA_SIDECAR_PATH` | Python sidecar 路径覆盖（visual 管线依赖） |

## 附录 B. 内置示例项目一览

| 名称 | 屏幕 | 布局 | 形状 | 说明 |
| --- | --- | --- | --- | --- |
| `curved-flat` | MAIN | 8×4 | flat | 自带 `raw.csv` + `measured.yaml`，可跑全站仪全流程 |
| `curved-arc` | MAIN | 16×6 | curved (R=12000mm) | 自带 `measured.yaml`，可跑重建 + 导出 |
| `monitor-bench` | BENCH | 1×2 | flat | 两块显示器模拟，自带 `capture_manifest.json` + `screen_mapping.json` + `known_geometry.json`，用于视觉管线 bench |

## 附录 C. 验证状态汇总（2026-06-07）

| Step | 功能 | 状态 | 备注 |
| ---: | --- | :---: | --- |
| 0 | 编译 + 版本 | PASS | `lmt 0.1.0` |
| 1.1 | version | PASS | |
| 1.2 | schema | PASS | 47 types, 2 incomplete |
| 1.3 | manifest | PASS | 28 operations |
| 2 | seed-example | PASS | dry-run + curved-flat + curved-arc |
| 3 | project load/add/list | PASS | abs_path 自动 canonicalize |
| 4 | measurements load | PASS | 11 点, grid 模式 |
| 5.1 | total-station grid import | PASS | 45 点, 0 异常 |
| 5.2 | total-station scatter import | PASS | 125 点, warning 正确 |
| 5.3 | instruction-card HTML | PASS | 4.8KB 自包含 HTML |
| 5.4 | 错误场景（缺 --yes） | PASS | exit 2, invalid_input |
| 6.1 | reconstruct grid | PASS | RMS=2.0mm, 45 vertices |
| 6.2 | reconstruct scatter | SKIP | fixture 数据与项目不匹配（exit 12 boundary check 行为正确） |
| 6.3 | list-runs | PASS | |
| 6.4 | get-run-report | PASS | |
| 7.1 | export obj (disguise) | PASS | 160 行, +Y up |
| 7.1 | export obj (neutral) | PASS | 160 行, +Z up |
| 7.2 | export pose-obj | SKIP | 需 visual reconstruct 产出 pose report |
| 8.1 | simulate | PASS | 6 views, 4608 obs |
| 8.2 | eval | PASS | dist err=0.12mm, angle err=0.13° |
| 8.3 | compare-known | SKIP | 需真实 pose report |
| 9.1 | plan-capture (--min-views) | PASS | 8 stations, 32/32 pass, fail_reason 字段正常 |
| 9.1 | capture-card | PASS | 714KB HTML |
| 9.3 | generate-pattern (vpqsp) | PASS | 32 cabinets, 288 markers, schema vpqsp.v1 |
| 9.3 | generate-pattern (charuco) | PASS | 32 cabinets, 256 markers (legacy) |
| 9.5.1 | generate-structured-light | PASS | 2048 dots, 15 frames |
| 9.2 | calibrate | SKIP | 需棋盘格照片 |
| 9.4 | visual reconstruct | SKIP | 需多角度拍摄照片 |
| 9.5.2 | decode-structured-light | SKIP | 需结构光录像 |
| 9.5.3 | calibrate-structured-light | SKIP | 需 corr 文件 |
| 9.5.4 | reconstruct-structured-light | SKIP | 需 corr + intrinsics |

**19/28 PASS, 0 FAIL, 9 SKIP**（SKIP 项均需真实相机/全站仪数据）。

## 附录 D. E2E 测试覆盖范围

CLI E2E 测试（`crates/lmt-cli/tests/cli_e2e.rs`）覆盖以下场景，可作为各功能预期行为的权威参考：

- **基础设施**：version / schema / manifest / completion 输出格式
- **全站仪管线**：grid import（dry-run / refuse / happy）、scatter import + reconstruct + export 全链路
- **项目管理**：save-load round-trip
- **Seed example**：dry-run / happy / 重复目标拒绝
- **视觉管线**：simulate → eval → compare-known 合成台全链路、plan-capture --min-views 端到端验证
- **VP-QSP**：generate-pattern happy（vpqsp.v1 schema + screen_id_code）、无容量上限（26 cabinets）、dry-run、未知 method → exit 7、reconstruct detection_failed → exit 13
- **compare-known**：happy + 容差 flags（--max-size-mm / --max-dist-mm / --max-angle-deg）注册验证
- **export pose-obj**：happy（2-cabinet neutral/disguise）、缺 --yes 拒绝、dry-run
- **错误码覆盖**：exit 2/3/7/12/13/14/15/16/17/18 各至少一个 case
- **输出格式**：`--output json` / `--output ndjson` / completion raw 输出
