# disguise 导出 = 一条确定命令(标准摆法)

> 日期：2026-05-28
> 范围：让 `lmt export pose-obj ... disguise` 默认产出一个**确定、可预期摆放**的模型,不靠手动 `--root`/`--ground`。
> 背景：本会话早些时候导 bench 墙时漏带 `--root V000_R001 --ground` → 给了摆放错误的模型。根因是「正确摆法靠人记参数」这一设计本身脆弱。

> **修订 v2(2026-05-28,回应 Codex adversarial review,三条 finding 全采纳)**：
> - F1：把一致性契约从"任意旋转后一致"**收窄为"只规范 yaw + 平移"**,明确 +Y=面板竖直的前提,真实倾斜刻意保留(保真)。测试改 yaw+平移不变性,不是全 SO(3)。
> - F2：前向**对齐 Path A 的 disguise 约定**(发光面→-Z),并加 Path A vs Path B golden 测试钉死,不靠人工"导反改一行"。**实现修正**:golden 实测 Path A 发光面落 **-Z**(本 spec 初判 +Z 有误,已全文改为 -Z;正是 golden 在人工验收前抓出来的)。
> - F3：朝向主方法从"平均法向"换成**中心列朝向**(≥180° 墙如 bengtie 184° 也成立);真正退化时 **fail-fast 报错要求 `--root`**,不静默产出伪标准摆法。

## 1. 背景与目标

### 1.1 问题

`lmt export pose-obj` 的默认(不带摆放参数)= **原始 BA 帧**：不贴地、不居中、朝向挂在重建根箱体(`V000_R000`)上 = 随机。要得到能用的摆放,得手动带 `--root <cabinet> --ground` —— 这套参数是某个现场的固定属性,却要每次导出靠人记忆带上,极易出错(本会话已踩坑)。

### 1.2 关键事实(已核源码)

- **反算原始朝向本就不一致**：全站仪挂架站、ArUco-BA 挂根箱体,都任意。
- **Path A(`reconstruct surface` → `export obj`)早已从几何重推朝向**：`crates/core/src/reconstruct/surface_fit/frame.rs` 的 `derive_cylinder_frame`(origin=弧左下角、定向用**弧中点法向 θ_mid**)、`derive_plane_frame`(PCA),再 `adapt_to_target` 到 disguise。所以 Path A 的 disguise 导出与架站无关、朝向一致。
- **Path A disguise 朝向约定**(`crates/core/src/export/adapt.rs`)：`Disguise` 适配 `(x,y,z)→(x,z,-y)`,模型凸(外)法向 `+Y → disguise -Z`;`build.rs` 反 winding 使**发光/凹面落在 -Z(观众侧)**。
- **Path B(pose-obj)目前不做这件事**：直接用原始 BA 帧 → 朝向随根箱体乱跑,这是病根。
- **pose report 帧的"上"**：VBA 世界系 = 根箱体 active-surface frame,**+Y 是面板竖直方向(板坐标 y-up),不是重力垂直**。无重力参照。

### 1.3 目标

给 Path B 的 `disguise` 导出补上**从几何重推摆放**(Path A 用弧中点法向;Path B 墙可能带逐箱体误差、无干净弧,用**中心列朝向**的稳健类比):
- `lmt export pose-obj <报告> disguise --out <文件>` **不带摆放参数**就产出确定、可预期摆放的模型。
- 摆放是**对整面墙的一次刚性搬动**,逐箱体真实偏差(倾角/错位/后仰)**完全保留 1:1**。
- 用户在 disguise 里**实看确认**;不满意用 `--root` 覆盖。

### 1.4 成功判据

- `disguise` 不带参数 → 输出贴地(min Y=0)、水平居中、发光面朝固定前方(-Z,对齐 Path A)。
- **一致性(收窄契约)**：同一面墙,原始帧叠加**任意 yaw(绕竖直轴)+ 平移**后再导,输出摆放**逐顶点一致**。**非 yaw 旋转(真实 roll/pitch/倾斜)被原样保留**,不强行摆平(保真;见 §3 说明)。
- 逐箱体相对位姿(法向夹角/相对平移)在标准摆法前后不变。
- **前向契约由 golden 测试钉死**:同一合成墙经 Path A 与 Path B 导出,发光面落同一轴、winding/UV 一致。
- **退化不静默**:无法定向的墙 → 报错要求 `--root`,不产出伪标准摆法。
- 最终验收:disguise 实看(手动)。

