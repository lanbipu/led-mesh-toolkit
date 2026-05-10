# LED Mesh Toolkit — 设计文档

> **状态**：Draft v1.0
> **日期**：2026-05-10
> **负责人**：lanbipu@gmail.com
> **参考文档**：
> - `01_全站仪LED建模工具_技术方案.md`（方向参考）
> - `02_视觉反算LED屏模型技术方案.md`（方向参考）

---

## 1. 项目定位与范围

### 1.1 工具职责

LED Mesh Toolkit 是为 VP / xR 项目提供 LED 屏几何建模的桌面工具集。两条独立的数据获取路径：

1. **M1 - 全站仪路径**：从 Trimble / Leica 等全站仪实测 3D 点 → 自动归名 → 重建 mesh
2. **M2 - 视觉反算路径**：通过 LED 屏显示 ChArUco pattern + 多机位摄影 → Bundle Adjustment 反算 3D 点 → 重建 mesh

两条路径产出**格式一致的 OBJ + UV**，可互换互验，可直接导入 Disguise Designer 或 UE nDisplay。

### 1.2 目标用户

- 现场技术总监（懂 VP/xR 工作流，不一定懂 photogrammetry）
- 现场测量员（用全站仪，可能不熟 LED 工艺）
- 远程勘察 / 出差工程师（M2 主要用户）

### 1.3 适用屏体

| 屏体类型 | 支持 | 备注 |
|---|---|---|
| 平面屏 | ✅ | 完全支持 |
| 弧形屏（恒定半径） | ✅ | 主用例 |
| 椭圆 / 变曲率弧形 | ⚠️ | 通过密集采样支持，精度受限 |
| 折角屏（多段平面） | ✅ | 通过 GUI 标记折角列 |
| 异形屏（L 形、缺角） | ✅ | GUI 删格子模式 |
| 复杂自由曲面 | ❌ | 不在工具范围 |
| 球形 / 穹顶屏 | ❌ | 不在工具范围 |

### 1.4 明确不做的事

- 色彩校准 / 亮度匹配（OpenVPCal 的领域）
- 实时跟踪标定（Disguise Spatial Calibration 的领域）
- 点云后处理（FARO 等专业测量后处理软件的领域）
- 自由曲面建模
- LED 屏内容显示控制（Disguise / 服务器的领域）

---

## 2. 整体架构

### 2.1 三层模型

```
[Input Adapters]              [IR - 中间表达]            [Output Targets]
  CSV (全站仪)  ──┐                                    ┌─→ disguise OBJ
  ChArUco BA ────┼──→  MeasuredPoints  →  Reconstruct  ┼─→ unreal OBJ
                 ┘     ReconstructedSurface            └─→ neutral OBJ
                       MeshOutput
```

每个 input adapter 内部算法独立，输出统一为 `MeasuredPoints`。重建 / UV / 导出共用一套实现，所有数据源都受益。

### 2.2 代码组织（Cargo workspace + 多 crate）

```
led-mesh-toolkit/
├── Cargo.toml                       # workspace 根
├── package.json                     # 前端
├── crates/
│   ├── core/                        # ⭐ 共用：IR + 重建 + UV + 导出
│   │   ├── ir.rs                    # MeasuredPoints / ReconstructedSurface / MeshOutput
│   │   ├── reconstruct/             # 直连 / 边界插值 / 径向基 / 标称
│   │   ├── uv.rs
│   │   └── export/                  # disguise / unreal / neutral
│   ├── adapter-total-station/       # 🔵 M1 全站仪 adapter
│   │   ├── csv_parser.rs
│   │   ├── geometric_naming.rs
│   │   └── reference_frame.rs
│   └── adapter-visual-ba/           # 🟣 M2 视觉反算 adapter
│       ├── pattern_generator.rs
│       ├── feature_detector.rs      # 调 Python sidecar
│       └── bundle_adjustment.rs     # 调 Python sidecar
├── src-tauri/                       # Tauri 后端入口
├── src/                             # Vue 3 前端
└── python-sidecar/                  # M2 Python 子进程
    ├── main.py
    ├── charuco.py
    └── bundle_adjust.py
```

### 2.3 技术栈

| 层 | 技术选型 | 与 UECM 关系 |
|---|---|---|
| App 框架 | Tauri 2.x | 同 |
| 前端 | Vue 3 + TypeScript + Vite | 同 |
| UI | Tailwind CSS + reka-ui + class-variance-authority | 同 |
| 状态 | Pinia | 同 |
| 路由 | Vue Router | 同 |
| i18n | vue-i18n | 同 |
| 后端核心 | Rust（rusqlite, serde, tokio） | 同 |
| 数学 / 几何 | nalgebra, glam, kiddo, argmin | 新增 |
| 3D 渲染 | Three.js + 薄 Vue 包装 | 新增 |
| 2D 网格编辑 | Konva.js / vue-konva | 新增 |
| M2 视觉算法 | Python sidecar（OpenCV-contrib + scipy） | 借用 PowerShell sidecar 模式 |
| 测试 | Vitest + Cargo test + Pytest | Vitest / Cargo 同 |
| 打包 | 单 .exe / .dmg / .AppImage | 同 |

