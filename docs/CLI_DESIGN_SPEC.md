# AI-Native App Interface Specification (v3.0)

> 适用对象：Claude Code 等 AI Agent 开发与调用的工具/服务接口
> 设计目标：Contract-first，CLI 强制 + 可选适配器（MCP / HTTP / Skill）按需扩展
> v3.0 关键变更：
>   - CLI 定位为强制基础，MCP/HTTP/Skill 改为可选
>   - 引入 Contract Manifest 与 operation_id 作为跨适配器 canonical 标识
>   - MCP 响应升级到 structuredContent + outputSchema（对齐 2025-11-25 规范）
>   - MCP 错误语义按 protocol vs tool-execution 拆分
>   - 移除"非 TTY 自动切 JSON"行为，改为显式 --output 或 AI_AGENT=1
>   - Skill 定位明确为 Agent Experience Layer，不是第三套 API

---

## 0. 设计哲学

### 0.1 核心命题
让 App 同时被人类、shell/CI、AI Agent 可靠调用，**业务逻辑只写一次**。

### 0.2 接口分层
```
            Core SDK（业务逻辑，必须）
                   ↓
         Contract Manifest（接口契约，必须）
                   ↓
     ┌─────────────┼─────────────┐
     ▼             ▼             ▼
 CLI Adapter   MCP Adapter   HTTP Adapter
 （必须）       （可选）       （可选）
                   │
                   ▼
            Skill Package
         （可选，AI host 专用）
```

### 0.3 适配器层级与必要性
| 层 | 是否必须 | 何时需要 |
|---|---|---|
| Core SDK | **必须** | 任何 App |
| Contract Manifest | **必须** | 任何 App |
| CLI | **必须** | 任何 App（人类 / CI / 通用 Agent 的兜底） |
| MCP Server | 可选 | 主要消费者为 Claude Code / MCP host |
| HTTP API | 可选 | 跨网络 / 跨语言调用 |
| Skill Package | 可选 | 为 Claude Code 提供路由策略、workflow 封装、安全约束 |

**核心准则**：CLI 是 Unix 生态事实通用语，永远是最稳兜底。可选适配器按需添加，**禁止用任何可选适配器替代 CLI**。

### 0.4 命名澄清（v3.0 修订）
v2.0 中"POSIX-Compliant"措辞不严谨——严格 POSIX 偏好单字符短选项，GNU 在其上才加入 `--long-option`。v3.0 改用精确表述：

> **POSIX-compatible process semantics + GNU-style long options + AI-stable CLI profile**

含义：
- **POSIX 语义**：exit code、stdout/stderr 分离、stdin、`--` 分隔符、`-` 表示 stdin/stdout
- **GNU 长选项**：`--config`、`--output`、`--dry-run`
- **AI-stable profile**：文档示例与 AI 调用规范收敛到 canonical syntax（详见 §3.1）

---

## 1. Core SDK 规范

### 1.1 适配器无关性（替代 v2.0 "纯函数"要求）
v2.0 要求 "纯函数" 对真实业务（数据库、文件、外部 API、GPU/渲染节点）过严。v3.0 修订为：

> Core SDK **不依赖任何具体适配器**；所有 IO、时间、随机数、环境变量、认证上下文、文件系统、网络 client **必须通过显式 dependency injection 进入**。

### 1.2 三层结构（推荐）
```
Domain Models / Schemas    ← 数据模型、契约定义
        ↓
Application Services       ← 业务用例
        ↓
Ports (interfaces)         ← Storage / Network / FS / Clock / Auth
        ↓
Adapters (CLI/MCP/HTTP)    ← 仅此层做 IO，引用 Ports
```

### 1.3 禁止 / 允许
- **禁止**：直接调用 `print` / `input` / `sys.exit` / 直接读取 `os.environ` / TTY/ANSI 操作 / CLI 参数解析 / JSON-RPC 协议处理
- **允许**：访问数据库、文件、外部 API、队列等 IO——**前提是通过注入的 Port 接口**

### 1.4 Schema 集中定义
所有数据 schema 在 Core SDK 中定义一次（推荐 Pydantic / Zod / serde struct），所有适配器引用同一份定义。

### 1.5 测试要求
- Core SDK 单元测试覆盖率 ≥ 80%
- 适配器集成测试不允许 mock Core SDK（必须真实调用）

---

## 2. Contract Manifest（v3.0 新增章）

v2.0 用 `app schema` 输出 CLI 结构，但 CLI / MCP / HTTP 各自的结构定义存在漂移风险。v3.0 将其升级为**单一 Contract Manifest**，所有适配器从此生成或校验。

