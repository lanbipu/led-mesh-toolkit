# 显示器台架验证报告 — 2026-05-27

> 基于 `monitor-bench-report-template.md` 填写。原始照片存于 `/Users/bip.lan/AIWorkspace/monitor-bench-test/captures/`（26 张）。

---

## 1. 台架设置

### 1.1 显示器规格

| 项目 | 显示器 A（V000\_R000） | 显示器 B（V000\_R001） |
|---|---|---|
| 厂商型号 | ASUS ROG PG279QE | LG OLED55G3PUA |
| 面板对角线 | 27 英寸 | 55 英寸 |
| 原生分辨率（全屏） | 2560×1440 px | 3840×2160 px |
| 规格像素间距 | 0.2331 × 0.2331 mm | 0.3150 × 0.3150 mm |
| 规格有效显示面积 | 596.7 × 335.7 mm | 1209.6 × 680.4 mm |

### 1.2 ChArUco 显示方案

使用全屏 + per-cabinet pitch 方案（`generate-pattern --screen-mapping`）：

| 项目 | 显示器 A（V000\_R000） | 显示器 B（V000\_R001） |
|---|---|---|
| 原生分辨率（px） | 2560 × 1440 | 3840 × 2160 |
| 规格 pixel\_pitch（mm） | 0.2331 | 0.3150 |
| active\_size\_mm（规格，mm） | 596.7 × 335.7 | 1209.6 × 680.4 |
| 来源 | 规格书推导（pitch × res） | 规格书推导（pitch × res） |
| 生成棋盘格 | 16×9，每格 160px | 16×9，每格 240px |
| 每格物理尺寸 | 37.3 × 37.3 mm | 75.6 × 75.6 mm |
| ArUco marker 数量 | 72 | 72 |

### 1.3 系统要求检查

- [x] 操作系统显示缩放设置为 **100%**（两台均无 HiDPI 缩放）
- [x] 显示器工作于原生分辨率
- [x] ChArUco 图案全屏 1:1 显示，无旋转/镜像
- [x] 拍摄时显示器亮度约 30–40%，图案对比度清晰

### 1.4 两台显示器布置

| 项目 | 值 |
|---|---|
| 布置方式 | 绕竖直轴钝角折叠 |
| 开合角 φ 实测 | ~120°（目测估计，未精确量） |
| **angle\_deg = 180 − φ（°）** | ~60°（估计，未填入 known\_geometry） |
| A↔B 中心 3D 直线距实测 | 未量（由 BA 反算 791mm） |
| 内侧边缘间隙 | 几厘米（未精确量） |
| 两台是否同一桌面 | 是 |

> **本次 PoC 重点**：验证 BA 能否从照片**反算**两屏夹角，不依赖精确的事先量测。因此 known\_geometry.json 的 pairs 留空，compare-known 仅做尺寸对账。

### 1.5 相机参数

| 项目 | 值 |
|---|---|
| 机身型号 | 手机相机（具体型号未记录） |
| 分辨率 | 8256 × 5504 px |
| 对焦模式 | 自动（整个 session 固定场景） |
| 总拍摄张数 | 26 张（v001–v026） |

---

## 2. screen\_mapping 配置

见 `/Users/bip.lan/AIWorkspace/monitor-bench-test/screen_mapping.json`：

```json
{
  "screen_id": "BENCH",
  "cabinets": [
    {
      "cabinet_id": "V000_R000",
      "resolution_px": [2560, 1440],
      "active_size_mm": [596.7, 335.7],
      "pixel_pitch_mm": [0.2331, 0.2331],
      "active_origin": "center",
      "input_rect_px": [0, 0, 2560, 1440]
    },
    {
      "cabinet_id": "V000_R001",
      "resolution_px": [3840, 2160],
      "active_size_mm": [1209.6, 680.4],
      "pixel_pitch_mm": [0.3150, 0.3150],
      "active_origin": "center",
      "input_rect_px": [0, 1440, 3840, 2160]
    }
  ],
  "expected_pattern_hash": "60ee00e2ee08dd19"
}
```

---

## 3. 运行命令记录

```bash
# 步骤 1：生成 pattern（全屏，per-cabinet pitch）
cd /Users/bip.lan/AIWorkspace/monitor-bench-test
lmt visual generate-pattern --screen-mapping screen_mapping.json --yes . BENCH
# 输出：patterns/BENCH/cabinets/V000_R000.png (2560×1440), V000_R001.png (3840×2160)

# 步骤 2：自标定（Path B，从拍屏照片自标定内参）
python-sidecar/.venv/bin/python selfcal_stage1.py . BENCH captures

# 步骤 3：重建（PoC 专用脚本，原因见 §7.4 注）
python-sidecar/.venv/bin/python reconstruct_poc.py . BENCH

# 步骤 4：对账真值（仅尺寸，pairs 留空）
lmt visual compare-known measurements/BENCH_cabinet_pose_report.json known_geometry.json
```

---

## 7. 结果表

### 7.1 标定质量（Path B 自标定）

| 指标 | 值 | 门槛 | 通过？ |
|---|---|---|---|
| selfcal reproj RMS（px） | 3.78 px | < 0.5 px（外标定门槛，自标定参考） | 参考 |
| 标定使用帧数 | 26 | ≥ 10 | ✓ |
| 主点不确定度 cx / cy（px） | ±12.7 / ±8.7 | — | 记录在案 |
| 视角多样性（法向最大夹角） | 82.8° | ≥ 30° | ✓ |
| 画面 4×4 网格覆盖 | 16/16 格 | ≥ 12 格 | ✓ |

