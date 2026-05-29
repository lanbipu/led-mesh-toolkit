# SL 时序检测前端设计 · 靠"闪"不靠"亮" + 屏幕 ROI 隔离运动物体

> 日期：2026-05-29
> 范围：重写 `sl_decode` 的找点 / 找哨兵前端，让 Path B（点阵结构光）在现场实拍
> （又亮又花的背景 + 屏外有运动物体）下也能解；脱离 disguise 灰底素材这根拐杖。

## 1. 背景与问题

Path B：屏幕上显示白点矩阵，每个点按二进制编码闪（on/off）→ 相机录一段 → `sl_decode`
解出「点的相机像素坐标 (x,y) ↔ 屏幕坐标 (u,v)」对应关系，喂给多视图 BA 反算屏幕 3D。

当前 `python-sidecar/src/lmt_vba_sidecar/sl_decode.py` 的前端是 **naive 的、假设黑底**：

| 环节 | 现状（核实过的代码） | 现场为什么崩 |
| --- | --- | --- |
| 找哨兵 `segment_code_region` (L45–75) | `frame.mean() > sentinel_threshold*255` —— 整帧均值 | 现场背景本身就亮，整帧均值常年高，全屏白哨兵顶不出来 |
| 找点 `_centroids` (L103–106) | `cv2.threshold(frame, 128, …)` + 连通域 —— 全局 128 亮度阈值 | 背景像素远超 128 → 被当成一大坨假点；暗的斜看点反而丢 |
| 读位 `_read_bit_at` (L109–114) | 3×3 patch 均值 `> 128` —— 全局 128 | 同上，亮度判据在现场失效 |
| 切段 `index_plateaus` (L78–100) | 帧间变化像素数（已是时序量）| 本身没问题，但全局统计会被屏外运动物体污染 |

灰底合成素材（disguise，背景 ≈64 < 128）现在能 100% 解，**纯属巧合**：背景恰好低于
128 阈值。这不是鲁棒性，换成现场亮背景必崩。

**核心问题**：单帧里"屏幕白点"和"亮墙 / 反光 / 窗 / 旁边的屏"都是亮像素，亮度上无法区分。
能区分的只有"**它闪不闪**"——屏上的点按编码闪，静止背景不闪。所以"纯黑底 + 白点"的干净
图不是从某一帧抠的，是**从整段录像逐像素看"变没变"算出来的**。

### 1.1 现场条件（已与用户确认）

1. **机位三脚架锁死，全程不动** → 静止背景天然零变化，**不需要帧间配准**。
2. **屏外有运动物体（人 / 车等），但绝不遮挡屏幕** → 运动物体只在屏幕区域**之外**捣乱。

第 2 条把设计导向关键动作：**先把屏幕框出来（ROI），之后所有计算只在 ROI 内做**，屏外
运动物体（既会产生假点、又会污染哨兵 / 切段的全局统计、破坏同步）就被彻底隔离。

## 2. 目标与成功判据

**目标**：用「逐像素时序变化（靠闪不靠亮）+ 屏幕 ROI」替换 naive 前端，使 decode 在
任意亮度 / 纹理的静止背景 + 屏外运动物体下成立，且对大角度斜看的暗点也成立。

**成功判据**（全部以合成素材验证——现场无已知真值，合成是可控的 known-good，符合
"synthetic 是最好情况"）：

- **S1 回归**：现有灰底素材（disguise 视觉器导出）走新前端仍 **100% 解**，
  `cli_e2e.rs` 现有 SL 用例与 sidecar pytest 全绿。
- **S2 亮背景**：合成"亮 + 有纹理"背景 + 叠点序列 → 新前端解出 ≥99% 点；
  同素材走 naive 前端会失败（作为对照断言）。
- **S3 屏外运动物体**：S2 基础上，在屏幕 ROI **之外**叠一个移动的亮块（模拟走动的人）
  → 解出率不下降、不因运动物体多出错误点、哨兵 / 切段不被带偏。
- **S4 暗点 / 斜看**：合成"点亮度低于背景"的素材（暗点 < 亮背景）→ 仍正确解
  （证明判据是"变化"不是"亮度"）。
- **S5 可视化产物**：`--emit-debug-image` 产出一张"纯黑底 + 白点"的检测掩膜图，
  肉眼可核对"点抠干净没、屏幕框对没"。