### 2.4 平台支持

- **目标**：Windows 10/11（VP/xR 现场主流）
- **开发**：macOS / Linux 可用
- **测试**：Vitest + Cargo test 跨平台；Python sidecar 在三平台均可

---

## 3. IR（中间表达）

### 3.1 设计原则

IR 切在"两套算法的输出汇合点"。前半段（数据获取）每个 adapter 独立实现；后半段（重建 / UV / 导出）共用。

### 3.2 核心数据结构

```rust
pub struct MeasuredPoint {
    pub name: String,              // "MAIN_V001_R001"
    pub position: Vec3,            // 模型坐标系下的位置（米）
    pub uncertainty: Uncertainty,  // 不确定度
    pub source: PointSource,       // 数据来源
}

pub enum Uncertainty {
    Isotropic(f32),                // 全站仪：单个 σ 值（mm）
    Covariance3x3(Mat3),           // 视觉反算：3×3 协方差矩阵
}

pub enum PointSource {
    TotalStation,                  // M1
    VisualBA { camera_count: u32 }, // M2
}

pub struct MeasuredPoints {
    pub points: Vec<MeasuredPoint>,
    pub coordinate_frame: CoordinateFrame,  // origin + X 轴 + XY 平面定义
    pub screen_id: String,
    pub shape_prior: ShapePrior,            // curved/flat/folded + 关键参数
    pub cabinet_array: CabinetArray,        // 行×列 + 单箱体尺寸 + mask
}

pub struct ReconstructedSurface {
    pub vertices: Vec<Vec3>,                // 模型坐标系下的顶点
    pub topology: GridTopology,             // 网格拓扑
    pub uv_coords: Vec<Vec2>,
    pub quality_metrics: QualityMetrics,    // 重建后的质量指标
}

pub struct MeshOutput {
    pub vertices: Vec<Vec3>,                // 已按 target 软件适配
    pub triangles: Vec<[u32; 3]>,
    pub uv_coords: Vec<Vec2>,
    pub target: TargetSoftware,             // disguise / unreal / neutral
}
```

### 3.3 接口 trait

```rust
trait InputAdapter {
    fn parse(&self, source: &Path, config: &ProjectConfig) 
        -> Result<MeasuredPoints>;
}

trait Reconstructor {
    fn reconstruct(&self, points: &MeasuredPoints) 
        -> Result<ReconstructedSurface>;
}

trait OutputTarget {
    fn export(&self, surface: &ReconstructedSurface, path: &Path) 
        -> Result<()>;
}
```

### 3.4 不确定度的工程意义

- 重建器读 `uncertainty` 决定加权策略（小不确定度高权重）
- 一份重建器代码处理 M1 和 M2 两类数据
- 全站仪默认 `Isotropic(±1-3mm)`；视觉反算从 BA 协方差矩阵填 `Covariance3x3`
- **M1 阶段就必须实现这个接口**，避免 M2 加进来时改 IR

---

## 4. M1 - 全站仪路径

### 4.1 工作流总览

```
[设计阶段 - GUI]
  输入箱体阵列 + 形状先验 → 2D 布局编辑器 → 3 参考点选定
  → 导出指示卡（PDF + HTML）+ 项目 YAML

[现场测量]
  操作员先测 3 参考点 → 再测其他点
  → Trimble Access 导出 CSV

[回工具 - GUI 或 CLI]
  导入 CSV + 加载 YAML → 几何归名 → 重建 → 3D 预览 → 导出 OBJ
```

### 4.2 输入数据格式

#### CSV（全站仪原始数据）

```csv
name,x,y,z,note
1,1234.567,5678.901,12345.678,
2,31234.567,5678.901,12340.012,
3,1234.567,5678.901,2345.678,
4,1734.500,5680.000,12345.500,
...
```

- 单位：毫米
- 坐标系：仪器原始（任意）；工具内部变换到模型坐标系
- `name` 字段可以是数字点号（仪器自动给）—— 工具按几何归名
- `note` 可选

#### 项目 YAML

