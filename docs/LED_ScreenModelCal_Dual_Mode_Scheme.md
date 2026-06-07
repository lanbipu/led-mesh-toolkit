# LED 屏幕模型反算与几何校准双模式方案

版本：v1.0  
定位：用于 LED 虚拟拍摄现场，通过相机拍摄 LED 屏幕标定图案，反算现场实际搭建后的 LED 屏幕三维模型，并服务于后续摄影机、镜头、tracking 与 LED 空间几何校正。

---

## 1. 核心目标

LED 虚拟拍摄中的空间对位精度，依赖一个前提：

> 校准系统中的三维 LED 屏幕模型，必须尽可能 1:1 对应现场真实搭建后的 LED 发光面。

实际现场中，CAD / OBJ / 原始 nDisplay mesh 往往与现场存在偏差，例如：

- 主墙整体高度或位置偏差；
- 墙体有轻微倾斜；
- 地屏与主墙夹角不是精确 90°；
- 弧形屏半径与设计值不一致；
- 某些 cabinet 有前后凸起或角度偏差；
- 多块屏幕之间的连接关系与理论模型不同；
- LED processor mapping 与物理屏幕安装存在局部错位。

因此，需要一个可以直接面向 LED 发光面的屏幕模型校正系统。

本方案建议将功能分为两种模式：

```text
1. Quick Screen Fit：快速屏幕模型校正
2. Precision Screen Scan：高精度屏幕三维重建
```

两种模式的关系：

```text
快速模式：用于现场快速修正 CAD / 原始模型。
高精度模式：用于正式建立 as-built LED screen mesh。
```

---

## 2. 总体产品定位

建议产品模块命名为：

```text
ScreenModelCal
```

包含：

```text
ScreenModelCal
├── Quick Screen Fit
│   └── 快速屏幕模型校正
│
├── Precision Screen Scan
│   └── 高精度屏幕三维重建
│
└── Validate Current Mesh
    └── 当前屏幕模型验证
```

其中用户主要选择前两个模式：

```text
Quick Screen Fit      适合快速现场校正
Precision Screen Scan 适合影视工业级深度校准
```

---

## 3. 两种模式总览

| 项目 | Quick Screen Fit 快速模式 | Precision Screen Scan 高精度模式 |
|---|---|---|
| 目标 | 快速修正屏幕位置、角度和大尺度偏差 | 1:1 重建现场 LED 发光面 |
| 使用场景 | 每日开机、现场快速对位、drift check | 搭建完成后、正式拍摄前、hero shot 校准 |
| 图案 | Dense Coded Marker Field + Gaussian Dot | Gray Code + Phase Shift |
| 采集方式 | 摄影机可移动中连续采集 | 多视角固定 pose 采集完整图案序列 |
| 是否需要 CAD | 需要，作为初始模型 | 需要，作为初始拓扑和约束 |
| 是否需要外部尺度基准 | 可选 | 强烈建议 |
| 输出 | 修正后的 quick mesh / transform patch | as-built mesh / UV-to-XYZ map / per-cabinet pose |
| 速度 | 30 秒 – 5 分钟 | 20 分钟 – 数小时 |
| 精度目标 | 2–10 mm 级别 | 相对 0.5–2 mm，绝对 1–3 mm |
| 操作对象 | 现场技术人员 | 校准工程师 / VP 工程师 |
| 核心价值 | 快速进入可拍状态 | 形成可交付、可复现的真实屏幕模型 |

---

# Part A：Quick Screen Fit 快速屏幕模型校正

---

## 4. 快速模式的目标

Quick Screen Fit 的目标不是完整重建每个 LED 像素的三维位置，而是快速修正原始屏幕模型中的主要误差。

它主要解决：

```text
1. 屏幕整体位置不准
2. 屏幕整体角度不准
3. 墙地夹角不准
4. 多块屏幕之间的相对关系不准
5. 局部 panel group 有明显偏移
6. CAD / OBJ 和现场实际搭建结果不一致
```

快速模式的定位：

```text
快速修正大误差，让系统先进入可用状态。
```

---

## 5. 快速模式适用场景

适合：

