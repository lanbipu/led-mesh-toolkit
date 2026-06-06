# 小段弧 · 结构光重建端到端验证配方（disguise 闭环）

> 目标：在 disguise 里搭一**小段弧**（同曲率、少列），跑完整条
> `generate → 播放/拍摄 → decode → reconstruct → compare-known`，在**已知真值**下
> 验证"结构光闪码 + 多机位 BA 反算弧形 LED 屏"的几何/数学和整条软件链是否正确。
>
> 为什么先做小段：当前 `estimate_nonroot_cabinet_init`（`reconstruct.py:532`）**只支持
> 直接跟 root 桥接、无 transitive bridging**，非桥接箱体旋转初值被写死 identity。整条
> 60m / 90–180° 弧远端箱体转了 ~90°，从 identity 收不回来 → 现在做不了全屏。**小段弧
> 关键在于：一个机位就能同时拍到 root + 全部箱体**，于是每个箱体都拿得到 PnP bridge
> 初值，BA 干净收敛——既验到了曲率（多列 fan），又避开了 bridging 缺口。验过之后再去补
> transitive bridging 上全屏。详见 memory `project_sl_curved_wall_bridging_blocker`。

---

## 0. 验证什么 / 不验证什么

**验**：几何 + BA 数学正确性；整条 decode→reconstruct→compare 软件链；1.5° 曲率能否恢复。
**不验**（这是最佳情况，留给真机）：真实镜头畸变+标定误差、相机噪声/卷帘、LED bloom/视角
衰减、环境光、运动模糊、真实 pitch 不均匀。

---

## 1. 段尺寸与参数（worked example，把 mm/px 换成你的真实箱体规格）

| 参数 | 取值（示例） | 说明 |
| --- | --- | --- |
| `cabinet_count` | `[9, 5]` | 9 列展开曲率、5 行给竖向，共 45 箱；约 16:9 便于填满相机画面 |
| `cabinet_size_mm` | `[500, 500]` | **填你真实的发光区尺寸** |
| `pixels_per_cabinet` | `[256, 256]` | **填你真实每箱 LED 数**；须能整除 |
| `screen_resolution`（自动） | `[2304, 1280]` | = cols×px, rows×px |
| `radius_mm` | `19099` | **= cabinet_width_mm / (1.5 × π/180)** = 500 / 0.0261799 |

> 曲率半径公式：`radius_mm = cabinet_width_mm / radians(每列角度°)`。每列 1.5°、500mm 箱
> → 19099mm。这是**真实墙的同一个半径**，小段只是少几列——所以验小段=验真实曲率。
> 约束 `radius ≥ 0.6 × 半屏宽`，小段轻松满足。

---

## 2. 建 project（只需手写 project.yaml，不需要 screen_mapping.json）

`lmt seed-example curved-arc ./arc-seg --yes` 拿模板，然后把 `arc-seg/curved-arc/project.yaml`
的 screen 改成：

```yaml
project:
  name: Arc-Segment
  unit: mm
screens:
  MAIN:
    cabinet_count: [9, 5]
    cabinet_size_mm: [500, 500]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: curved
      radius_mm: 19099
      fold_seams_at_columns: []
    shape_mode: rectangle
    irregular_mask: []
coordinate_system:
  origin_point: MAIN_V000_R000
  x_axis_point: MAIN_V008_R000
  xy_plane_point: MAIN_V000_R004
output:
  target: disguise
  obj_filename: "{screen_id}_mesh.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
```

> SL reconstruct 的箱体几何全部来自 sl_meta（generate 从 project.yaml 的 uniform
> pixels_per_cabinet 算出），**不读 screen_mapping.json**（已核对 sidecar 零引用）。

---

## 3. 生成结构光 pattern

```bash
lmt visual generate-structured-light <项目根目录> MAIN --yes
```
**裸命令即可**——不用传 `--margin`/`--dot-spacing`/`--seq-format`，也不用设任何环境变量。

产物在 `patterns/MAIN/sl/`：
- `frames/frame_0000.png … frame_0015.png` —— **16 个逻辑帧**
  （顺序：白 sentinel → all-on anchor → 13 个 code 帧 → 白 sentinel；2880 dots → data_bits=12 → total_bits=13 → 13+3=16）
- `MAIN.seq/MAIN_00000.tif … MAIN_00015.tif` —— **disguise 直接可吃的 TIFF image sequence**
  （未压缩 24-bit、II 小端、从 0 连号）。`output.target=="disguise"` 时由 `--seq-format auto`（默认）自动产出。
  **拷整个 `MAIN.seq` 进 disguise VideoFile 即可。**
