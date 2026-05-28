# Path B 导出改造 · 整体网格 UV + 单文件

> 日期：2026-05-28
> 范围：只改造 Path B 的 `lmt export pose-obj`，让它产出 xR/VP 可用的单文件 LED 屏 OBJ。
> 用途：xR/VP 相机实拍（逐箱体几何保真 + 内容横铺整面墙）。
> 后续：用户在 disguise 里做实际模拟测试验收。

> **修订 v2（2026-05-28，回应 Codex adversarial review）**：第 4 节把"无下游依赖"的
> 断言换成 grep 核实的调用点清单 + 显式兼容性决策（见 4.1）；据实删掉原"需同步
> Tauri shim + GUI"（实际无此调用面）。**不**采纳 Codex 的 deprecation 别名 / DTO 双字段 /
> schema 版本建议——与 CLAUDE.md "不加向后兼容 shim" 冲突，且旧输出对 disguise 本不可用、
> 无真实消费者。

## 1. 背景与目标

Path B = 拍屏 ChArUco → 自标内参 → BA 反算每块箱体位姿，零全站仪，产出
`cabinet_pose_report.json`（每块 cabinet 一个世界系 4 角点，`cabinet_id` 格式
`V<col>_R<row>`，model-constrained 刚性平板）。

`lmt export pose-obj` 把这些位姿导成 OBJ 给 disguise。但当前实现对 xR/VP **不可用**：

1. **UV 错**：`panel_surface`（`crates/lmt-app/src/export.rs:317`）给每块写死
   `compute_grid_uv(1×1)` = 铺满 `[0,1]`。2880 块 = 2880 个重叠满幅 UV → disguise 会把
   **整幅内容贴到每一块**，而不是一幅画横铺整面墙。
2. **打包散**：每块一个文件（`<cabinet_id>_<target>.obj`），2880 个文件没法导。

### 1.1 目标输出格式（参考模型 + disguise KB 已证）

用户提供的参考模型 `led_wall_v02.obj` 实测解析 + disguise LED 工作流共同确认目标格式：

- **几何**：每箱体独立面片，**不焊接**（参考模型 11520 顶点 = 2880×4），保 1:1 逐箱体位姿。
- **UV**：一张**整体 0-1 网格**，每块占自己的格子（参考模型 U 有 121 档=120 列+1、
  V 有 25 档=24 行+1）。disguise 要求 UV snap 成 grid，否则缝隙→输出黑像素
  （KnowledgeBase: `help-disguise-one/196-step-2-mapping-content-p2-5.md`）。
- **打包**：单个 OBJ，**粘成一整块**（单一 group、不分箱体名字；几何仍是各自独立的
  4 角点，不焊接）。
- **坐标系**：+Y up、毫米→米、发光面朝向正确。

**核心认知**：几何"焊不焊接"与 UV"是不是整体"是两条**正交**的轴——顶点全不焊接
（逐箱体 1:1）与 UV 一张整体网格可以**同时成立**，参考模型即证明。

### 1.2 成功判据

- 导出单个 OBJ：N 块 → 顶点=4N、三角面=2N、UV=4N、单 group、不焊接。
- UV 为整体网格：不同 U 值个数 = cols+1、不同 V 值个数 = rows+1（对标参考模型判据）。
- 几何顶点与 pose report 的 `corners_mm` 一致（仅 mm→m），逐箱体位姿无损。
- **最终验收**（手动、用户做）：导入 disguise，内容横铺整面墙、发光面朝向对、
  xR/VP 视锥里逐箱体位置贴合现实。

## 2. 核心改动：UV 网格化（地基）

唯一的核心改动在 `panel_surface`：让它知道这块的列/行、总列/行数，把 UV 从"铺满 [0,1]"
改成"这块自己的格子"。

- UV 格子：`U ∈ [col/cols, (col+1)/cols]`，`V ∈ [row/rows, (row+1)/rows]`。
- 列/行从 `cabinet_id` 解析（`V<col>_R<row>`，`reconstruct.py:_cabinet_id` 写的格式，
  3 位零填充；解析取尾部 `V\d+_R\d+`）。
- 总列数/行数从全体 `cabinet_id` 反推：`cols = max(col)+1`、`rows = max(row)+1`。
- **几何顶点完全不动**，只改 UV。
- V 轴向上递增（row 0 = 最底排 = V 0），对齐 disguise / 3ds Max 左下角原点约定
  （与 `crates/core/src/uv.rs:compute_grid_uv` 现有约定一致）。