- 每天开机后的快速检查；
- LED volume 搭建完成后的初步对位；
- 换摄影机位置后的快速校正；
- 换镜头后的 screen alignment 检查；
- 发现虚拟背景与实拍前景有明显错位时快速修正；
- 高精度校准前的初始值生成；
- 高精度校准后的日常 drift check。

不适合：

- 最终交付级 screen mesh；
- 每块 cabinet 级别的毫米级测量；
- 对弧形屏半径、地屏起伏、panel bowing 做最终精度评估；
- 完全替代 Faro / Leica / 全站仪的绝对测绘任务。

---

## 6. 快速模式推荐图案

快速模式推荐使用自研图案：

```text
VP-QSP：Virtual Production Quick Screen Pattern
```

它的核心是：

```text
Dense Coded Marker Field
+
Gaussian Dot
+
Multi-scale Anchor
+
Normal / Inverted Pair
```

也就是：

```text
密集编码标记场 + 高稳定白点中心
```

---

## 7. 为什么不只用普通白点矩阵？

普通白点矩阵的优点是检测稳定，白点中心可以做 subpixel centroid。

但它的问题是：

```text
1. 点本身通常没有强 ID
2. 大 LED wall 上容易出现身份歧义
3. wall + floor + ceiling 多屏组合时容易混淆
4. 摄影机移动中很难快速知道当前看到的是哪块屏幕区域
5. 需要更依赖 tracking 和原始模型去猜测点的身份
```

因此，普通白点矩阵适合作为：

```text
快速验证图案
局部 drift check 图案
特征定位辅助图案
```

但不建议作为大型 LED volume 快速屏幕模型校正的唯一主图案。

---

## 8. 为什么吸收 SP Grid 类图案思路？

SP Grid 类图案的核心价值是：

```text
单帧可识别
局部区域有身份信息
适合大面积 LED wall
适合摄影机移动中连续采集
```

你上传的 SP Grid 演示文本中，流程包含：

- 生成 SP pattern；
- 根据 panel 尺寸设置 pattern size；
- 对 wall / floor 设置 shuffle，避免图案 overlap；
- 摄影机移动中持续采集，不需要停下来；
- 系统实时显示 reprojection error、sensor coverage、screen angle、panel coverage；
- 自动剔除红色低质量数据，优先使用绿色高质量数据；
- 以 lens and tracking calibration 为目标，最终保存 align profile 和 lens profile。

因此，快速模式应该吸收这种操作逻辑：

```text
让摄影机边移动边采集，
系统自动判断哪些帧有用，
实时优化屏幕模型和相机/屏幕关系。
```

---

## 9. VP-QSP 图案结构设计

单个 marker 建议设计为：

```text
外层：粗黑白定位边框
中层：低频 binary ID
内层：orientation bits
中心：Gaussian white dot
角点：subpixel corner feature
冗余：error correction bits
```

### 9.1 外层定位边框

目的：

```text
快速检测 marker 位置
保证旋转、缩放、透视情况下仍可识别
```

要求：

```text
边框不能太细
黑白对比明显
避免过高频细节
```

### 9.2 中层 binary ID

ID 内容建议包含：

```text
screen_id
cabinet_group_id
local_marker_id
orientation
checksum / error correction
```

这样即使只看到局部区域，也可以知道：

```text
这是哪块屏
属于哪个 cabinet group
在屏幕局部坐标中的位置
```

### 9.3 中心 Gaussian dot

这是对 Disguise 白点矩阵优点的吸收。

中心点不要做硬边圆点，而应做：

```text
Gaussian blob
```

原因：

```text
1. centroid 更稳定
2. 抗轻微 defocus
3. 抗 LED bloom
4. 比小黑白格子的角点更适合作 subpixel 定位
```

### 9.4 Normal / Inverted Pair

快速模式至少播放两帧：

```text
VP-QSP normal
VP-QSP inverted
```

这样可以提高：

```text
曝光鲁棒性
反光鲁棒性
黑栅影响判断
低对比区域剔除
```

---

## 10. VP-QSP 布局设计

不要规则重复排列。

推荐：

```text
blue-noise / shuffled layout
```

布局原则：