- `sequence.mp4`（本流程**不用**）
- `sl_meta.json` —— 后面 decode/reconstruct 都引用它

> **点阵/格式/sidecar 全自动**（底层已做进算法）：`--margin`/`--dot-spacing` 不传则按箱体分辨率
> 自动推导（margin≈1/16、spacing≈1/8 箱体短边 → 任何屏铺成 ~8×8 满格）；`--seq-format auto` 在
> target=disguise 时自动出 TIFF `.seq`；`locate_sidecar` 自动用 venv 的当前 sidecar。要覆盖就显式传
> `--margin/--dot-spacing/--seq-format`。`hold_ms / fps` 仍是 sidecar 硬编码默认。

---

## 4. disguise 侧：建弧、贴图、布机位、拍摄

**4.1 建 LED mesh**：按 §1 同一半径（19099mm）、1.5°/列搭 9×5 弧形屏。**物理尺寸必须跟
project.yaml 一致**——重建的绝对尺度只来自 pitch（project/sl_meta），尺寸不一致会"假性不符"。

**4.2 内容 1:1 贴到 mesh**：内容画布 = `2304×1280`，每个箱体的 256×256 区域**精确**映射到
该箱体 LED，**零缩放/letterbox/warp**。否则 `dot id → 屏幕(u,v) → 3D点` 的不变量被破坏。

**4.3 虚拟相机（纯 pinhole）**：
- 关掉 **景深 / bloom / 运动模糊 / 一切后期**；环境**纯黑**。
- 分辨率示例 `3840×2160`，水平 FOV 取你 disguise 的真实值（示例 40°）。
- 写 `intrinsics.json`（**确认 disguise 的 FOV 是水平/垂直/对角，用对应公式**）：
  ```json
  {
    "K": [[5275.6, 0, 1920], [0, 5275.6, 1080], [0, 0, 1]],
    "dist_coeffs": [0, 0, 0, 0, 0],
    "image_size": [3840, 2160]
  }
  ```
  `fx = fy = (W/2)/tan(HFOV/2) = 1920/tan(20°) = 5275.6`；`cx=W/2, cy=H/2`；畸变全 0。

**4.4 布机位（≥4，建议 6）**：全部能**同时看到整段** 9×5（这是 bridge 初值的前提）。
示例：距离 ~7m，水平 −3 / −1.5 / 0 / +1.5 / +3 m，再加 1–2 个不同高度，都看向段中心。
拉开角度差、别小基线（参照已有合成测试 ±1200mm @ 3500mm）。

**4.5 帧精确拍摄（你已确认能做）**：逐帧把 pattern 的 15 个逻辑帧贴到屏上，每帧截一次虚拟
相机 → 每个机位 **15 张 PNG**，命名让字典序=时间序（`pose1/frame_0000.png … 0014.png`）。
导出分辨率**正好 3840×2160**（== intrinsics.image_size，reconstruct 会强校验）。

> ⚠️ 两个必看：① **白 sentinel 帧要占满画面**——decoder 用整帧平均亮度 `>0.85×255` 找
> sentinel，且阈值**不暴露 CLI**；屏没填满会报 `could not find two white sentinel frames`。
> 纯黑背景 + 紧框 + ~16:9 段形通常能过；过不了就给 decode 加个 `--sentinel-threshold`
> 小补丁。② **anchor 帧里所有点要清晰分离**——decoder 的点图像坐标全取自 all-on anchor 帧；
> 4K 相机 + 无 bloom + 别太斜，确保 25×45 个点不粘连。

---

## 5. decode（每机位一次）

```bash
lmt visual decode-structured-light ./captures/pose1 \
  --sl-meta ./arc-seg/curved-arc/patterns/MAIN/sl/sl_meta.json \
  --out ./corr/pose1.json --yes
# pose2 … pose6 同样
```
- 传**目录**（15 张 PNG）→ 走 canonical 解码（区间帧数==total_bits+1），时序零歧义。
- 产物 `corr/poseN.json` 含 `camera_image_size`（取自帧）、`screen_id`、`sl_meta_sha256`、
  每点 `id/u/v(屏幕)/x/y(相机)`。

---

## 6. reconstruct