## 3. 核心原理与一处诚实的边界

### 3.1 把判据从"亮度"换成"变没变"

逐像素求时序极差 `range = max − min`：

- 静止背景（哪怕是 250 的白墙）：`range ≈ 0` → 判黑、不是点。
- 闪烁的点（哪怕斜看只在 40↔90 之间）：`range` 大 → 判点。

这与 128 阈值的毛病**完全相反**：128 会留下白墙、丢掉暗点；时序极差留下暗点、丢掉白墙。
绝对亮度不再进入判断，所以"任意背景成立"成立，灰底 / 白底 / 花背景一视同仁。

### 3.2 一处必须讲清的边界（不偷换概念）

"靠闪不靠亮"负责两件事：**(a) 确定屏幕在哪（ROI）**、**(b) 怎么读每个点的位**。但
**点中心的精确定位（seeding）仍要用 anchor 帧（all-on，每个点都亮）**——因为 id=0 的点
在所有 code 帧里**从不亮**，纯时序极差找不到它，只有 anchor 里有它。

关键在于：anchor 的亮度判据**只在 ROI 内做**。ROI 内的"背景"是屏幕自己的黑（点与点之间），
不是屏外的亮墙；屏外的亮东西已被 ROI 排除。而 ROI 本身是从时序活动图推出来的（靠闪）。
所以链条自洽：**靠闪定 ROI → ROI 内 anchor 自适应阈值定点 → 靠闪读每点的位**。
没有把"找亮矩形"偷偷塞回主路径。

## 4. 架构：三遍管线（全部在 Python sidecar 内）

所有 CV 改动落在 `sl_decode.py`；Rust 侧只加参数并透传。

### Pass 1 — 粗 ROI（从全片活动图）

对**整段**逐像素求 `range`。屏幕矩形因为哨兵（全屏白）刷过 + 点在闪，整片高活动且呈
**实心矩形**；屏外运动物体是**分离、细长、不实心**的活动块（且与屏幕不重叠——不遮挡）。
取最大的实心矩形活动连通域的 bbox = 粗 ROI。

- 提供 `--screen-roi x,y,w,h` 手动覆盖（机位锁死 → 全片同一框），作为兜底 / 调试。
- 自动失败（找不到实心矩形）→ `detection_failed`（13），消息提示"建议手动指定 --screen-roi"。

> 注意：Pass 1 用整段（含哨兵）求 range，目的只是框屏，不是抠点——哨兵会把整个屏幕刷亮
> 正好让屏幕成实心块，利于框定。抠点在 Pass 3 用"仅 code 区"的 range，避开哨兵污染。

### Pass 2 — 同步（只在 ROI 内）

把 `segment_code_region` 与 `index_plateaus` 的全局统计**限制到 ROI**：

- 哨兵：`roi_mean = frame[roi].mean()`，全屏白时 ROI 接近饱和 → 仍清晰可辨。
  `sentinel_threshold`（现有 flag，默认 0.85）**语义不变、计算域改成 ROI**（不删 flag，
  避免动刚合进来的契约链）。
- 切段：`index_plateaus` 的帧间变化像素数只在 ROI 内统计。

屏外运动物体在 ROI 外 → 完全影响不到同步。这是比"抠点"更根本的收益（运动物体最危险的
不是多几个假点，而是凭空多切一段 / 哨兵错位 → 整段解码崩）。

### Pass 3 — 抠点 + 读位 + 解码闸（只在 ROI 内）

1. **seeding（定点中心）**：在 anchor 帧、ROI 内做**自适应阈值**（Otsu / 局部对比，
   非全局 128）+ 连通域 → 候选点中心（亚像素）。anchor=all-on，所以**所有点含 id=0**都被找到。
2. **形状 / 尺寸过滤**：按 `dot_radius_px`（sl_meta 里有）滤圆形、合理大小的连通域，
   去掉混进 ROI 的大 / 不规则块（如屏面反射）。
3. **逐点读位（靠闪 + 相对自身）**：每个点的 on/off 跟**它自己**在 code 区的 min/max 比
   （取中点或对该点时间序列做二值化），不用全局 128 → 斜看暗点也能正确读。
4. **解码闸（已有，作最后一道）**：`decode_bits` 偶校验 + id 必须落在 sl_meta 的 `uv_by_id`
   里。任何漏网的假点（恒亮反射、运动物体残留）闪不出合法编码 → 解码失败被丢。