```text
1. 相邻 marker 的 ID 和局部邻域不能重复
2. wall / floor / side wall / ceiling 之间不能出现相似局部纹理
3. screen corner、wall-floor seam、弧形切线区域要增加特征密度
4. 大尺寸 anchor 用于远景和初始定位
5. 中尺寸 marker 用于主求解
6. Gaussian dot 用于 subpixel 精定位
```

多尺度布局：

```text
Large Anchor：每 2–4 米一个
Medium Coded Marker：主采样层
Gaussian Dot：中心定位层
Seam Marker：边界、转角、cabinet seam 加密
```

---

## 11. 快速模式播放序列

推荐基础序列：

```text
01  Black
02  White / 50% Gray
03  VP-QSP Normal
04  VP-QSP Inverted
05  Gaussian Dot Only
06  Optional Full-screen Flash
```

极简序列：

```text
01  VP-QSP Normal
```

工程推荐最小序列：

```text
01  Black
02  White
03  VP-QSP Normal
04  VP-QSP Inverted
```

这样仍然很快，但比单帧图案更稳定。

---

## 12. 快速模式采集流程

现场流程：

```text
1. 导入 CAD / OBJ / nDisplay mesh / LED topology
2. 输入 cabinet 尺寸、pixel pitch、screen mapping
3. LED 屏播放 VP-QSP 图案
4. 摄影机从多个角度扫过屏幕
5. 系统实时检测 marker
6. 系统自动选择高质量帧
7. 实时更新 screen correction
8. 显示误差与覆盖情况
9. 输出 quick as-built mesh
```

摄影机采集建议：

```text
从远处开始
覆盖屏幕中心
覆盖四角
覆盖 wall-floor corner
覆盖屏幕边缘
覆盖弧形屏切线方向
覆盖地屏远端
覆盖天幕或侧墙边缘
```

---

## 13. 快速模式算法流程

```text
Camera Frame
↓
曝光归一化
↓
检测 Large Anchor
↓
检测 Dense Coded Marker
↓
解码 screen_id / marker_id / orientation
↓
Gaussian dot centroid subpixel refinement
↓
建立 2D image point ↔ screen UV / 3D prior point
↓
RANSAC PnP 初始估计
↓
增量 bundle adjustment
↓
更新 screen transform / screen angle / panel group offset
↓
输出 quick model
```

---

## 14. 快速模式求解变量

快速模式建议限制自由度，避免过拟合。

推荐求解：

```text
screen-level transform
per-screen pose
screen-to-screen angle
wall-floor angle
curved screen radius correction
panel group offset
large seam offset
camera-to-screen alignment
tracking-to-screen alignment
simple lens correction
```

不建议快速模式求解：

```text
每个 LED 像素的 XYZ
每块 cabinet 的复杂 bowing
每个 module 的独立形变
高阶 generic lens model
完整 temporal scanout profile
```

这些应交给高精度模式。

---

## 15. 快速模式输出

```text
quick_as_built_screen_mesh.obj
quick_screen_transform.json
screen_angle_patch.json
panel_group_offset.json
quick_uv_to_xyz_approx.exr
screen_coverage_map.exr
sensor_coverage_map.exr
quick_reprojection_report.pdf
```

用途：

```text
nDisplay mesh 快速修正
Disguise / Pixotope screen object 快速修正
自研实时渲染系统 screen model 更新
Precision Screen Scan 初始值
每日 drift check 对比基准
```

---

## 16. 快速模式精度目标

合理工程目标：

```text
reprojection error：0.5–1.5 px
screen relative error：2–10 mm
screen-to-screen angle error：0.05°–0.2°
耗时：30 秒 – 5 分钟
```

前提：

```text
原始 CAD / topology 基本正确
camera lens file 基本可用
tracking 稳定
marker 在画面中尺寸足够
图案没有被 media server 缩放或锐化
摄影机覆盖足够角度
```

---

# Part B：Precision Screen Scan 高精度屏幕三维重建

---

## 17. 高精度模式的目标

Precision Screen Scan 的目标是：

```text
尽可能 1:1 还原现场真实 LED 发光面的三维形状。
```

它不只是修正 CAD，而是生成：

```text
as-built LED screen mesh
```

它需要回答：