- **异形屏天然兼容**：缺的 `(col,row)` 没有条目 → 那格不出箱体、不占 UV，正确。

### 2.1 实现位置决策

UV 在 **Rust 导出层**算（不改 pose report 格式、不动 Python、不改 schema）。
理由：UV 是纯导出关注点，塞进重建产物（Python/DTO）会让职责不清；当前只一个调用点，
不需要通用 grid-layout 模块（YAGNI）。

## 3. 单文件打包 + 朝向

### 3.1 打包

现状是循环 2880 次、每次 `write_obj` 一个文件。改成把所有块**拼进一个大网格**再写一次：

- 遍历每块 → 4 顶点（mm→m）、2 三角面（索引按累计偏移）、4 格子 UV；
- 累加进单个 `MeshOutput`（顶点不去重、不焊接）；
- 调一次 `write_obj`。`write_obj`（`crates/core/src/export/obj.rs`）写单一 `g screen_mesh`
  ——正好是"粘成一整块、不分名字"。
- 结果：1 个 OBJ，顶点=4N、三角面=2N、UV=4N、单 group。
- `--root`（以某块为基准摆正）/ `--ground`（底边贴地）保留，作用在合并后的整块上。

### 3.2 朝向（xR/VP 关键）

LED 屏有正反面，发光面必须朝向正确（凹面/朝中心），否则相机里看到背面/被剔除。

- 现状：pose report 角点带 BL,BR,TR,TL 顺序（`reconstruct.py:_active_surface_corners_mm`，
  CCW from bottom-left）+ "+Z 朝外=发光面"约定；导出按
  `TargetSoftware::Neutral` **原样输出**（+Y up 已是 disguise native，charuco y-up 修复后
  无需 core→target 适配器）。理论上朝向正确。
- 处理：**保持现有顶点绕序，不额外写法线**——写 `vn` 要动 Path A 也在用的共享
  `write_obj`，超出本次 Path B 范围。靠绕序定正反面。
- **诚实边界**：朝向最终靠模拟测试在 disguise 里实看确认。若内容显示在背面，修复局部
  ——只在 Path B 这条路翻转每块角点顺序，不碰 Path A。

### 3.3 单位 / 坐标系

已正确（mm/1000→米、+Y up、Neutral 原样），不动。

## 4. CLI 接口变化

命令名不变（`lmt export pose-obj`），改输出参数与行为：

```
现在: lmt export pose-obj <pose_report> <target> --out-dir <目录>  [--root ..] [--ground]
改后: lmt export pose-obj <pose_report> <target> --out <文件.obj>  [--root ..] [--ground]
```

- `--out-dir <目录>` → `--out <文件路径>`（单文件）。
- grid UV + 单文件整块为**唯一行为**，不做开关（满幅 [0,1] 那种对 disguise 无用，无保留价值）。
- **删除**旧的"每块一个文件 + 满幅 UV"模式（调用点与兼容性决策见 4.1）。
- destructive（写文件）保留 `gate_destructive` + `--yes` / `--dry-run`；dry-run 仍走
  `check_pose_obj_inputs` 校验（report 可读、非空、`--root` 存在）。
- 返回结果 DTO：`ExportPoseObjResult` 由 `{ target, cabinet_count, files: Vec<String> }`
  改为 `{ target, cabinet_count, file: String }`（cabinet_count = 合并了多少块）。

### 4.1 兼容性与调用点（已 grep 核实，2026-05-28）

这是一个 breaking change（`--out-dir`→`--out`、`files`→`file`）。**不加**任何向后兼容
shim（deprecation 别名 / DTO 双字段 / schema 版本）——遵 CLAUDE.md "Don't use
backwards-compatibility shims when you can just change the code"；且旧输出对 disguise
本就不可用、无真实消费者，留 deprecation 期无价值。做法：**所有调用点一次性同步改**。

全仓 `pose-obj` / `ExportPoseObjResult` / `--out-dir` 调用点（grep 实测）：

