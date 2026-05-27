# 显示器台架验证协议 + 报告模板（零全站仪）

> 使用方法：把本文件重命名为 `YYYY-MM-DD-monitor-bench-report.md`，填写所有 `<<...>>` 占位符，随原始照片和 JSON 产物一起 commit。

---

## 背景

本台架用**两台已知规格的显示器**作为真值来源，验证 `lmt visual` 命令链的几何精度（spec §11）。无需全站仪：显示器规格书给出像素间距、面板物理尺寸；卷尺量两台中心距和夹角。

**本次验证覆盖**：相机标定质量、ChArUco 检测率、capture\_manifest / screen\_mapping / CLI 端到端链路、重建几何的尺度准确性（每台尺寸）、相对位移（中心距）、相对旋转（法向夹角）。

**本次验证不覆盖**（诚实边界，spec §11 §18）：LED bloom / 摩尔纹 / 远距离拍摄 / 显示器以外的非平面形变。这些在真实 LED 测试中另行覆盖。

---

## 1. 台架设置

### 1.1 显示器规格

| 项目 | 显示器 A（V000\_R000） | 显示器 B（V000\_R001） |
|---|---|---|
| 厂商型号 | `<<型号>>` | `<<型号>>` |
| 面板对角线 | `<<in>>` 英寸 | `<<in>>` 英寸 |
| 原生分辨率（全屏） | `<<W×H>>` px | `<<W×H>>` px |
| 规格像素间距 | `<<x_pitch>> × <<y_pitch>>` mm | `<<x_pitch>> × <<y_pitch>>` mm |
| 规格有效显示面积 | `<<W×H>>` mm | `<<W×H>>` mm |

> **像素间距推导**（若规格书只给分辨率和对角线）：
> `pitch_x = active_width_mm / native_width_px`，
> `pitch_y = active_height_mm / native_height_px`。
> 若规格书直接给 pitch，以规格书为准。

### 1.2 ChArUco 显示区域（全屏 + per-cabinet pitch 方案）

> 本台架使用 `--screen-mapping` 方案：每块 cabinet 独立指定 `resolution_px`、`active_size_mm`、`pixel_pitch_mm`，生成器（`generate-pattern`）自动用 `choose_board_shape` 选出合适的 `squares_x × squares_y`，使每格物理上是正方形并铺满整个显示区域。**不再需要手动裁剪正方形子区域。**

**原理**：`charuco_corner_local_mm()` 使用 `square_px × pixel_pitch_mm` 推导每个角点的物理坐标，与棋盘是否为正方形无关——16×9 的棋盘（每格仍是正方形）和 9×9 的棋盘精度一样。

**操作**：

1. 按规格书填写每台显示器的 `resolution_px`（原生分辨率）、`active_size_mm`（有效显示面积）、`pixel_pitch_mm`（规格 pitch 或 active_size / resolution 推导）。
2. 用 `generate-pattern --screen-mapping` 生成图案。每台显示器会得到一个填满全屏的棋盘图（如 2560×1440 和 3840×2160），用 `show_pattern.py` 全屏显示即可。
3. 理想情况下，用卷尺或游标卡尺**实测** `active_size_mm`（规格 pitch × 分辨率会有 OS 缩放或 overscan 误差）。若条件受限，用规格书值并在报告里注明。

填写下表：

| 项目 | 显示器 A（V000\_R000） | 显示器 B（V000\_R001） |
|---|---|---|
| 原生分辨率（px） | `<<W_A × H_A>>` | `<<W_B × H_B>>` |
| 规格 pixel\_pitch（mm） | `<<px_A>>` | `<<px_B>>` |
| active\_size\_mm（规格/实测，mm） | `<<W_A_mm × H_A_mm>>` | `<<W_B_mm × H_B_mm>>` |
| 来源 | 规格书 / 实测（选填） | 规格书 / 实测（选填） |

> **关于 known\_geometry.json 的 size\_mm**：填各台的实测（或规格）有效面积短边和长边。由于棋盘格已是非正方形，size\_mm 也应填 `[width_mm, height_mm]`（两方向可不同）。

### 1.3 系统要求（缺任意一项 → 重建结果无意义）