```yaml
project:
  name: "Studio_A_Volume"
  unit: "mm"
  
screens:
  MAIN:
    cabinet_count: [120, 20]            # 列 × 行
    cabinet_size_mm: [500, 500]
    pixels_per_cabinet: [256, 256]
    shape_prior:
      type: curved                      # curved | flat | folded
      radius_mm: 30000                  # 弧形必填
      fold_seams_at_columns: []         # 折角必填
    shape_mode: rectangle               # rectangle | irregular
    irregular_mask: []                  # irregular 模式：被剔除的箱体 [(col, row), ...]
    bottom_completion:
      lowest_measurable_row: 5          # 实际可测最低行
      fallback_method: vertical
      assumed_height_mm: 2000
  FLOOR:
    cabinet_count: [12, 8]
    cabinet_size_mm: [500, 500]
    shape_prior:
      type: flat
    shape_mode: rectangle

coordinate_system:
  origin_point: "MAIN_V001_R005"        # 与 lowest_measurable_row 一致
  x_axis_point: "MAIN_V120_R005"        # 必须是物理可测的位置
  xy_plane_point: "MAIN_V001_R020"

output:
  target: disguise                       # disguise | unreal | neutral
  obj_filename: "{screen_id}_mesh.obj"
  weld_vertices_tolerance_mm: 1.0
  triangulate: true
```

### 4.3 命名规则

- 格式：`<SCREEN>_V<col>_R<row>`，col 范围 1-120，row 范围 1-20
- col / row 都是 1-based，左下为 (V001, R001)
- 工具按"3 参考点法"建立坐标系后，按几何位置最近邻匹配自动归名
- 现场测量员**不需要**手工敲名字到仪器

### 4.4 3 参考点法（坐标系定义）

| 角色 | 测点 | 作用 |
|---|---|---|
| Origin | 第 1 个测点 | 模型 (0, 0, 0) |
| X-axis | 第 2 个测点 | +X 方向 |
| XY-plane | 第 3 个测点 | 约束 XY 平面 |

工具内部计算（Gram-Schmidt 正交化）：

```
X = normalize(P_x − P_origin)
Z = normalize((P_xy − P_origin) × X)   ← 叉乘得 up
Y = Z × X                              ← 正交化
```

不依赖全站仪水平校准，3 个参考点完全确定坐标系朝向。

> **⚠️ 关键约束**：3 个参考点必须是**物理上可测的位置**——不能选被舞台、地板设备、栏杆遮挡的位置。当底部存在遮挡时（`lowest_measurable_row = N`），参考点选择应从 R<N> 行起：
>
> - Origin = `V001_R<N>`（左下角，实际可测最低位置）
> - X-axis = `V120_R<N>`（右下角，同一行）
> - XY-plane = `V001_R<top>`（左上角，顶行）
>
> 参考点选错（落在遮挡区）会导致整个坐标变换失败。GUI 应在 2D 编辑器中阻止用户把 3 参考点放到测量基线下方。

### 4.5 几何归名算法

伪代码：

```rust
let kdtree = KdTree::from_iter(expected_grid_positions);
for measured_point in csv_points {
    let (dist, expected_id) = kdtree.nearest(measured_point);
    if dist < threshold_mm {
        assign(measured_point, expected_id);
    } else {
        flag_as_outlier(measured_point);
    }
}
```

- 阈值默认 50mm（半个箱体宽度的 1/5）
- 歧义点（两个测点都距离同一个 expected 位置 < 阈值）→ GUI 高亮让用户确认
- 离群点 → GUI 标红
- 漏测预期点 → GUI 标灰 + 报告里列出

### 4.6 形状先验 + 异形处理

#### 形状先验组合：(a) YAML 声明 + (b) 测点修正

- YAML 声明 `type: curved/flat/folded` + 关键参数（半径或控制点）
- 实测点用于验证 / 修正先验
- 重建器以先验为初值，实测点为残差修正

#### 异形处理决策树

```
shape_mode: rectangle (默认)
  ├─ 用户测全 → 直接按矩形阵列建模
  └─ 用户没测全 → fallback 补全（仅缺角点）+ warning

shape_mode: irregular
  └─ 用户在 2D 网格编辑器删格子明确表达 mask
     - mask 内：建模
     - mask 外：留空 / 不参与 mesh
     - 缺测的 mask 内位置：fallback 补全 + warning
```

### 4.7 底部遮挡处理

LED 屏底部经常被舞台、设备、栏杆遮挡，前 3-5 行可能完全不可测。

**GUI 加"测量基线"功能**：用户在 2D 网格上标记"实际可测最低行"。基线下面的行用 fallback 推算。

#### YAML 配置

```yaml
bottom_completion:
  lowest_measurable_row: 5
  fallback_method: vertical
  assumed_height_mm: 2000
```

#### 算法

- 假设 R001-R004 在 R005 的正下方（垂直延伸）
- 高度差 = (5-1) × 500mm = 2000mm
- 报告中显式标记"R001-R004 是 fallback 补全的"

#### 精度声明

```
WARNINGS:
- R001-R004 因底部遮挡未实测，垂直延伸补全（high 2.0m）
- 补全精度估计 ±5-15mm（依赖箱体堆叠垂直度）
- 该精度对 VP 项目可接受；xR 项目建议清理遮挡后补测底边
```