```text
每块屏幕在真实空间中的位置是什么？
每块 cabinet 的真实位置和角度是什么？
墙地夹角是多少？
弧形屏实际半径是多少？
哪些区域相对 CAD 前凸或后退？
LED 发光面真实 UV → XYZ 映射是什么？
```

---

## 18. 高精度模式适用场景

适合：

- 新 LED volume 搭建完成后的正式测量；
- hero camera / hero shot 开拍前；
- 高端影视工业级几何校正；
- 需要后期 screen replacement / set extension 的项目；
- 需要输出正式 as-built screen mesh 的项目；
- 对弧形屏、地屏、天幕、侧墙夹角要求高的项目；
- 需要减少 Faro / Leica 完整扫描成本，但仍需要高精度 screen model 的项目。

不适合：

- 时间极其紧张的临时校准；
- 没有任何尺度基准但要求绝对毫米精度；
- 无法控制 LED 输出图案的场景；
- 相机、镜头、tracking、曝光完全不可控的场景。

---

## 19. 高精度模式推荐图案

高精度模式推荐：

```text
VP-SSP：Virtual Production Screen Scan Pattern
```

核心：

```text
Gray Code + Phase Shift
```

完整图案层级：

```text
Layer 0：Radiometric Frames
Layer 1：Large Anchor ID
Layer 2：X/Y Gray Code
Layer 3：X/Y Multi-step Phase Shift
Layer 4：Gaussian Dot Validation
Layer 5：Cabinet Seam Pattern
Layer 6：Temporal Flash / Moving Edge
```

---

## 20. 为什么高精度模式使用 Gray Code + Phase Shift？

高精度屏幕重建需要的数据是：

```text
camera image pixel ↔ LED continuous UV
```

不是少量：

```text
marker corner
white dot center
checkerboard corner
```

Gray Code 用于解决：

```text
这个点属于 LED 屏幕哪个绝对区域
```

Phase Shift 用于解决：

```text
这个点在该区域内部的亚像素连续位置
```

最终可以得到：

```text
screen_u = 7420.38
screen_v = 1288.71
```

这种连续 UV 数据适合做：

```text
多视角三角化
screen surface reconstruction
per-cabinet pose solve
UV-to-XYZ map
```

---

## 21. 高精度模式播放序列

以 16K × 4K LED canvas、coarse period = 64 LED px 为例：

```text
01      Black
02      White
03      18% Gray
04      50% Gray
05      80% Gray

06      Large Anchor ID
07      Large Anchor ID Inverted

08-23   X Gray Code + Inverse，8 bit × 2
24-35   Y Gray Code + Inverse，6 bit × 2

36-40   X Phase Shift，5-step
41-45   Y Phase Shift，5-step

46      Gaussian Dot Field A
47      Gaussian Dot Field B
48      Cabinet Seam Pattern

49-54   Temporal Flash / Moving Edge / Scanout Pattern
```

如果追求更高抗噪：

```text
Phase Shift 使用 6-step 或 8-step
增加 multi-frequency phase
每张 phase frame 多帧平均
增加 HDR exposure bracket
```

---

## 22. 高精度模式采集流程

推荐采集方式：

```text
固定 pose
播放完整图案序列
移动到下一个 pose
再次播放完整图案序列
```

不建议高精度模式边移动边采集。

原因：

```text
1. 多帧 phase shift 需要稳定相机
2. 移动会造成 phase mismatch
3. rolling shutter 和 LED scanout 会影响连续相位
4. 固定 pose 更容易做多帧平均和 HDR
```

---

## 23. 高精度模式相机配置

推荐配置：

```text
3–6 台固定 survey camera
或
1 台高精度 tracked camera 多位置拍摄
```

优先使用：

```text
global shutter camera
高分辨率 sensor
低畸变镜头
固定焦点
固定曝光
RAW / 无压缩采集
稳定三脚架或刚性安装
```

主摄影机可以作为辅助，但不建议完全依赖主摄影机完成最高精度 screen survey。

---

## 24. 高精度模式视角覆盖要求

每个 screen surface 至少需要：

```text
3 个以上有效视角
1 个接近正视角
2 个明显斜视角
```

每个 cabinet 至少需要：