- [ ] 操作系统显示缩放设置为 **100%**（无 HiDPI 缩放）。
- [ ] 确认显示器工作于**原生分辨率**（不插值）。
- [ ] ChArUco 图案在正方形区域内 1:1 显示，**无任何旋转、镜像**。
- [ ] 拍摄时显示器亮度适中，无过曝/欠曝，图案对比度清晰。

### 1.4 两台显示器布置

**本台架的实际摆位**：两台**尺寸不同**的显示器竖直立在桌面，像翻开的书一样**绕竖直轴**张开成一个**钝角（开合角 ≈ 120°，非精确值）**；两台**不贴合**，内侧边缘之间留有**几厘米间隙**。

> 这三点（不等大 / 钝角折叠 / 有间隙）都**不影响**台架成立——重建用的是每台各自的实测尺寸、两台中心的 3D 直线距离、两台法向夹角，BA 自由解每块板的真实位姿。下面把它们如实量出来即可。

#### 角度约定（务必读，否则真值填反）

`compare-known` / `known_geometry.angle_deg` 用的是 **两块屏的法向夹角**，**不是**开合角：

```
angle_deg（两法向夹角） = 180° − 开合角 φ
例：开合角 φ ≈ 120°  →  angle_deg ≈ 60°
    完全共面（并排）   →  angle_deg = 0°
```

填表时填的是 **angle_deg（法向夹角）**。先量开合角 φ，再换算。

#### 怎么量这个折角（绕竖直轴，倾角仪无效）

倾角仪只能测对水平面的倾角，**测不到绕竖直轴的开合角**。用下面任一种：

- **方法 A（够用）**：把数字角度尺 / 手机角度 App 的两臂**平贴桌面**，分别靠住两台显示器的**底边**，直接读开合角 φ。精度约 **±1–2°**。
- **方法 B（更准、可重复）**：用钢板尺在桌面上量出两台显示器**底边四个角点**的平面坐标（各 2 点），两条底边方向的夹角即开合角 φ。卡尺级可做到 **±0.5° 以内**。要把角度真值做紧时用这个。

#### 布置实测表

| 项目 | 值 |
|---|---|
| 布置方式 | 绕竖直轴钝角折叠（开合 ≈ 120°，非精确） |
| 开合角 φ 实测（°） | `<<φ>>` ± `<<不确定度>>`（方法 A 或 B） |
| **angle_deg = 180 − φ（°）** | `<<angle_deg>>`（填进 known_geometry） |
| A↔B 中心 3D 直线距实测（mm） | `<<distance_mm>>` ± `<<不确定度>>`（钢板尺，穿过间隙与折角，量两正方形区域中心点的直线距离） |
| 内侧边缘间隙实测（mm，参考） | `<<gap_mm>>` |
| 两台是否同一桌面（参考，非真值） | `<<是 / 否>>` |

> **关键诚实点（真值精度框定能验到多紧）**：手测角度只有 ±1–2°（方法 A）、手测中心距只有 ±1–2mm，**这就是真值的精度上限**。所以本台架的**主指标是「每台尺寸误差」**——卡尺实测 S 可到 <0.5mm，对它验 §10.3 的 size ≤ 2mm 是有意义的紧检验。而**夹角 / 距离只能做粗校验**：重建值与手测真值的差应落在「真值不确定度 + 工具精度」之内，**不能**直接拿来对 §10.3 的 angle ≤ 0.3°（除非用方法 B 把角度真值做到优于 0.3°，手工通常做不到）。详见 §7.4 注。

### 1.5 相机参数

| 项目 | 值 |
|---|---|
| 机身型号 | `<<型号>>` |
| 镜头焦距 | `<<mm>>` |
| ISO | `<<>>` |
| 快门 | `<<>>` |
| 光圈 | f/`<<>>` |
| 对焦模式 | **手动**（整个 session 锁焦，不得改动） |
| 分辨率 | `<<W×H>>` px |

---

## 2. screen\_mapping 推导

按 §1.2 量出的数据填写 `examples/monitor-bench/screen_mapping.json`（或你的项目目录内的副本）：