### 4.8 重建算法（插件化）

按测点稀疏度自动选：

| 重建器 | 触发条件 | 算法 |
|---|---|---|
| `direct_link` | 完整网格采样（每箱体角都测） | 直接连点，无插值 |
| `boundary_interp` | 边界完整 + 中段抽样 | 边界拟合 + 双线性插值 + 中段验证 |
| `radial_basis` | 自适应稀疏 | 径向基插值 / 薄板样条 |
| `nominal` | 仅 4 角 + 形状先验 | 按形状先验 + 箱体阵列做标称建模 |

重建器选择策略：从最严格到最宽松依次尝试，第一个满足触发条件的胜出。

### 4.9 测点下限 + 精度报告

#### 必要

- 3 参考点（origin / X 轴 / XY 平面）
- 4 角（实际可测最低行 + 顶行）
- 形状先验

#### 推荐

- 4 角 + 顶/底边各 ~5 点 + 中段 1 点 = ~16 点（VP 标准够用）

#### 报告内容（JSON + GUI 双输出）

```json
{
  "input": {
    "expected_points": 277,
    "measured_points": 273,
    "missing": ["V015_R020", "V034_R020", "V067_R020", "V089_R020"],
    "outliers": [],
    "ambiguous": []
  },
  "reconstruction": {
    "method": "boundary_interp_with_middle_validation",
    "middle_max_dev_mm": 4.2,
    "middle_mean_dev_mm": 1.3,
    "shape_fit_rms_mm": 2.1
  },
  "output": {
    "vertex_count": 2541,
    "face_count": 4800,
    "obj_path": "MAIN_disguise.obj",
    "vertices_under_disguise_limit": true
  },
  "estimated_precision": {
    "rms_mm": 4.5,
    "p95_mm": 8.2,
    "basis": "测点密度 + 形状残差"
  },
  "warnings": [
    "顶边 4 个点漏测，使用插值补全 - 建议补测",
    "V064_R010 中段偏差 7.3mm - 该处可能存在变形"
  ]
}
```

GUI 同时在 3D mesh 上**高亮 warning 区域**（红色 / 橙色 cell）。

### 4.10 现场指示卡

#### 内容结构（PDF + HTML 双版本）

```
═══════════════════════════════════════════════
LED 屏建模 - 测量指示卡
项目：Studio_A_Volume    日期：2026-05-10
箱体阵列：120 × 20，单箱体 500×500mm
预期总点数：277（3 参考点 + 274 测量点）
═══════════════════════════════════════════════

【第一步：3 个参考点（按顺序，仪器点号 1-3）】

序号  角色          物理位置（在 origin 坐标系下）
1     ① Origin      主屏 R005 行左侧箱体右上顶点 (0, 0, 0)
2     ② X-axis      主屏 R005 行右侧箱体左上顶点 (60.0, 0, 0)
3     ③ XY-plane    主屏 R020 行左侧箱体右下顶点 (0, 0, 7.5)

【第二步：网格示意图（带高亮）】
[2D 展开图：● 必测 / ○ 抽样 / ░░ 异形剔除 / ╳ 遮挡补全]

【第三步：详细列表（按建议测量顺序）】

#    分组       工具命名      绝对坐标 (X, Y, Z) 米
4    顶边-001   V001_R020     (0.0, 0.0, 10.0)
5    顶边-002   V002_R020     (0.5, 0.0, 10.0)
...
═══════════════════════════════════════════════
```

#### 现场两种操作模式

| 模式 | 现场操作 | 工具角色 |
|---|---|---|
| **A 基础（仪器原坐标）** | 测量员瞄准"大致位置"测点 | 工具事后用 ①②③ 建立坐标变换 → 几何归名 |
| **B 进阶（Trimble Access 配准）** | 测 ①②③ 后做坐标配准 → 仪器后续显示 origin 坐标系 → 直接按绝对坐标瞄准 | 工具仍然事后做归名 + 校验 |

工具不强制走哪种模式——给出绝对坐标即可，现场流程由操作员选。

---

## 5. M2 - 视觉反算路径

### 5.1 整体管线（基于 Doc 2 的 D 方案）

```
Step 1: ChArUco Pattern 生成
Step 2: Disguise 显示 + 多机位拍摄
Step 3: ArUco 检测 + 棋盘格亚像素细化
Step 4: Bundle Adjustment（OpenCV + scipy.least_squares）
Step 5: ArUco ID 解码 → 网格命名（V<col>_R<row>）
Step 6: 输出 → MeasuredPoints (IR) → 复用 M1 重建管线
```

### 5.2 Pattern 设计