## 2. 命令与触发(target 驱动)

命令签名**不变**:`lmt export pose-obj <pose_report> <target> --out <path> [--root <cabinet_id>] [--ground]`。只改**默认行为**:

- **`disguise`(无 `--root`)** → 自动**标准摆法**(§3)。日常唯一形态。
- **`neutral`(无 `--root`)** → **原始帧**(调试),老行为不变。
- **`--root <id>`(任何 target)** → **手动模式**:按该箱体定帧 + 可选 `--ground`,老行为。标准摆法关闭。
- `unreal`:本轮**范围外**,保持现状(原始帧)。

**无新增 flag、无 DTO/schema/CLI 签名变更**——只是 `disguise` 默认从「原始帧」变「标准摆法」。

## 3. 标准摆法定义

仅当 `target==disguise && root==None`,对所有箱体角点做**一次刚性变换**(只转+平移,不改形状):

1. **求前向(中心列朝向,Path A θ_mid 的稳健类比)**:
   - 每块发光面外法向 = `CabinetFrame::from_corners(corners).z`(已有;退化块跳过)。
   - 取**中心列** `c_mid = round((cols-1)/2)`;若该列无在位箱体,取最近的非空列。
   - `n_fwd` = 该列**所有在位箱体法向的平均**,归一化。
   - `n_h = (n_fwd.x, 0, n_fwd.z)`(水平分量)。
   - **中心列朝向比"全墙平均法向"稳健**:≥180° 包角墙(如 bengtie 184°)平均法向会互相抵消,中心列朝向仍明确。
2. **绕 +Y 转正**:`θ = atan2(n_h.x, n_h.z)`;对所有顶点绕 +Y 旋转使 `n_h → -Z`(= 先 R_y(-θ) 到 +Z 再绕 Y 转 180°;实现里直接 `x'=-(x·cosθ - z·sinθ)`,`z'=-(x·sinθ + z·cosθ)`)。**只转 yaw;+Y(上)不变。**
   - **前向 = -Z,对齐 Path A**:Path A 发光/凹面落 -Z(见 §1.2);Path B 发光面外法向(`CabinetFrame.z`)朝观众,转到 -Z 即与 Path A 一致。**此约定由 §5 的 Path A vs Path B golden 测试钉死(Path A 为准)。**
3. **贴地**:`y -= min_y`(全顶点)。
4. **居中**:`x -= mean_x`,`z -= mean_z`(全顶点水平质心到原点)。

**为什么只转 yaw、不摆平**:LED 墙是立着的,"上"=面板竖直(+Y);唯一**任意、需固定**的自由度是水平朝向(yaw)。墙的真实倾斜(后仰/不平)是要**保留**的真实几何——强行摆平等于丢掉本项目要保的偏差。

**契约边界(诚实)**:本变换假设 pose report 的 +Y 是面板竖直(见 §1.2)。它**只去掉任意 yaw + 任意平移**;若整墙相对重力本身是斜的,该倾斜被保留(真实几何)。因此"同一面墙跨不同架站/根箱体导出一致"**仅在两帧只差 yaw+平移时成立**;若两帧的"上"不同(非 yaw 旋转),全姿态不保证一致——以 disguise 实看为最终对齐。

**几何仍按 `TargetSoftware::Neutral` 原样输出**:标准摆法保持 +Y up,不套 `adapt_to_target` 轴适配器(沿用现状)。

### 3.1 退化处理(fail-fast,不静默)