| 位置 | 现状 | 改动 |
| --- | --- | --- |
| `crates/lmt-cli/src/cli.rs:261-269` | `pose-obj` 子命令 `out_dir: PathBuf` | `--out-dir`→`--out`（文件路径） |
| `crates/lmt-cli/src/commands/export.rs:138-207` | `pose_obj()` 用 `out_dir`（dry-run 文案 + 调用） | 改 `--out`；dry-run 预览文案改单文件 |
| `crates/lmt-app/src/export.rs` | `run_export_pose_obj` / `check_pose_obj_inputs` / 结果构造 + 单测(412-521) | 签名改单文件 + 合并 mesh；更新单测 |
| `crates/lmt-shared/src/dto.rs:333` | `ExportPoseObjResult{..files:Vec}` | 改 `file:String`（保 JsonSchema） |
| `crates/lmt-shared/src/schema.rs:77,131` | schema dump 注册 | 随 DTO 改自动覆盖；确认 dump 通过 |
| `crates/lmt-shared/src/manifest.rs:119` | 命令字符串含 `--out-dir`、Result 指 `ExportPoseObjResult` | 字符串改 `--out` |
| `docs/agents-cli.md:39` | 命令表行 `--out-dir`、`Result {…files}` | 改 `--out`、`{…file}` |

**无 Tauri command、无前端 GUI 调用**（`src-tauri/`、`src/` grep 均为空）→ 原"同步 Tauri
shim + GUI"删除。**无 in-repo JSON 消费 `.files`**（CLI 只透传 envelope）。`compare-known`
用 pose report 的 JSON，与本 OBJ 导出无关。

## 5. 测试

### 5.1 单元测试（核心逻辑）

1. `cabinet_id` 解析：`V012_R007` → (12,7)；格式不对报错。
2. 网格维度反推：一组 id → cols=max列+1、rows=max行+1。
3. UV 格子：`V000_R000` → `[0,1/cols]×[0,1/rows]`；`V060_R000` → U≈0.5（对标参考模型实测值）。
4. 合并网格：N 块 → 顶点=4N、三角面=2N、UV=4N、单 group、**不焊接**（验证接缝重复坐标保留）。
5. 异形屏：缺块的 (col,row) 不出箱体，维度仍正确。
6. **整体结构回归**：合成 cols×rows pose report → 导出后断言"不同 U 值个数=cols+1、
   不同 V 值个数=rows+1"（把参考模型判据固化为回归测试）。

### 5.2 E2E（CLI 契约四类，各至少一条）

- happy：正常 report → 单 OBJ，顶点/面/UV 数对、UV 铺满 0-1 网格。
- refuse：不安全 `cabinet_id`（路径穿越）/ 越界被拒。
- dry-run：`--dry-run` 只校验、不写文件。
- error envelope：空 report / `cabinet_id` 解析失败 / `--root` 指向不存在的块 → 规范错误信封
  + 退出码。

### 5.3 模拟测试（手动验收，不在自动化范围）

用户后续在 disguise 里做，是功能"真正完成"的验收标准（见 1.2）。若朝向错，修复=3.2 的局部翻转。

## 6. 范围外（明确不做）

- **Path A（全站仪 → `reconstruct` → `run_export`）任何改动**：它是焊接连续曲面范式，
  天生做不了逐箱体独立位姿；本次不动，维持其"连续曲面"语义。
- **Path A 逐箱体拟合重建**：2880 块每块需 3-4 测点 = 上万点，现场不可行，不做。
- **给共享 `write_obj` 加法线输出**：会改 Path A 输出，超范围；朝向靠绕序 + 模拟测试兜底。
- **内容模板图（UV 渲染图）生成**：给内容创作者用的 template，属另一关注点，本次不做。
- **多屏世界场景合并**：本次只处理单屏（一个 pose report）。

## 7. 交付清单（CLI 契约）

1. **lmt-shared**：`ExportPoseObjResult` 改单文件结构（`file: String`，保 `JsonSchema` +
   在 `schema::dump_all()` 内）；`manifest.rs:119` 命令字符串 `--out-dir`→`--out`。
2. **lmt-app**：`panel_surface` 接 (col,row,cols,rows) 出网格 cell UV；`run_export_pose_obj`
   改为合并单 `MeshOutput` + 单 `--out` 路径 + 解析 id/反推维度；删旧 N-文件分支；更新单测。
3. **lmt-cli**：`cli.rs` 的 `out_dir: PathBuf` → `--out`；`commands/export.rs` 的 `pose_obj()`
   改单文件路径 + dry-run 预览文案；`--yes` / `--dry-run` 保留。
4. **Tauri / GUI**：pose-obj **无 Tauri command、无前端调用**（已 grep 核实），无需同步——
   原 spec 的"同步 Tauri shim + GUI"据此删除。
5. **cli_e2e.rs**：happy / refuse / dry-run / error envelope 四类。
6. **docs/agents-cli.md**：命令表第 39 行更新（`--out`、`Result {…file}`），side_effect 不变；
   如新增错误码同步错误码表。