### 2.1 Manifest Schema
```json
{
  "contract_version": "1.0",
  "operations": [
    {
      "operation_id": "users.create",
      "summary": "Create a new user account",
      "input_schema":  { "...": "JSON Schema" },
      "output_schema": { "...": "JSON Schema" },
      "error_schema":  { "...": "JSON Schema" },
      "side_effects": {
        "writes": true,
        "external_calls": false,
        "idempotent": false
      },
      "exit_codes": [0, 2, 4, 6],
      "cli": {
        "command": "app users create",
        "supports_stdin": true,
        "supports_dry_run": true
      },
      "mcp": {
        "tool_name": "users_create",
        "annotations": {
          "readOnlyHint": false,
          "destructiveHint": false,
          "idempotentHint": false,
          "openWorldHint": false
        }
      },
      "http": {
        "method": "POST",
        "path": "/v1/users"
      }
    }
  ]
}
```

### 2.2 operation_id 是 canonical 标识
- 所有适配器引用同一 `operation_id`
- **不强制** CLI 子命令、MCP tool 名、HTTP path 字面一致——允许各适配器优化自身用户体验
- **但必须可机器对齐**：每个适配器自描述输出均包含 `operation_id`，便于一致性测试

### 2.3 一致性测试原则
不直接 diff 两套结构（结构天然不同）。**先 normalize 到 manifest，再 diff**：
```bash
./app manifest --output json | jq -r '.operations[].operation_id' | sort > /tmp/ops.txt
# CLI 子命令、MCP tools/list、HTTP OpenAPI 提取的 operation_id 集合必须 = /tmp/ops.txt
```

---

## 3. CLI Adapter（必须实现）

### 3.1 语法规则：parser 宽容，文档严格
关键原则：**parser tolerance for humans, doc strictness for AI**。

| 维度 | parser 接受 | AI 文档 / canonical 示例 |
|---|---|---|
| 长 / 短选项 | 二者皆可 | 仅长选项 |
| `--key value` vs `--key=value` | 二者皆可 | 仅 `--key value` |
| 合并短选项 `-xyz` | 接受 | 不出现 |
| 布尔标志 | `--enable-x` / `--no-enable-x` | 显式 long form |

这避免了 v2.0 "强制禁止 `--key=value`" 对人类用户过苛的问题。

### 3.2 必须支持的 flags
| Flag | 用途 |
|---|---|
| `--help`, `-h` | 人类可读帮助；exit 0 |
| `--version` | 版本号；exit 0 |
| `--yes`, `-y` | 跳过所有交互确认 |
| `--dry-run` | 模拟执行不产生副作用 |
| `--config <path>` | 加载配置文件（YAML/TOML/JSON） |
| `--output <fmt>`, `-o` | `text` / `json` / `ndjson`（`stream-json` 为别名） |
| `--input-format <fmt>` | stdin 输入格式 |
| `--log-level <lvl>` | `debug` / `info` / `warn` / `error` |
| `--verbose`, `-v` / `-vv` | 累积式日志级别 |
| `--quiet`, `-q` | 等价 `--log-level error` |
| `--no-color` | 禁用 ANSI（额外尊重 `NO_COLOR` env） |
| `--no-input` | 拒绝任何交互提示（AI 调用强烈推荐） |
| `--` | POSIX 位置参数分隔符 |

### 3.3 stdin / stdout / stderr 契约
- 文件名 `-` 表示 stdin / stdout
- 嵌套数据通过 stdin JSON 传入，**禁止**展平为大量 flag
- **stdout** 在 `--output json|ndjson` 模式下：**仅**输出符合 schema 的数据
- **stderr**：所有日志、进度、警告、错误堆栈

### 3.4 TTY 处理（v3.0 修订）
v2.0 的"非 TTY 自动切 JSON"会破坏 `app users list | head` 等常见管道行为。v3.0 改为显式信号机制：

| 场景 | 默认 `--output` | 附加行为 |
|---|---|---|
| TTY | `text` | 启用 color / spinner |
| 非 TTY（管道） | `text` | **禁用** color / spinner / progress |
| `AI_AGENT=1` env | `json` | 触发 AI-stable profile |
| 显式 `--output <fmt>` | 用户指定 | 最高优先级 |

环境变量 `AI_AGENT=1` 是 AI Agent 调用时的**显式信号**，不依赖隐式 TTY 推断。

### 3.5 三档输出格式
- `text`：人类可读
- `json`：单次完整 JSON 对象
- `ndjson`（别名 `stream-json`）：每行一个 JSON object，长任务流式消费