```bash
lmt visual reconstruct-structured-light ./arc-seg/curved-arc MAIN \
  --sl-meta ./arc-seg/curved-arc/patterns/MAIN/sl/sl_meta.json \
  --intrinsics ./intrinsics.json \
  --corr ./corr/pose1.json --corr ./corr/pose2.json \
  --corr ./corr/pose3.json --corr ./corr/pose4.json \
  --corr ./corr/pose5.json --corr ./corr/pose6.json --yes
```
产物：
- `measurements/measured.yaml`
- `measurements/MAIN_cabinet_pose_report.json` —— 每箱 `cabinet_id`(=`V000_R000`,**无 MAIN_ 前缀**)
  / `position_mm` / `rotation_matrix` / `normal` / `corners_mm` / `reprojection_rms_px` / `quality`。

> provenance gating：所有 corr 必须同一 `screen_id` + 同一 `sl_meta_sha256`，且 ==
> project.screen_id 与实际 sl_meta 的哈希。全程用同一个 sl_meta 就行。

---

## 7. 真值 known.json（从你的弧几何算，key 用 `V{col:03d}_R{row:03d}`，无前缀）

```json
{
  "cabinets": {
    "V000_R000": {"size_mm": [500, 500]},
    "V001_R000": {"size_mm": [500, 500]},
    "...": "全部 45 个箱体",
    "V008_R004": {"size_mm": [500, 500]}
  },
  "pairs": [
    {"a": "V000_R000", "b": "V001_R000", "distance_mm": 500.0,  "angle_deg": 1.5},
    {"a": "V000_R000", "b": "V008_R000", "distance_mm": 3992.7, "angle_deg": 12.0},
    {"a": "V000_R000", "b": "V000_R001", "distance_mm": 500.0,  "angle_deg": 0.0}
  ]
}
```

真值计算（R=19099，1.5°/列）：
- **相邻列法线夹角** = `1.5°`（最关键——直接验曲率恢复）；隔 k 列 = `k×1.5°`。
- **同行列间中心弦距** = `2R·sin(Δ角/2)`：相邻列 `2·19099·sin(0.75°)=500.0`；隔 8 列
  `2·19099·sin(6°)=3992.7`。
- **同列相邻行**：法线夹角 `0°`，距离 = `cabinet_height_mm = 500`。
- disguise 弧用同一 R 搭 → 真值 == 设计值。

---

## 8. compare-known + 判据

```bash
lmt visual compare-known ./arc-seg/curved-arc/measurements/MAIN_cabinet_pose_report.json ./known.json
```
内置 PASS 阈值：**size ≤ 2.0mm，distance ≤ 3.0mm，angle ≤ 0.3°**。
输出 `passed: true` 即整条链在最佳情况下复原了段形状/尺度/曲率。

**"可行"判据（全满足）**：
1. compare-known `passed: true`（尤其相邻列 `angle_error_deg` 远小于 0.3°）。
2. pose report 每箱 `reprojection_rms_px < ~1px`、`quality == "ok"`、无 `cabinet_quality` warning。
3. （可选）`lmt export pose-obj …/MAIN_cabinet_pose_report.json disguise --out recon.obj`
   导回 disguise 跟真值 mesh 叠一眼。

> 小段不必做逐点 3D 误差（那是全弧累积漂移才需要的，要外部刚体对齐）。compare-known 的
> 坐标系无关三量（尺寸/距离/角度）对小段足够。

---

## 9. 过了之后

- **补 transitive / 链式桥接初始化** → 才能上全 60m 弧；上全弧时改用**逐点 3D 误差**
  （全局对齐后量每点 mm、画热力图）抓累积漂移，compare-known 的相邻对量抓不到全局漂。
- 再上真机，覆盖畸变/噪声/bloom 等本配方不验的真实因素。

---

## 附：踩坑清单（每条对应已核对的代码事实）

1. 白 sentinel 必须占满画面（整帧均值 `>0.85×255`，阈值未暴露 CLI）。
2. 点图像坐标取自 all-on anchor 帧 → 该帧所有点须分离、无 bloom/DOF、勿过斜。
3. 内容 1:1 贴 mesh，画布 2304×1280，每箱 256×256 区域精确对位，零重采样。
4. 拍摄帧尺寸 == intrinsics.image_size（reconstruct 强校验）。
5. 帧精确：每逻辑帧一张截图（15 张），目录输入 → canonical 解码。
6. known.json key = `V000_R000`（无 `MAIN_`）；reconstruct 不需要 screen_mapping.json。
7. disguise 弧用 project.yaml 同一 radius 搭，真值=设计，尺度才对齐（尺度只来自 pitch）。
8. 所有命令 destructive，要 `--yes`（generate/decode/reconstruct 都 gated）。