```json
{
  "screen_id": "BENCH",
  "cabinets": [
    {
      "cabinet_id": "V000_R000",
      "resolution_px": [<<W_A>>, <<H_A>>],
      "active_size_mm": [<<W_A_mm>>, <<H_A_mm>>],
      "pixel_pitch_mm": [<<pitch_A>>, <<pitch_A>>],
      "active_origin": "center",
      "input_rect_px": [0, 0, <<W_A>>, <<H_A>>],
      "rotation": 0,
      "mirror_x": false,
      "mirror_y": false
    },
    {
      "cabinet_id": "V000_R001",
      "resolution_px": [<<W_B>>, <<H_B>>],
      "active_size_mm": [<<W_B_mm>>, <<H_B_mm>>],
      "pixel_pitch_mm": [<<pitch_B>>, <<pitch_B>>],
      "active_origin": "center",
      "input_rect_px": [0, <<H_A>>, <<W_B>>, <<H_B>>],
      "rotation": 0,
      "mirror_x": false,
      "mirror_y": false
    }
  ],
  "expected_pattern_hash": "REPLACE_WITH_PATTERN_HASH"
}
```

> `input_rect_px` 格式：`[x, y, width, height]`（left-top 原点，px）。B 从 (0, H_A) 起排（竖排）。两台分辨率可以不同。
>
> **重要**：这个组装屏虚拟坐标**只用于 BA 的标称初值与 cabinet 命名**，**不代表也不约束两台的真实摆位**。真实的相对位置（含**几厘米间隙**）与相对朝向（含 **~120° 折角**）由 BA 从照片里**自由解算**（每箱体 SE3 是自由参数，gauge 只钉根箱体；见 spec §3/§7）。所以这里两块虚拟相邻、共面，与现实折叠 + 留缝**不矛盾**。
>
> 唯一要留意的：标称初值是「共面相邻」，而你的真实折角约 60°（法向夹角）偏离较大；正常情况下 per-image PnP + BA 能从该初值收敛到真实折角，**若不收敛会报 `ba_diverged`**（见 §8：补足同时看到两台的视角即可）。

---

## 3. expected\_pattern\_hash 填写方法

`reconstruct` 在运行前会用 preflight 校验：

```python
actual_hash = hashlib.sha256(pattern_meta.model_dump_json().encode()).hexdigest()[:16]
```

若 `actual_hash != screen_mapping.expected_pattern_hash`，报 `invalid_input` 并 **不运行**。

**推荐方法（最简单）**：先不填 hash，直接跑一次 reconstruct，它会在错误消息里给出 `got '<actual_hash>'`——把这个值复制回 `screen_mapping.json`：

```bash
# 先把 expected_pattern_hash 填成任意 dummy 值（比如 "REPLACE_WITH_PATTERN_HASH"），
# 然后运行 --dry-run（不写盘，仅校验到 preflight）：
lmt --yes visual reconstruct /path/to/project BENCH \
  --capture-manifest /path/to/project/capture_manifest.json \
  --dry-run

# 若 dry-run 不触发 hash 校验（dry-run 只校验到 gate 不运行 Python），
# 则跑一次真实 reconstruct（会失败于 hash mismatch，但会报出正确 hash）：
lmt --yes visual reconstruct /path/to/project BENCH \
  --capture-manifest /path/to/project/capture_manifest.json
# 错误消息形如：
#   Pattern hash mismatch: expected 'REPLACE_WITH_PATTERN_HASH', got 'a3f1b2c4d5e6f789'.
# → 把 'a3f1b2c4d5e6f789' 填到 expected_pattern_hash 里，再跑一次。
```

**备选方法（手动计算）**：hash 是对 `PatternMeta.model_dump_json()` 的内容做 SHA-256 取前 16 位。注意是对 pydantic `model_dump_json()` 的字节序列取 hash，**不是**对原始文件字节取 hash（二者可能因字段顺序不同而不同）。直接从错误消息拿最安全。

---

## 4. 拍摄 SOP

### 4.1 前置步骤