---

## 4. 输出契约（CLI / MCP / HTTP 共享 envelope）

### 4.1 成功 envelope
```json
{
  "schema_version": "1.0",
  "status": "ok",
  "operation_id": "users.create",
  "data": { /* 业务负载 */ },
  "meta": {
    "request_id": "uuid-v4",
    "duration_ms": 142,
    "timestamp": "2026-05-24T10:30:00Z"
  }
}
```

### 4.2 错误 envelope
```json
{
  "schema_version": "1.0",
  "status": "error",
  "operation_id": "users.create",
  "error": {
    "code": "ARG_VALIDATION",
    "exit_code": 2,
    "message": "Field 'email' must be a valid email address",
    "retryable": false,
    "details": { "field": "email" }
  },
  "meta": { "...": "同上" }
}
```

`exit_code` 字段在 MCP / HTTP 响应中也保留，便于跨适配器共享错误处理逻辑。

### 4.3 ndjson 事件类型
每行一个 object，必须含 `type`、`sequence`、`timestamp`、`request_id`，最后一条必须 `final: true`：
```
{"type":"start",    "sequence":0, "timestamp":"...", "request_id":"...", "schema_version":"1.0"}
{"type":"progress", "sequence":1, "timestamp":"...", "request_id":"...", "completed":3, "total":10}
{"type":"item",     "sequence":2, ...,                                    "data":{...}}
{"type":"warning",  "sequence":3, ...,                                    "message":"..."}
{"type":"result",   "sequence":4, ..., "final":true, "status":"ok|error", "...":"..."}
```

### 4.4 Schema 版本化
- 顶层 `schema_version` 语义化版本 `MAJOR.MINOR`
- Breaking change → MAJOR +1；新增可选字段 → MINOR +1
- CLI / MCP / HTTP 共享同一版本号，与 `contract_version` 对齐

### 4.5 Locale 中立
- 时间戳：**强制** ISO 8601 (`2026-05-24T10:30:00Z`)
- 编码：UTF-8（无论 `LC_*`）
- 数字：`.` 小数点，无千位分隔符

---

## 5. Exit Code 体系（v3.0 细化）

| Code | 语义 |
|---|---|
| 0 | success |
| 1 | runtime / internal error（未分类） |
| 2 | CLI usage / argument syntax error |
| 3 | config error |
| 4 | auth / permission error |
| 5 | resource not found |
| 6 | conflict / precondition failed |
| 7 | timeout |
| 8 | external dependency failure |
| 9 | partial failure |
| 10–63 | app-specific reserved（须文档显式声明） |
| 126 / 127 | **POSIX 保留，应用禁止主动使用** |
| 128+N | signal convention（130=SIGINT，143=SIGTERM） |

`error.code` 承载细粒度业务语义；exit code 仅承载粗粒度分类。

---

## 6. 幂等性与 Dry-Run

### 6.1 Idempotency
所有写操作应优先设计为幂等（如 `apply` 优于 `create`）。`manifest.side_effects.idempotent` 显式声明。

### 6.2 Dry-Run
所有有副作用的操作必须支持 `--dry-run`（CLI）或 `dry_run: true`（MCP）：
- 完整参数校验与计划计算
- 不调用任何写入路径
- 输出预期变更摘要至 `data.dry_run_plan`

---

## 7. MCP Adapter（可选）

### 7.1 何时需要
- 主要消费者是 Claude Code / Claude Desktop / 其他 MCP host
- 需要类型安全的工具调用（避免字符串解析）
- 需要协议级 cancellation 与 progress

### 7.2 协议基础
- MCP 协议 2025-11-25（JSON-RPC 2.0 over stdio / Streamable HTTP）
- 必须实现 `initialize`、`tools/list`、`tools/call`
- 每个工具的 `_meta.operation_id` 指向 Contract Manifest

### 7.3 工具响应：structuredContent + outputSchema（v3.0 关键修订）
v2.0 把 JSON envelope 序列化进 `content[0].text` 是旧 pattern。当前 MCP 规范支持 `structuredContent` 和工具声明 `outputSchema`，必须优先使用：

```json
{
  "content": [
    { "type": "text", "text": "Created user user_123." }
  ],
  "structuredContent": {
    "schema_version": "1.0",
    "status": "ok",
    "operation_id": "users.create",
    "data": { "id": "user_123" },
    "meta": { "...": "..." }
  },
  "isError": false
}
```

- `content[].text`：人类 / 模型可读摘要
- `structuredContent`：AI / 程序解析主入口（必须符合工具声明的 `outputSchema`）
- `isError`：业务级成功 / 失败标记