| 参数 | 值 |
|---|---|
| Pattern 拓扑 | 每箱体独立 ChArUco（隔离故障） |
| ArUco 字典 | DICT_6X6_1000 |
| 棋盘格规格 | 8 × 8 内角点（9 × 9 格） |
| 物理尺寸 | 与箱体一致（500 × 500mm） |
| Marker 像素尺寸 | ≥ 50 × 50px（4K 视野下） |
| ArUco ID 编码 | `id = (row × n_cols + col) × markers_per_cabinet` |

#### LED Pixel Pitch 适用范围

| pitch | 每格物理尺寸 | 像素数 | 评估 |
|---|---|---|---|
| 1.5mm（高密度 xR） | 55mm | ~37 | ✅ 优秀 |
| 2.5mm（标准 xR） | 55mm | ~22 | ✅ 良好 |
| 3.9mm（VP 主流） | 55mm | ~14 | ⚠️ 临界 |
| 5.0mm（中端 VP） | 55mm | ~11 | ⚠️ 临界 |
| 6.5mm+（低端） | 55mm | ~8 | ❌ 离散化严重 |

> **降级方案**：pitch > 5mm 时退化到方案 C（纯 ArUco，无棋盘格）—— 牺牲亚像素精度换鲁棒性。

### 5.3 拍摄 SOP

#### 设备

| 用途 | 推荐 |
|---|---|
| PoC + 早期项目 | 项目组现有 4K-8K 微单 / 单反（如 A7Rxx, R5） |
| 生产级标配 | Sony A7R5（6K, 9504×6336）或 Canon R5（8K）+ 24-70mm 定焦 50mm |
| 应急备份 | iPhone 14 Pro+（4K Pro RAW） |

#### 关键约束

- 镜头预先用 OpenCV checkerboard 标定
- **现场锁焦 + 锁光圈 + 锁变焦**（变更 → BA 多解）
- RAW 或低压缩 JPEG（避免压缩伪影影响角点）

#### 拍摄规划（60×10 弧墙示例）

```
              [机位俯视图]

    ┌─────────────────────────┐
    │  弧墙（60m）              │
    │       ╱──────╲            │
    │     ╱           ╲         │
    │   ╱               ╲       │
    │ [P1]   [P5]    [P9]       │  ← 远景排（距屏 ~15m）
    │                            │
    │     [P2]  [P6]  [P10]     │  ← 中景排（距屏 ~8m）
    │                            │
    │  [P3]  [P7]    [P11]      │  ← 近景排（距屏 ~3m）
    │                            │
    │     [P4]  [P8]  [P12]     │  ← 高位（地面 +3m）
    └─────────────────────────┘

每机位 3-5 张，水平方向覆盖左中右 3 个角度。
```

#### 拍摄要点

- **每个 ChArUco 至少被 4 张图像观测**（稳健最小值）
- **重叠率 ≥ 60%**
- 远近结合：远距保整体 + 近距保精度
- 1/250s 以上快门（避免运动模糊）
- LED 输出 50-80% 亮度（避免过曝同时保留对比度）

预期照片数量：**30-60 张**

### 5.4 算法栈：Python sidecar

#### 选型理由

OpenCV-contrib aruco + scipy.least_squares 是金标准管线（~1500-2500 行 Python）。Rust 没有同等成熟度的 aruco 绑定。

#### 架构

```
Tauri (Rust 后端)
   ↓ tokio::process::Command 启动子进程
   ↓ stdin / stdout JSON 双向 IPC
Python sidecar (cv2 + scipy + numpy)
   ↓ 接收图像路径列表 + 项目配置
   ↓ ChArUco 检测 + BA 求解
   ↓ 输出 MeasuredPoints JSON
Tauri (Rust 后端)
   ↓ 反序列化 → 走 IR 后续管线
```

借用 UECM 的 PowerShell sidecar 模式，把 PowerShell 替换为 Python。

#### Python 依赖（pyproject.toml 范例）

```toml
[project]
name = "led-mesh-vba-sidecar"
requires-python = ">=3.10"

dependencies = [
    "numpy>=1.24",
    "scipy>=1.10",
    "opencv-contrib-python>=4.8",
    "pydantic>=2.0",
]
```

打包：PyInstaller 单 exe 嵌入到 Tauri 资源里（参考 UECM 的 PsExec64 vendoring）。

### 5.5 PoC 设计（M2 启动前置）

#### 必测项（Doc 2 第 7.3 节）

1. ✗ 真实 LED 屏（非屏幕模拟）实测精度
2. ✗ ChArUco 在 LED 显示的视觉退化对角点精度的影响
3. ✗ 影棚环境光对算法稳定性的影响
4. ✗ 与全站仪测量结果的精度对比（作为 ground truth）

#### 最小 PoC 设计

| 项 | 配置 |
|---|---|
| LED 测试墙 | 4 × 4 箱体（1m × 1m）足够 |
| Ground truth | 全站仪测同一组角点 |
| 拍摄 | 10-15 张图（远 / 中 / 近混合） |
| 通过门槛 | RMS < 5mm（VP 标准） |