若中心列水平法向 `|n_h| < 1e-6`(墙朝向接近垂直 = 病态,非正常 LED 墙)→ **返回 `LmtError::InvalidInput`**("cannot auto-orient: wall normal near-vertical; pass --root <cabinet_id>"),**非零退出、不写文件**。决不静默产出继承原始随机 yaw 的伪标准摆法(自动化会误当 canonical 用)。用户用 `--root` 显式定帧即可。

## 4. 确认 / 覆盖

- **确认**:导出后在 disguise 里瞄一眼朝向。
- **覆盖**:`--root <箱体>` → 手动按该箱体定帧(老行为),标准摆法关闭。或在 disguise 里直接转(绝对对齐本就在 disguise 做)。

`disguise` 不给 `--root` 时 `--ground` 多余(标准摆法已贴地),按 no-op。

## 5. 测试

**单元(lmt-app)**:
- **yaw+平移不变性(契约核心)**:合成墙角点叠加**任意 yaw + 平移**后,标准摆法输出与未扰动版**逐顶点一致**(容差内)。**显式记录**:叠加**非 yaw 旋转**(roll/pitch)时输出**会**带上该倾斜(契约如此,不是 bug)——加一条断言锁定"真实倾斜被保留"。
- **转正正确**:朝任意水平方向的平墙 → 标准摆法后中心列法向水平分量 ≈ (0,0,1)、min Y ≈ 0、水平质心 ≈ 原点。
- **保偏差**:两块相对位姿在标准摆法前后不变。
- **退化 fail-fast**:中心列法向近垂直 → `InvalidInput`(不 panic、不写文件)。
- **中心列稳健性**:≥180° 包角合成墙(平均法向≈0)仍能定出明确前向(用中心列)。

**Path A vs Path B golden(钉死前向契约,F2)**:
- 构造一面合成墙,**两条路各导一次 disguise OBJ**(Path A:喂 reconstruct surface;Path B:喂等价 pose report)。断言**发光面落同一轴(-Z)、winding 一致、UV 方向一致**。Path A 为权威基准。

**行为变更**:`export_pose_obj_disguise_target_equals_raw_world_frame`(假设 disguise==原始,已不成立)→ 改为「disguise=标准摆法(朝 -Z、贴地)、neutral=原始」。现有用 `neutral` 的 E2E/单测不受影响。

**E2E**:新增 `disguise` 不带参数 → 贴地/居中/朝前;退化墙 → 错误信封 + 非零退出 + 不写文件;`--root` 仍覆盖;`neutral` 仍原始。

**手动验收**:disguise 实看(最终拍板)。

## 6. 范围外

- `unreal` target 的 pose-obj 标准摆法(保持现状)。
- 改 Path A(`reconstruct surface` 导出)——已有 frame 推导,不动。
- 绝对场地坐标对齐(由 disguise 舞台标定负责)。
- 前向轴可配置化(固定 -Z + golden 钉死 + `--root` 覆盖即可,YAGNI)。
- 用重力/IMU 等外部参照把墙摆平(本项目无此输入;真实倾斜刻意保留)。

## 7. 交付清单(CLI 契约)

1. **lmt-app**(`crates/lmt-app/src/export.rs`):新增标准摆法纯函数(中心列前向 / 绕 Y 转正 / 贴地居中 / 退化 fail-fast);`run_export_pose_obj` 按 §2 优先级分支(root→手动;disguise→标准摆法;neutral→原始);更新 disguise 单测。
2. **单元测试**:§5 全部(含 yaw+平移不变性、保倾斜、退化 fail-fast、≥180° 稳健)。
3. **golden 测试**:Path A vs Path B 同一合成墙的发光面轴/winding/UV 一致。
4. **cli_e2e.rs**:disguise 标准摆法 case、退化错误信封 case;确认 `--root` 覆盖、`neutral` 原始仍过。
5. **docs/agents-cli.md**:`export pose-obj` 行补「disguise 默认=标准摆法(中心列转正+居中+贴地);neutral=原始;--root 手动覆盖;无法定向→报错要 --root」;同步 `manifest.rs` 描述串(命令字符串不变)。
6. **无 DTO/schema/CLI 签名变更**。退化错误复用 `INVALID_INPUT`(无需新错误码)。