### 7.4 错误分类（v3.0 关键修订）
| 错误类型 | MCP 返回方式 |
|---|---|
| JSON-RPC 格式错误 | JSON-RPC `error` |
| method 不存在 | JSON-RPC `error` |
| tool name 不存在 | JSON-RPC `error` |
| `arguments` 不是 object | JSON-RPC `error` |
| **字段值不合法（如 email 格式错）** | **`isError: true` + structured error** |
| **认证失败、资源不存在、业务冲突** | **`isError: true` + structured error** |
| Server crash | JSON-RPC `error` 或 transport-level failure |

核心原则：**模型可通过修正参数自我重试的错误 → `isError: true`**；协议级、不可恢复错误 → JSON-RPC `error`。

### 7.5 Tool Annotations（强制）
从 Contract Manifest 的 `side_effects` 派生：

| Annotation | 含义 |
|---|---|
| `readOnlyHint` | 只读操作 |
| `destructiveHint` | 破坏性操作 |
| `idempotentHint` | 多次调用结果一致 |
| `openWorldHint` | 与外部世界交互（不可纯粹回滚） |

**注**：MCP 官方文档明确这些是 hint，不是安全保证。真正的安全防线必须在 host / policy 层。

### 7.6 部署形态
| 形态 | 适用场景 |
|---|---|
| stdio | 本地工具，Claude Code/Desktop 直接调用 |
| Streamable HTTP | 远程服务，需鉴权 |

---

## 8. HTTP Adapter（可选）

仅在需跨网络 / 跨语言调用时实现：
- 必须从 Contract Manifest 生成或校验
- 必须暴露 OpenAPI 3.x spec，每个 path 标注 `x-operation-id`
- 鉴权走 `Authorization` header，**禁止**通过 URL query 传递 secret
- 错误响应使用与 CLI / MCP 相同的错误 envelope

---

## 9. 安全要求（v3.0 强化）

| 维度 | 要求 |
|---|---|
| 敏感参数 | 禁走 cmdline argv（`ps` 可见），用 env / stdin / config |
| 敏感字段标注 | `inputSchema` 中 `_meta.sensitive: true`（非标准 JSON Schema，仅作 host 脱敏 hint） |
| 配置文件权限 | 启动时检查 mode ≤ `0600`，否则警告或拒绝 |
| 日志脱敏 | stderr 写入前对 `token` / `password` / `secret` / `api_key` 字段掩码；**禁止**出现在 `structuredContent` 或 `dry_run_plan` |
| Workspace sandbox | 文件读写默认限制在 workspace 根目录与显式声明的 additional paths |
| Shell 执行 | **禁止**字符串拼接 shell；必须 argv array 形式（`subprocess.run([...], shell=False)`） |
| Timeout | 每个 operation 必须可配置 timeout，默认值 ≤ 5 分钟 |
| Cancellation | CLI 响应 SIGINT/SIGTERM 优雅清理（返回 128+N）；MCP 支持协议级 cancellation |
| Audit | 所有写操作记录 `request_id`、actor、`operation_id`、diff 或 `dry_run_plan` |
| 破坏性操作 | 默认 require confirmation，除非 `--yes` 或 policy 明确允许 |

---

## 10. 自描述能力

### 10.1 CLI（必须）
| 命令 | 输出 |
|---|---|
| `app --help` | 人类可读帮助 |
| `app manifest` | **Contract Manifest JSON**（v3.0 新增 canonical 来源） |
| `app schema` 或 `app --help-json` | CLI 结构 JSON Schema |
| `app completion <bash\|zsh\|fish>` | Shell 补全 |
| `app version --output json` | 版本元信息 |

### 10.2 MCP（若启用，协议内置）
- `initialize` → server 能力声明
- `tools/list` → 所有工具及其 `inputSchema`、`outputSchema`、annotations、`_meta.operation_id`

---

## 11. Skill Package（可选，Claude Code 专用）

### 11.1 定位：Agent Experience Layer
Skill **不是**第三套 API。它是 Claude Code 的"**操作手册 + 路由策略 + 安全策略 + workflow 封装**"。

### 11.2 强制约束
- **MUST NOT** 重复声明业务 schema（必须引用 Contract Manifest）
- **MUST NOT** 解析人类可读 text 输出（只解析 `structuredContent` 或 stdout JSON）
- **MUST NOT** 把 secret 放进 prompt / logs / structured output
- **MUST** 声明 transport 策略（mcp-first / cli-first / cli-only / mcp-only）
- **MUST** 声明失败处理（按 `error.code` 与 `exit_code` 判断，**不得**猜测自然语言报错）
- **MUST** 写操作先 dry-run + 摘要确认（除非 manifest 标记 `idempotent` 且用户已显式授权）