1. **生成 pattern**（在 project 目录内）：

   ```bash
   lmt --yes visual generate-pattern /path/to/project BENCH --method charuco \
       --screen-mapping screen_mapping.json
   ```

   产物：`patterns/BENCH/cabinets/V000_R000.png`、`V000_R001.png`、`full_screen.png`、`pattern_meta.json`（schema v2）。

   > **`--screen-mapping` 是这个台子的关键**：两台显示器尺寸不同，必须先按 §1.2 把
   > 每台的 `resolution_px` / `active_size_mm` / `pixel_pitch_mm` / `input_rect_px`
   > 填进 `screen_mapping.json`，再生成。生成器据此为**每个箱体按其自身点间距/尺寸**
   > 出专属棋盘（支持非正方形、不等尺寸),并把每块棋盘贴到它的 `input_rect_px`
   > 位置。`screen_mapping.json` 必须**恰好**覆盖网格里的每个箱体,缺/多/拼错都会
   > 报 `invalid_input`。**注意:改了 pattern 后 `pattern_meta` 变了 → pattern hash
   > 也变了,必须按 §3 重新获取并回填 `expected_pattern_hash`,否则 reconstruct 会
   > 报 hash mismatch。**

2. **显示 pattern**：把 `V000_R000.png` 全屏（或在正方形区域 1:1）显示在显示器 A；把 `V000_R001.png` 显示在显示器 B。**两台同时显示**。

3. **相机标定**（如未完成）：用同一台相机、同一镜头、同一焦距，对着标定棋盘格（物理打印，不用显示器）拍 15–30 张，拍摄角度多样化（上下左右各倾 ±30°），覆盖画面四角：

   ```bash
   lmt --yes visual calibrate /path/to/project BENCH \
     /path/to/checkerboard_photos \
     --square-mm <<棋盘格方格物理边长mm>> \
     --inner <<WxH，如 9x9>>
   ```

   产物：`calibration/BENCH_intrinsics.json`。

   标定质量门槛：reproj RMS < 0.5 px。如果超过 0.5 px，检查：焦距是否锁定、覆盖是否足够（四角不能空）、棋盘格是否平整。

4. **填写 expected\_pattern\_hash**（见 §3）。

### 4.2 拍摄

- **拍摄数量**：12–20 个机位，每机位一张（两台显示器都清晰可见）。
- **机位布置**：

  | 机位组 | 描述 | 数量 |
  |---|---|---|
  | 正面近距（0.5–1 m） | 尽量填满画面 | 2–3 张 |
  | 正面中距（1.5–2.5 m） | 两台都入画 | 3–4 张 |
  | 正面远距（3 m+） | 整体拍 | 2 张 |
  | 左斜（30–45° 偏左） | 两台都可见 | 2–3 张 |
  | 右斜（30–45° 偏右） | 两台都可见 | 2–3 张 |
  | 俯角（向下 20–30°） | 可选 | 1–2 张 |

- **规则**：拍摄过程中**不得改变**焦距、对焦、缩放；每张照片里两台显示器都要可见（至少一台清晰可见，不重要视角可只见一台，但至少需要足够多视角同时看到两台以保证可观测性）。

- **可观测性要求（spec §12）**：每个 cabinet 需被 ≥ 2 个视角观测到，且至少有 ≥ 8 个有效角点。重叠覆盖越多 BA 越稳定。

- **命名规则**：按 `v001.jpg`、`v002.jpg` … 顺序命名，存入 `captures/` 目录。

### 4.3 capture\_manifest.json 填写

模板见 `examples/monitor-bench/capture_manifest.json`。把 `views` 数组改成实际文件名列表，相对路径以 manifest 所在目录为基准。

```json
{
  "method": "charuco",
  "intrinsics": "calibration/BENCH_intrinsics.json",
  "pattern_meta": "patterns/BENCH/pattern_meta.json",
  "screen_mapping": "screen_mapping.json",
  "views": [
    { "view_id": "v001", "images": ["captures/v001.jpg"] },
    ...
  ]
}
```

---

## 5. 真值入账（known\_geometry.json）

填写 `examples/monitor-bench/known_geometry.json`（或项目目录内副本）：

```json
{
  "cabinets": {
    "V000_R000": { "size_mm": [<<S_A>>, <<S_A>>] },
    "V000_R001": { "size_mm": [<<S_B>>, <<S_B>>] }
  },
  "pairs": [
    {
      "a": "V000_R000",
      "b": "V000_R001",
      "distance_mm": <<实测中心 3D 直线距>>,
      "angle_deg": <<法向夹角 = 180 − 开合角 φ；如开合 120° → 60>>
    }
  ]
}
```