```text
2–3 个有效视角覆盖
```

重点覆盖：

```text
主墙四角
wall-floor corner
弧形屏两端
弧形屏切线方向
地屏远端
天幕边缘
侧墙转角
屏幕 seam
施工可能误差最大的区域
```

只从正面拍不够，因为正面视角对前后深度和倾斜角不敏感。

---

## 25. 高精度模式尺度基准

这是高精度模式的关键。

如果目标是 1:1 还原，必须有尺度约束。

推荐输入：

```text
cabinet 真实尺寸
LED pixel pitch
scale bars
控制点
少量全站仪点
少量 laser distance meter 测距
tracking system 尺度
```

推荐最低配置：

```text
cabinet 尺寸
+ pixel pitch
+ 4–8 个 scale bars / control points
```

推荐影视工业级配置：

```text
cabinet 尺寸
+ pixel pitch
+ 8–20 个控制点
+ 3–6 台固定 survey camera
+ 主摄影机 tracking 数据辅助
```

没有任何尺度基准时，系统可以得到相对形状，但不能可靠承诺绝对 1:1 尺寸。

---

## 26. 高精度模式算法流程

```text
多视角 camera frames
↓
black / white / gray radiometric normalization
↓
Large Anchor 解码
↓
X/Y Gray Code 解码
↓
X/Y Phase Shift 解连续相位
↓
phase unwrap 得到 LED continuous UV
↓
建立 camera pixel ↔ LED UV dense observations
↓
初始多视角 triangulation
↓
联合 bundle adjustment
↓
求解 screen pose / cabinet pose / local deformation
↓
使用 cabinet 尺寸、pixel pitch、control points 作为约束
↓
输出 as-built screen mesh
↓
使用 validation views 检查误差
```

---

## 27. 高精度模式求解变量

推荐分层求解。

### 27.1 Screen-level

```text
每块 screen 的整体 position
每块 screen 的整体 rotation
screen-to-screen angle
wall-floor angle
curved screen radius
ceiling / side wall relation
```

### 27.2 Cabinet-level

```text
每块 cabinet 的 SE(3) pose
cabinet seam offset
cabinet row / column drift
cabinet front-back displacement
cabinet small rotation
```

### 27.3 Surface-level

```text
local bowing
twist
B-spline surface deformation
UV-to-XYZ dense mapping
```

### 27.4 Camera / lens-level

```text
camera intrinsics
lens distortion
principal point
sensor tilt
tracking-to-camera offset
camera pose refinement
```

### 27.5 Timing-level

```text
video-tracking delay
LED processor delay
rolling shutter
scanout phase
```

Timing-level 不是屏幕模型反算的必需项，但如果同一系统后续还要服务虚拍实时几何校正，应预留。

---

## 28. 高精度模式输出

```text
as_built_screen_mesh.obj
uv_to_xyz.exr
per_screen_pose.json
per_cabinet_pose.json
cabinet_seam_offset.json
cabinet_deformation.json
screen_topology_calibrated.json
nDisplay_mesh_or_pfm/
MPCDI_package/
Disguise_Pixotope_compatible_OBJ/
screen_deviation_heatmap.exr
validation_report.pdf
```

报告中必须包含：

```text
RMS reprojection error
max reprojection error
每块 screen 的误差
每块 cabinet 的误差
控制点误差
screen-to-screen angle error
coverage map
低置信度区域
建议补拍区域
是否达到发布标准
```

---

## 29. 高精度模式精度目标

合理工程目标：

```text
reprojection error：0.1–0.5 px
relative screen geometry error：0.5–2 mm
absolute geometry error：1–3 mm
screen-to-screen angle error：0.01°–0.05°
```

前提：

```text
相机已标定
镜头畸变已知或联合求解
有 scale bars / control points
LED 图案原生输出
曝光不过曝
无明显 moiré / scanline artifact
多角度覆盖充分
控制点分布合理
```

没有外部尺度基准时，不建议承诺 1–3 mm 绝对精度。

---

# Part C：两个模式的统一架构

---

## 30. 统一数据结构

两个模式应共用同一种 observation 数据结构：