### 4.1 "纯黑底 + 白点"图（用户要的产物）

`--emit-debug-image` 把 Pass 3 第 1 步的**二值检测掩膜**（ROI 内、黑底白点）写成一张图，
存在 corr.json 旁（`<out>.debug.png`）。它就是用户说的"把现场照片转成纯黑底白点"的那张图，
既是管线中间产物、也是肉眼核对的验证素材。v1 只出这一张（YAGNI）；原始 range 图按需再加。

## 5. 接口契约改动（遵守 CLAUDE.md CLI 契约，逐项列）

### 5.1 sidecar IPC（`python-sidecar/.../ipc.py`）

`DecodeStructuredLightInput` 新增（均 Optional，保持向后兼容）：

- `screen_roi: tuple[int,int,int,int] | None`（默认 None = 自动）
- `emit_debug_image: bool = False`

`sentinel_threshold` 保留不变（语义改为对 ROI-mean）。**不新增** variance 阈值字段——
Pass 1/3 的 range 阈值走 Otsu 自动（v1 YAGNI，需调再说）。

`run_decode_structured_light` 分支：始终走新前端（不留 legacy 模式开关——一条鲁棒路径，
新前端在灰底素材上必须同样 100%，由 S1 回归守住）。返回的 ResultData 之外，corr.json 旁
可选写 debug 图。

### 5.2 CLI（`crates/lmt-cli/`）

- `cli.rs` `VisualCmd::DecodeStructuredLight`（现有 `input_path, sl_meta, out, sentinel_threshold`）
  加：`--screen-roi <X,Y,W,H>`（`Option<String>`，CLI 层解析校验格式）、`--emit-debug-image`（bool flag）。
- `commands/visual.rs::decode_structured_light`（L383）：解析 `--screen-roi` 字符串→四元组
  （格式错 → `INVALID_INPUT` 2，**在 destructive gate 之前**校验，与 reconstruct 的 `>=2 corr`
  前置校验同款），透传给 lmt-app；destructive gate（写 corr.json + 可选 debug 图）不变。
  dry-run 的 `would_write` 数组在有 `--emit-debug-image` 时要含 debug 图路径。

### 5.3 lmt-app + adapter

- `lmt-app/src/visual.rs::run_decode_structured_light`（L537）签名加
  `screen_roi: Option<[u32;4]>, emit_debug_image: bool`，透传进 `DecodeStructuredLightArgs`。
- `adapter-visual-ba/src/api.rs::DecodeStructuredLightArgs`（L480）加同名字段；
  IPC JSON payload（L501）按 `sentinel_threshold` 同款，`if let Some` 条件注入 `screen_roi`、
  `emit_debug_image`。
- transport 层（`sidecar.rs` / `locate.rs`）不动。

### 5.4 DTO（`crates/lmt-shared/src/dto.rs`）

`DecodeStructuredLightResult`（现 `output_path, n_dots_decoded`）加（均 Optional，
backward-compatible）：

- `debug_image_path: Option<String>` —— 写了 debug 图时返回其路径。
- `screen_roi: Option<[u32;4]>` —— 实际用的 ROI（自动或手动），供操作者核对"框对没"。

二者均纯类型，已有 `#[derive(JsonSchema)]`，自动进 `schema::dump_all()`（schema.rs L76 区）；
无需进 `incomplete` 列表。`schema dump` 测试覆盖。

### 5.5 错误码（**v1 不新增**，复用现有，省契约 churn）

- ROI 自动检测失败 / 点太少 → 复用 `detection_failed`（code `detection_failed` / exit 13），
  仅**消息**具体化（"未能定位屏幕点阵簇，建议手动 --screen-roi"）。
- 哨兵 / 切段 / 解析失败 → 复用 `decode_failed`（exit 18）。

不动 `error_codes` / `exit_codes` / 二者 1:1 映射测试，不动 `ContractManifest` 的 26-op 断言
（不新增 operation）。若日后要区分 ROI 失败，再补 `roi_not_found` 并三处同步——此处记录、v1 不做。

### 5.6 manifest + agents-cli.md

- `lmt-shared/src/manifest.rs` decode op 的 CLI 字符串加 `[--screen-roi X,Y,W,H] [--emit-debug-image]`；
  `exit_codes` 仍是 `[0,2,3,4,13,18]`（无新增）。