- `size_mm`：填 §1.2 各台**实测正方形边长 S**（每台正方形 → 两方向相同；但 **A、B 的 S 可不同**，分别填各自的）。
- `distance_mm`：填 §1.4 的 **A↔B 中心 3D 直线距**（穿过间隙与折角的直线距离）。
- `angle_deg`：填 §1.4 换算出的**法向夹角 = 180 − 开合角 φ**（例：开合 120° → 填 60°）。**不要直接填开合角**。
- 建议在本文件或报告里同时记下各真值的**实测不确定度**（角度 ±1–2°、距离 ±1–2mm），§7.4 判定时要用到。

---

## 6. 运行命令序列

```bash
PROJECT=/path/to/your/project

# 步骤 1：生成 pattern（如已生成可跳过）。--screen-mapping 必带：按每台显示器
# 自身尺寸/点间距出专属棋盘（见 §4.1），生成后记得按 §3 刷新 expected_pattern_hash。
lmt --yes visual generate-pattern $PROJECT BENCH --method charuco \
    --screen-mapping $PROJECT/screen_mapping.json

# 步骤 2：标定（如已有 intrinsics.json 可跳过）
lmt --yes visual calibrate $PROJECT BENCH \
  /path/to/checkerboard_photos \
  --square-mm <<棋盘格方格边长mm>> \
  --inner 9x9

# 步骤 3：重建
lmt --yes visual reconstruct $PROJECT BENCH \
  --capture-manifest $PROJECT/capture_manifest.json

# 输出（写到 $PROJECT/measurements/）：
#   measured.yaml
#   BENCH_cabinet_pose_report.json

# 步骤 4：对账真值
lmt visual compare-known \
  $PROJECT/measurements/BENCH_cabinet_pose_report.json \
  $PROJECT/known_geometry.json
```

> `compare-known` 是 write\_safe（只读 JSON，不写盘），无需 `--yes`。

**JSON 模式（agent / 脚本使用）**：

```bash
lmt --json visual compare-known \
  $PROJECT/measurements/BENCH_cabinet_pose_report.json \
  $PROJECT/known_geometry.json
```

---

## 7. 结果表

> 填写重建后由 `compare-known` 输出的数值，对照 spec §10.3 nominal 档阈值判断是否通过。

### 7.1 标定质量

| 指标 | 值 | 门槛 | 通过？ |
|---|---|---|---|
| 相机标定 reproj RMS（px） | `<<>>` | < 0.5 px | `<<>>` |
| 标定使用帧数 | `<<>>` | ≥ 10 | `<<>>` |

### 7.2 重建质量

| 指标 | 值 | 门槛 | 通过？ |
|---|---|---|---|
| BA global RMS（px） | `<<>>` | < 1.0 px（参考） | `<<>>` |

### 7.3 cabinet 尺寸误差（spec §10.3 nominal，size ≤ 2 mm）

| cabinet | 重建尺寸 W（mm） | 重建尺寸 H（mm） | 真值 S（mm） | 最大轴误差（mm） | 通过？ |
|---|---|---|---|---|---|
| V000\_R000 | `<<>>` | `<<>>` | `<<S_A>>` | `<<>>` | `<<>>` |
| V000\_R001 | `<<>>` | `<<>>` | `<<S_B>>` | `<<>>` | `<<>>` |

### 7.4 cabinet pair 误差（spec §10.3 nominal，distance ≤ 3 mm，angle ≤ 0.3°）

> **判定说明（本台架真值为手测，必读）**：§10.3 的 distance ≤ 3mm / angle ≤ 0.3° 是**合成台**的指标，前提是真值远比它准。本台架的角度真值（开合角 φ）若用方法 A 量，只有 ±1–2°，**比 0.3° 还粗**——此时「夹角误差」一栏只能判：重建法向夹角与真值之差是否落在 **±真值不确定度** 内（即"没大错"），**不能**当作通过了 0.3° 精度。要真正验到 0.3°，须用 §1.4 方法 B 把角度真值做到优于 0.3°。距离同理（手测 ±1–2mm 时，3mm 阈值是被真值精度限制的粗校验）。**本台架真正紧的检验是 §7.3 的每台尺寸误差（卡尺真值 <0.5mm）。**