#### Stretch goals（PoC 通过后扩展）

- 不同 LED pitch 对比（1.5mm vs 2.5mm vs 5mm）
- 不同光照对比
- 不同镜头 / 相机分辨率对比

### 5.6 与 M1 的输出一致性

视觉反算输出的 `MeasuredPoints` 与 M1 完全相同：
- 同样的命名（`MAIN_V001_R001`）—— ArUco ID 解码后直接得
- 同样的坐标系（origin 坐标系）—— BA 输出后做坐标变换
- 不确定度 `Covariance3x3`（BA 残差矩阵）

**重建 / UV / 导出走 IR 共用管线**，与 M1 一致。

### 5.7 视觉反算的"绝对坐标系定义"问题（待 PoC 阶段决定）

BA 输出的 3D 点是**相对的**——以某一相机为锚点的相对姿态。要变换到 origin 坐标系（与 M1 一致），有 3 个候选方案：

| 方案 | 实现 | 取舍 |
|---|---|---|
| **A. 已知箱体结构 anchoring** | 工具内部按 cabinet_array + shape_prior 算出每个 ChArUco ID 的"标称物理位置"（origin 坐标系下），然后 Procrustes 对齐到 BA 输出 | 不需要现场额外测量；但屏幕实际形状与先验偏差大时漂移 |
| **B. GCP（Ground Control Point）锚定** | 屏幕外加 3-4 个全站仪测过的物理 marker（与屏幕同视野），视觉反算同时检测它们 → 强制把 BA 结果变换到全站仪坐标系 | 精度最稳；但要在现场布置 GCP，多一道工序 |
| **C. 全站仪 + 视觉反算混合** | 用全站仪测 3 参考点（跟 M1 一样）+ 视觉反算其他点；3 参考点作 BA 软约束 | 与 M1 流程统一；但 M2 的"独立可用"卖点削弱 |

**M2 PoC 阶段决定具体方案**——不强制现在选。Spec 在 IR 接口层不区分这三种方案（都输出"已在 origin 坐标系下的 MeasuredPoints"）。

---

## 6. 共用模块（在 `core/` crate 中）

### 6.1 重建引擎

```rust
trait Reconstructor {
    fn applicable(&self, points: &MeasuredPoints) -> bool;
    fn reconstruct(&self, points: &MeasuredPoints) 
        -> Result<ReconstructedSurface>;
}

impl Reconstructor for DirectLinkReconstructor { /* ... */ }
impl Reconstructor for BoundaryInterpReconstructor { /* ... */ }
impl Reconstructor for RadialBasisReconstructor { /* ... */ }
impl Reconstructor for NominalReconstructor { /* ... */ }
```

选择逻辑：按从严到宽顺序，第一个 `applicable` 返回 true 的胜出。

### 6.2 UV 展开

每箱体严格对一格 UV，跟 disguise Screen Resolution 1:1 匹配。

```rust
fn compute_uv(col: u32, row: u32, total_cols: u32, total_rows: u32) -> Vec2 {
    Vec2 {
        x: col as f32 / total_cols as f32,
        y: 1.0 - (row as f32 / total_rows as f32),  // disguise V 朝上
    }
}
```

异形剔除区在 UV 上仍有对应区域，但 mesh 上无顶点 → disguise 显示为黑屏。

### 6.3 OBJ 导出（三个 target）

```rust
trait OutputTarget {
    fn export(&self, surface: &ReconstructedSurface, path: &Path) -> Result<()>;
}
```

| Target | 坐标系适配 | 单位 |
|---|---|---|
| **disguise** | 右手系 + Y up（swap Y↔Z） | m |
| **unreal** | 左手系 + Z up（手系翻转） | cm |
| **neutral** | 原始模型坐标系（右手 + Z up） | m |

### 6.4 顶点焊接 + 三角化

- 顶点焊接：KD-Tree（kiddo crate），默认 1mm 容差
- 三角化：每四边形拆两个三角形，对角线选短边以减形变
- 验证：`vertex_count <= 200_000`（disguise 上限）

---

## 7. GUI 视图组织

### 7.1 顶层导航（Vue Router）

```
/projects                项目列表
/projects/:id/design     2D 布局编辑器（M1+M2 共用：箱体阵列 / 形状 / 参考点）
/projects/:id/instruct   指示卡导出（M1）
/projects/:id/import     数据导入（M1 CSV / M2 图像）
/projects/:id/preview    重建结果 3D 预览（M1+M2 共用）
/projects/:id/export     导出 OBJ（M1+M2 共用）
/projects/:id/charuco    ChArUco Pattern 生成器（M2）
/projects/:id/photoplan  拍摄规划 + 进度（M2）
```

### 7.2 关键视图职责

#### `/projects/:id/design`（2D 布局编辑器）