```text
Observation {
    camera_xy
    screen_id
    screen_uv
    screen_xyz_prior
    feature_type
    confidence
    covariance
    camera_id
    frame_id
    timecode
    tracking_pose
    FIZ
}
```

区别只在于 observation 来源不同：

```text
Quick Screen Fit：
marker corner / marker center / Gaussian dot center

Precision Screen Scan：
phase decoded dense LED UV
```

---

## 31. 统一求解后端

两个模式应共用同一个 solver backend：

```text
Input Observations
↓
Outlier Rejection
↓
Initial Pose Solve
↓
Bundle Adjustment
↓
Screen Model Optimization
↓
QA / Validation
↓
Export
```

优化器建议支持：

```text
RANSAC
PnP
multi-view triangulation
nonlinear least squares
robust loss
factor graph
bundle adjustment
covariance estimation
```

---

## 32. 统一模型表达

最终屏幕模型应统一表达为：

```text
X_screen(u, v)
```

也就是：

```text
输入 LED 屏幕 UV 坐标
输出真实世界 XYZ 坐标
```

模型层级：

```text
Screen Transform
↓
Cabinet SE(3)
↓
Local Surface Patch
↓
UV-to-XYZ Map
```

---

## 33. 与几何校准系统的关系

ScreenModelCal 不是独立孤立功能，它应该服务于整体 LED 虚拍几何校准。

完整系统关系：

```text
ScreenModelCal
负责反算真实 LED 屏幕模型

CameraGeoCal
负责校准摄影机、镜头、tracking、screen 之间的空间关系

TemporalCal
负责校准 tracking / render / LED processor / camera exposure 的时间延迟
```

三者共同形成：

```text
LED VP Calibration Suite
```

---

## 34. 推荐使用流程

### 34.1 新场地搭建完成后

```text
1. 导入 CAD / LED topology
2. 跑 Quick Screen Fit，修正大误差
3. 设置 scale bars / control points
4. 跑 Precision Screen Scan，生成 as-built screen mesh
5. 跑 CameraGeoCal Precision，生成 lens / tracking / screen calibration
6. 输出正式 calibration package
```

### 34.2 每日开机

```text
1. 跑 Validate Current Mesh
2. 如果误差小，继续使用当前 as-built mesh
3. 如果误差中等，跑 Quick Screen Fit
4. 如果局部误差大，局部跑 Precision Screen Scan
```

### 34.3 拍摄前

```text
1. QuickCal 快速检查 camera / screen alignment
2. 检查 reprojection error 和 coverage
3. 确认 tracking / delay 状态
4. 锁定 calibration state
```

---

## 35. 推荐研发路线

### Phase 1：Quick Screen Fit

优先开发：

```text
VP-QSP 图案生成
coded marker detection
Gaussian dot centroid
normal / inverted confidence
live capture
automatic frame selection
screen-level transform solve
panel group offset solve
reprojection dashboard
OBJ / nDisplay mesh export
```

目标：

```text
快速形成现场可用能力。
```

### Phase 2：Precision Screen Scan

继续开发：

```text
Gray Code decoder
Phase Shift decoder
dense UV observation
multi-view capture manager
scale bar / control point input
per-cabinet pose solve
B-spline deformation
UV-to-XYZ map
validation report
```

目标：

```text
形成影视工业级 screen reconstruction 能力。
```

### Phase 3：与整体 VP 校准系统联动

整合：

```text
lens solver
tracking solver
temporal solver
OpenTrackIO / UE Lens File / STMap export
calibration package
daily drift comparison
```

目标：

```text
形成完整 LED VP calibration product。
```

---

# 36. 最终结论

ScreenModelCal 应该采用双模式设计：

```text
快速校正模式：
Dense Coded Marker Field + Gaussian Dot
用于现场快速修正屏幕模型。

高精度模式：
Gray Code + Phase Shift
用于反算 1:1 as-built LED screen mesh。
```

最终产品逻辑：

```text
Quick Screen Fit 解决“快”
Precision Screen Scan 解决“准”
Validate Current Mesh 解决“每天是否还能继续用”
```

一句话总结：

> 用编码 marker 快速修正屏幕模型，用 Gray Code + Phase Shift 高精度重建真实 LED 发光面。