- `docs/agents-cli.md`：命令表第 44 行更新签名 + 说明（时序检测、ROI、debug 图）；
  side_effect 仍 **destructive**（写文件）；错误码表（13/18）行的触发条件补充时序前端的新情形描述。
- **不加 Tauri shim**：visual 全组按契约 CLI-only（`src-tauri/src/lib.rs` 的 14 命令不含 visual）。
  在 agents-cli.md 的 "Not exposed in CLI" 语义下维持现状即可。

## 6. 测试策略（TDD：先写失败测试再实现）

### 6.1 sidecar pytest（算法主战场）

新增合成 fixture 生成器（测试内构造，不依赖仓库外现场照片）：用现有 `structured_light.py`
生成 sl_meta + code 帧序列，叠到一张背景画布上，导出帧目录 / 视频喂 `run_decode_structured_light`：

- `test_decode_gray_bg_regression`（S1）：背景 =64 灰底（模拟 disguise 视觉器），断言 100% 解
  —— 守住"新前端在旧素材上不退化"。
- `test_decode_bright_textured_bg`（S2）：亮 + 纹理背景，断言解出 ≥99%；
  附 `test_decode_bright_bg_fails_with_naive`（对照：旧 128 路径会失败）。
- `test_decode_moving_object_outside_roi`（S3）：ROI 外叠移动亮块，断言解出率不降、无假点、同步不偏。
- `test_decode_dim_dots_below_bg`（S4）：点亮度 < 背景，断言仍正确解。
- `test_decode_finds_id0`：含 id=0（全 off 点），断言 anchor seeding 仍找到它。
- `test_roi_auto_vs_manual`：自动 ROI 与手动 `--screen-roi` 结果一致；自动失败时报 `detection_failed`。
- 改完 `python-sidecar/build_exe.sh` 重建 sidecar binary。

### 6.2 CLI E2E（`crates/lmt-cli/tests/cli_e2e.rs`）

按契约 happy/refuse/dry-run/error 四类补：

- `decode_structured_light_with_roi_and_debug_dry_run`：`--screen-roi`/`--emit-debug-image` 被解析，
  dry-run `would_write` 含 debug 图路径、不写文件。
- `decode_structured_light_invalid_roi_format`：坏 ROI 字符串 → exit 2（INVALID_INPUT），gate 前。
- happy：复用现有 sidecar（`LMT_VBA_SIDECAR_PATH` 指向 `.venv`）跑灰底素材 → 仍解出（S1 回归）。
- refuse（无 --yes/--dry-run → 2）沿用现有用例。

## 7. 范围外（明确不做，v1）

- **帧间配准 / 防抖**：机位锁死，不需要；手持是日后另立里程碑。
- **matched-filter（拿已知编码与每像素时间曲线相关）**：最强但把检测与解码耦死，后续加固项。
- **逐帧背景建模**（应付缓慢光照漂移）：靠闪 + 锁曝光已覆盖主路径，按需再上。
- **屏面玻璃上反射的运动物体**（在 ROI 内、会动）：靠形状过滤 + 解码闸兜底，不为它过度设计；
  残留风险已与用户对齐。
- **新错误码 `roi_not_found`**：见 5.5，复用 `detection_failed`。
- **现场实拍验证**：暂无现场素材，以合成 known-good 验收（S1–S5）。

## 8. 拍摄 SOP（写给现场，不是设计选项）

相机**锁死曝光 / 对焦 / 白平衡**，全程三脚架不动。自动曝光一"呼吸"，整帧亮度漂移会被
时序差分误判成"全在变"，ROI 内也救不了。这是纪律。

## 9. 交付清单

① sidecar：`sl_decode.py` 三遍管线（Pass1 ROI / Pass2 ROI 内同步 / Pass3 时序抠点+相对读位+debug 图）
+ `ipc.py` 三字段；② sidecar pytest（S1–S5 + id0 + roi）+ 重建 binary；
③ `cli.rs`/`commands/visual.rs` 两 flag + ROI 解析校验；④ `lmt-app`/`adapter` 透传；
⑤ `dto.rs` 两 Optional 字段（+ schema dump）；⑥ `manifest.rs` CLI 串；⑦ `cli_e2e.rs` 新用例；
⑧ `docs/agents-cli.md` 命令表 + 错误码触发条件描述。任一缺失视为未完成。