| pair | 重建中心距（mm） | 真值（mm） | 距离误差（mm） | 通过？ | 重建夹角（°） | 真值（°） | 夹角误差（°） | 通过？ |
|---|---|---|---|---|---|---|---|---|
| V000\_R000 ↔ V000\_R001 | `<<>>` | `<<distance_mm>>` | `<<>>` | `<<>>` | `<<>>` | `<<angle_deg>>` | `<<>>` | `<<>>` |

### 7.5 总体结论

- [ ] 全部通过 → 台架验证 **PASS**。
- [ ] 存在失败项 → 见 §8 故障排查。

---

## 8. 故障排查清单

如果 `compare-known` 报告某项 fail，按以下顺序检查：

| 症状 | 可能原因 | 排查动作 |
|---|---|---|
| 尺寸误差 > 2 mm | `active_size_mm` 填错 | 重新用卷尺/游标卡尺测 S，重填 screen\_mapping |
| 尺寸误差 > 2 mm | 正方形区域不是正方形（图案被 letterbox） | 检查显示的图案实际像素；确保 N×N px 区域 |
| 尺寸误差 > 2 mm | OS HiDPI 缩放未关闭 | 检查系统显示设置，关闭所有缩放 |
| 尺寸误差 > 2 mm | `pixel_pitch_mm` 推算错 | 重新计算 S/N，不要用规格书 pitch |
| 距离误差 > 3 mm | 真值测量不准 | 用钢板尺重测中心距（精度 ±1 mm 以内） |
| 夹角误差 > 0.3° | 角度真值不够准（手测 φ 只有 ±1–2°） | 先按 §7.4 注判断是否落在真值不确定度内；要更紧用 §1.4 方法 B（底边四角坐标算 φ）。注意填的是法向夹角 = 180 − φ |
| 夹角误差极大（如差 ~60° 或符号反） | angle_deg 填成了开合角 φ 本身 | 应填 180 − φ（开合 120° → 60°），见 §1.4 角度约定 |
| BA 不收敛（ba\_diverged） | 视角太少 / 覆盖不够 | 补充拍摄更多视角，确保两台都被 ≥ 6 视角看到 |
| 检测失败（detection\_failed） | ChArUco pattern 检测不到角点 | 检查图案对比度、是否有反光；重拍在不同光线下 |
| observability\_failed | 某个 cabinet 视角不足 | 补充正对该显示器的近距离拍摄 |
| hash mismatch（invalid\_input） | `expected_pattern_hash` 未更新 | 按 §3 重新获取 hash 后填入 screen\_mapping |
| calibrate RMS > 0.5 px | 标定棋盘格覆盖不够 / 焦距未锁 | 重拍标定图，确保覆盖四角，焦距全程锁定 |

---

## 9. 原始数据存档

运行完成后，把以下文件连同本报告一起 commit（敏感照片可存大文件存储，仅把路径记录在这里）：

- `calibration/BENCH_intrinsics.json`
- `patterns/BENCH/pattern_meta.json`
- `screen_mapping.json`（已填 hash）
- `known_geometry.json`（已填真值）
- `capture_manifest.json`（已填 views）
- `measurements/measured.yaml`
- `measurements/BENCH_cabinet_pose_report.json`
- `compare_known_result.json`（`lmt --json visual compare-known ... > compare_known_result.json`）
- 拍摄照片（大文件，可存 Git LFS 或外部存储，路径记录如下）：

  ```
  照片存放路径：<<路径或存储链接>>
  ```

---

## 附：诚实边界（spec §11）

本台架验证的是：

- 几何（尺寸 / 距离 / 夹角）精度——在清晰的平面显示器上；
- 标定质量和 ChArUco 检测率；
- screen\_mapping + capture\_manifest + CLI 端到端链路；
- BA 收敛性和 per-cabinet 可观测性。

**本台架不覆盖**：

- LED 像素 bloom / 摩尔纹 / 光学散射导致的角点定位误差；
- 真实 LED 屏的非平面形变（箱体安装误差、运输变形）；
- 远距离（3 m 以上）拍摄的尺度退化；
- 大型屏（> 50 cabinet）的 ArUco ID 容量限制。

这些需要在真实 LED 台架 / 现场测试中另行验证（spec §18 风险表）。