- Konva.js 画 2D 网格（120 × 20 cell）
- 点击删除箱体（异形 mask）
- 拖动测量基线（标记可测最低行）
- 选 3 参考点（点击 cell 切换 origin / X 轴 / XY 平面角色）
- 实时同步到 YAML

#### `/projects/:id/preview`（3D 预览）

- Three.js + 薄 Vue 包装
- WebGL 渲染 mesh，最大 200k 顶点
- OrbitControls（旋转 / 缩放）
- Warning 区域 cell 高亮（红 / 橙）
- 模式切换：标称形状 vs 测量后形状（diff 模式可看修正量）

#### `/projects/:id/import`（数据导入）

- M1：拖放 CSV 文件 → 几何归名 → GUI 显示归名结果（歧义 / 离群高亮）
- M2：选择图像文件夹 → Python sidecar 启动 → 进度条 → BA 完成

### 7.3 进度推送

UECM 的 `batch-progress` 模式直接复用：

```rust
#[tauri::command]
async fn rebuild_mesh(app: tauri::AppHandle, /* ... */) 
    -> Result<MeshResult, String> {
    let (tx, mut rx) = mpsc::channel(32);
    tokio::spawn(async move {
        while let Some(progress) = rx.recv().await {
            app.emit("mesh-rebuild-progress", progress).ok();
        }
    });
    // 算法跑起来，往 tx 推进度
}
```

---

## 8. 开发节奏与发布

### 8.1 里程碑

| 里程碑 | 内容 | 模式 |
|---|---|---|
| **M0 - 共用框架** | `core/` IR + 重建 / UV / 导出引擎 + GUI shell（路由 / 布局 / Pinia 骨架） | 单 session 串行（必须先做） |
| **M1 - 全站仪** | `adapter-total-station/` + M1 专属 GUI 视图（2D 编辑器 / 指示卡 / CSV 导入） | 与 M2 **并行**，独立 session |
| **M2 - 视觉反算** | `adapter-visual-ba/` + Python sidecar + M2 专属 GUI 视图（ChArUco / 拍摄规划 / 图像导入） | 与 M1 **并行**，独立 session |
| **M3** (可选) | H 方案：ChArUco BA + LED 几何先验 refinement | M2 完成后视效果决定 |

> **关键约束**：M0 必须先于 M1 / M2。共用框架不冻结，两个并行 session 会一直修改同一组接口造成冲突。M0 阶段仅一个 session 工作，把 `core/` 接口、IR 结构、GUI shell 全部定型。

### 8.2 并行 session 开发策略

- **M0 阶段**：单 session 完成基础框架，结束后 `core/` 接口冻结
- **M1 / M2 阶段**：两个独立 Claude Code session 同时启动并行开发：
  - **M1 session** 只动 `crates/adapter-total-station/` + 全站仪相关 GUI 视图（`/design` `/instruct` `/import` 的 CSV 部分）
  - **M2 session** 只动 `crates/adapter-visual-ba/` + `python-sidecar/` + 视觉反算相关 GUI 视图（`/charuco` `/photoplan` `/import` 的图像部分）
  - 两个分支的 git diff 几乎不重叠 → 合并冲突最小
- M0 之后 `core/` 不轻易动；M1 或 M2 实施过程中如发现 IR 接口不够（例如 M2 BA 的协方差字段需要扩展），是个独立 PR，需要双方 sync

### 8.3 发布流程

发布顺序由实际进度决定，不强制 M1 早于 M2 或反之：

| 版本 | 触发条件 | 内容 |
|---|---|---|
| v0.1.0 | M0 完成 | 基础框架可用，无 adapter（demo 数据可走通管线） |
| v0.2.0 | M1 或 M2 任一完成 | 单条路径可用 |
| v0.3.0 | M1 + M2 都完成 | 两条路径都可用 |
| v0.4.0 (可选) | M3 完成 | + 几何先验增强 |

### 8.4 PoC 节点（M2 进生产前）

```
[小型 LED 测试墙 + 全站仪 ground truth]
   ↓ ChArUco BA 实测
[RMS < 5mm？]
   ├─ Yes → M2 启动
   └─ No  → 调整 Pattern / SOP / 算法 → 再测
```

---

## 9. 风险与不确定项

### 9.1 M1 风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| 底部遮挡 fallback 实际精度未验证 | 中 | 先在 1-2 个真实项目测，调整阈值；xR 项目强制要求清理遮挡 |
| 几何归名歧义点处理 SOP | 中 | GUI 高亮歧义 + 用户手动确认 + 单元测试覆盖典型歧义场景 |
| 异形 mask 用户编辑出错 | 低 | 2D 编辑器加 undo/redo + YAML 校验 |
| Trimble Access 现场坐标配准（B 模式）的实际操作流程 | 中 | 第一个项目派现场指导，事后整理 SOP |