### 11.3 推荐目录结构
```
.claude/skills/<app-name>/
├── SKILL.md
├── reference/
│   ├── contract-manifest.json   # 从 app manifest 同步
│   ├── operations.md
│   ├── errors.md
│   └── examples.md
└── scripts/
    └── call_cli.py              # CLI fallback wrapper
```

### 11.4 一个 App 一个主 Skill
不要为每个功能生成 Skill（会污染 skill 列表、上下文膨胀、维护困难）。模式：
- **一个主 Skill**：覆盖整个 App
- **少量 workflow Skill**：仅为高频复合操作（如 `/appctl-deploy`、`/appctl-diagnose`）

### 11.5 三类 Skill
| 类型 | 用途 | 自动触发 |
|---|---|---|
| Reference Skill | 解释 App 接口、约定、错误处理 | 允许 |
| Workflow Skill | 多步流程（部署、导出、批处理） | 视风险决定 |
| Destructive Skill | 删除、发布、计费、生产变更 | **必须** `disable-model-invocation: true`，仅用户显式触发 |

### 11.6 SKILL.md 路由模板（核心片段）
```markdown
## Transport policy
1. 若 MCP server 连接可用，优先使用 MCP tool
2. 否则使用 CLI fallback：`app <cmd> --output json --no-color --no-input`
3. 复杂输入走 stdin JSON
4. 只解析 stdout JSON（CLI）或 structuredContent（MCP）

## Error policy
1. 检查 MCP `isError` 或 CLI `exit_code`
2. 解析 `error.code` 与 `error.retryable`
3. 仅在 `retryable: true` 时重试
4. 破坏性操作失败后必须重新 dry-run，禁止直接重试
```

### 11.7 Skill 工具命令（可选实现）
推荐 App 提供：
- `app skill generate --target claude-code --output .claude/skills/<app>`：从 Contract Manifest 生成 Skill
- `app skill verify <path>`：校验 Skill 引用的 `contract_version` 与当前一致

---

## 12. Claude Code Implementation Protocol（用 Claude Code 开发本 App 时遵守）

### Step 1 — 设计响应（编码前）
输出 5 份产物供用户确认：
1. **适配器决策**：CLI（必须）+ 启用哪些可选适配器？依据是什么？
2. **Contract Manifest 草稿**：operations 列表与 schema 摘要
3. **Command Tree**：CLI 子命令结构
4. **Exit Code Table**：自定义码语义
5. **MCP Tool List**（若启用 MCP）：tool 名与 `operation_id` 映射

### Step 2 — 实现顺序
1. Core SDK + Schema 集中定义 + Port 接口
2. Contract Manifest 模块（CLI / MCP / HTTP 都从此读取）
3. **CLI Adapter（强制实现）**
4. MCP Adapter（若启用）
5. HTTP Adapter（若启用）
6. CLI 自描述命令（`manifest`、`schema`、`completion`）
7. Skill Package（若启用）
8. 集成测试

### Step 3 — Conformance Tests（每完成一个 operation 强制执行）

**CLI**：
```bash
# stdout 纯净 JSON
./app <cmd> <args> --output json 2>/dev/null | jq .

# stderr 隔离
./app <cmd> <args> --output json 1>/dev/null   # stderr 应有日志

# 错误路径：exit=2 且 stdout 仍为合法 JSON
./app <cmd> --invalid-flag --output json; echo "exit=$?"

# dry-run 可观
./app <cmd> <args> --dry-run --output json | jq '.data.dry_run_plan'

# 人类管道行为正常（v3.0 修订验证）
./app users list | head    # 应为 text，不应被自动 JSON 化
```

**MCP（若启用）**：
```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize",...}' | ./mcp-server
echo '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | ./mcp-server \
  | jq '.result.tools[]._meta.operation_id'
# 业务错误必须为 isError:true 而非 JSON-RPC error
```

**Manifest 一致性**：
```bash
./app manifest --output json | jq -r '.operations[].operation_id' | sort > /tmp/ops.txt
# CLI 子命令 / MCP tools / HTTP path 提取的 operation_id 集合必须 == /tmp/ops.txt
```

### Step 4 — 文档化
项目根目录必须包含：
- `README.md`
- `docs/contract-manifest.json`：当前 manifest 快照
- `docs/exit-codes.md`
- `docs/schema-versions.md`
- `CHANGELOG.md`：标注 `contract_version` 变化