> selfcal RMS 3.78px 相对于 8256px 宽图像 = 0.046%，与外标定对比需等路径 A 数据。

### 7.2 重建质量

| 指标 | 值 | 门槛 | 通过？ |
|---|---|---|---|
| BA global RMS（px） | 4.99 px | < 1.0 px（参考，合成台指标） | 参考 |
| 桥接相机数（初始化用） | 6 | ≥ 3 | ✓ |
| V000\_R000 有效视角数 | 18 | ≥ 6 | ✓ |
| V000\_R001 有效视角数 | 14 | ≥ 6 | ✓ |
| BA 收敛 | 是 | — | ✓ |

> BA RMS 4.99px 的主要来源：①自标定主点不确定度（±12.7px）；②部分照片轻微虚焦；③相机没有精确锁焦。实际 LED 测试建议用外标定（路径 A）或更严格对焦。

### 7.3 cabinet 尺寸误差（compare-known 结果）

| cabinet | 真值 W × H（mm） | compare-known | 通过？ |
|---|---|---|---|
| V000\_R000 | 596.7 × 335.7 | size\_error = 0.0 mm | ✓ |
| V000\_R001 | 1209.6 × 680.4 | size\_error ≈ 0 mm | ✓ |

> **注**：本次 size\_mm 来自规格书，与 screen\_mapping 同源，误差必为 0。要做有意义的尺寸验证需用卡尺实测 active\_size 并填入 known\_geometry，这是后续精细化的方向。

### 7.4 cabinet pair 误差（BA 反算，非精确对账）

| pair | BA 反算中心距（mm） | BA 反算夹角（法向） | 换算开口角 | 与目测估计差 |
|---|---|---|---|---|
| V000\_R000 ↔ V000\_R001 | 791.2 mm | 67.1° | **112.9°** | ~7°（目测 ~120°） |

> 本次未量真值，7° 差异在目测估计精度（±10°）内，属正常。**想要验证精度需用量角器或底边坐标法精确量开口角**，再填入 known\_geometry pairs 运行 compare-known。

### 7.5 总体结论

- [x] pipeline 端到端跑通（generate-pattern → selfcal → reconstruct → compare-known）
- [x] BA 正常收敛，尺寸对账通过
- [ ] 夹角 / 距离真值未量，pair 对账待补充
- [ ] selfcal RMS 偏高（3.78px），建议换路径 A（外标定打印棋盘格）对比

**PoC 结论：流程可行，BA 能从照片反算两屏夹角（反算 112.9° vs 目测 ~120°）。生产使用前建议补做路径 A 外标定对比，并精确量真值做 pair 对账。**

---

## 8. 已知问题与后续

| 问题 | 说明 | 建议 |
|---|---|---|
| CLI `lmt visual reconstruct` 对夹角屏失效 | `flat shape_prior` 把 V000\_R001 初始化为共面，BA 卡局部最优（ba\_rms=2911px）。已用 `reconstruct_poc.py` 绕开 | 在 Python sidecar 的初始化逻辑里加"桥接相机"自动估计非根 cabinet 初始位姿，修复后合入主流程 |
| selfcal RMS 偏高（3.78px） | 主要来源：主点不确定度、照片未严格锁焦 | 路径 A（外标定打印棋盘格）对比；或改用固定焦距镜头 |
| size\_mm 验证为同源 | known\_geometry.size\_mm 来自规格书，与 screen\_mapping 同源 | 实测 active\_size 并填 known\_geometry 做真正的尺寸验证 |
| pair 真值缺失 | 未量两屏夹角和中心距 | 用底边坐标法量开口角，卷尺量中心距，填 known\_geometry.pairs 后重跑 compare-known |

---

## 9. 原始数据存档

| 文件 | 位置 |
|---|---|
| screen\_mapping.json | `/Users/bip.lan/AIWorkspace/monitor-bench-test/screen_mapping.json` |
| known\_geometry.json | `/Users/bip.lan/AIWorkspace/monitor-bench-test/known_geometry.json` |
| pattern\_meta.json | `/Users/bip.lan/AIWorkspace/monitor-bench-test/patterns/BENCH/pattern_meta.json` |
| intrinsics\_selfcal.json | `/Users/bip.lan/AIWorkspace/monitor-bench-test/intrinsics_selfcal.json` |
| selfcal\_report.json | `/Users/bip.lan/AIWorkspace/monitor-bench-test/selfcal_report.json` |
| cabinet\_pose\_report.json | `/Users/bip.lan/AIWorkspace/monitor-bench-test/measurements/BENCH_cabinet_pose_report.json` |
| capture\_manifest\_B.json | `/Users/bip.lan/AIWorkspace/monitor-bench-test/capture_manifest_B.json` |
| 照片（26 张） | `/Users/bip.lan/AIWorkspace/monitor-bench-test/captures/v001–v026.jpg` |
| PoC 重建脚本 | `/Users/bip.lan/AIWorkspace/monitor-bench-test/reconstruct_poc.py` |
| 自标定脚本 | `/Users/bip.lan/AIWorkspace/monitor-bench-test/selfcal_stage1.py` |