### 9.2 M2 风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| PoC 不通过（4 项必测之一失败） | 高 | PoC 失败 → 调 Pattern / SOP / 算法 → 再测；无效则降级到 M2 仅做远程勘察粗模 |
| Python sidecar IPC 设计 | 中 | 借鉴 UECM 的 PowerShell sidecar 设计；JSON 双向通信 + 进度推送 |
| BA 收敛性调优 | 中 | 用 OpenCV 标准管线作 baseline；不收敛时 fallback 到逐图 PnP |
| Python 运行时打包到 Tauri | 中 | PyInstaller 单 exe + Tauri 资源 vendor（参考 UECM 的 PsExec64） |

### 9.3 跨 M1 / M2 风险

| 风险 | 严重度 | 缓解 |
|---|---|---|
| IR 不确定度接口设计在 M1 阶段冻结后 M2 发现不够 | 中 | M1 阶段先写出 M2 用例的单元测试（mock MeasuredPoints），保证接口够用 |
| Cargo workspace 在 Tauri 项目里 build 配置 | 低 | UECM 已用单 crate 模式；workspace 只是把 crate 拆开，Tauri 配置仍然 reference 单一 backend |

### 9.4 已知不确定项（生产前必须验证）

- ChArUco 在真实 LED 屏上的视觉退化（PoC 项目）
- Trimble Access 坐标配准（B 模式）的现场操作（第一个生产项目验证）
- 异形 mask 的 fallback 补全在真实异形屏上的精度（积累项目数据后调阈值）
- M2 视觉反算的绝对坐标系定义方式（A/B/C 三选一，PoC 阶段决定）
- M3 几何先验加 refinement 的实际效果（M3 阶段才测）

---

## 10. 验收标准

### 10.1 M1 验收

| 测试项 | 通过标准 |
|---|---|
| 标准弧形屏（120 列 × 20 行）建模 | 顶点数 ≤ 3000，UV 在 [0, 1]，watertight |
| 平面屏建模 | 所有顶点共面残差 < 0.5mm |
| 折角屏建模 | 折角列处法向量不连续 |
| 异形屏（L 形）建模 | 删除 mask 区无顶点，其他正常 |
| 底部遮挡 fallback 补全 | 输出 mesh 完整，warning 明确提示 |
| 主屏 + 地屏拆分 | 输出两个独立 OBJ，坐标系一致 |
| 顶点焊接 | 共边顶点 < 1mm 距合并，watertight |
| 在 Disguise Designer 中加载 | 无错误、无黑屏、Radar 测试图正确 |
| 在 UE nDisplay 中加载 | 无错误、坐标系正确 |
| 60m × 10m 弧墙处理时间 | < 5 分钟 |
| 跨平台 | Windows / macOS / Linux 行为一致 |

### 10.2 M2 验收

| 测试项 | 通过标准 |
|---|---|
| **PoC 阶段** | RMS < 5mm（VP 标准） |
| **生产级阶段（10m 距离）** | ±3-6mm |
| **生产级阶段（20m 距离）** | ±5-10mm |
| 与全站仪 mesh 对比 | 平均偏差 < 全站仪精度 + 视觉反算精度的 RSS |
| 运行时 | 30-60 张图像 BA 求解 < 5 分钟 |
| Python sidecar 集成 | 异常退出能被 Tauri 捕获并展示 |

### 10.3 M3 验收（可选）

| 测试项 | 通过标准 |
|---|---|
| 拍摄机位减少 | 30+ 减到 10-20 张，精度维持 ±3-6mm |
| 几何先验失效时 | 静默偏差 < ChArUco D 方案精度 + 安全余量 |
| 与 D 方案对比 | 不更差（任何典型场景下） |

---

## 附录 A：术语表

| 术语 | 含义 |
|---|---|
| BA | Bundle Adjustment，光束法平差，photogrammetry 经典优化 |
| ChArUco | OpenCV 提供的 ArUco + 棋盘格组合 fiducial pattern |
| IR | Intermediate Representation，工具内部的标准数据格式 |
| OBJ | Wavefront 3D 模型格式，disguise / UE 都支持 |
| PoC | Proof of Concept，可行性验证 |
| SOP | Standard Operating Procedure，标准作业流程 |
| VP | Virtual Production，虚拟制片 |
| xR | Extended Reality，扩展现实（VP 的影视级形态） |

## 附录 B：参考资料

- `01_全站仪LED建模工具_技术方案.md`（项目方向参考）
- `02_视觉反算LED屏模型技术方案.md`（项目方向参考）
- Disguise Designer 关于 OBJ mesh 与 UV 的要求（项目知识库 `help-disguise-one`）
- OpenCV ArUco / ChArUco 文档（`opencv-contrib-python` 4.8+）
- UECM 项目架构（同类 Tauri + Vue 3 应用，`/Users/bip.lan/AIWorkspace/vp/ue-cache-manager`）
