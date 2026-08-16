# 需求引擎 · 后端实现计划

| 字段 | 内容 |
|------|------|
| 版本 | v0.2 |
| 日期 | 2026-08-06 |
| 前置 | PRD v0.2.1 + Windows/macOS/iOS 前端 Mock 已定调 |
| 目标 | 把「无推理状态引擎」做成可运行的后端，供前端与 MCP 客户端使用 |
| 已拍板 | **Rust** · **先接 Windows UI** · MCP 分角色 · 禁止自由改 status |

---

## 1. 目标与边界

### 1.1 要做成什么

一个 **本地优先** 的服务进程：

- 持久化 **项目（Project）** 与 **需求（Requirement）**
- 提供 **HTTP API**（给 Web/桌面壳 UI）
- 提供 **MCP Server**（给讨论侧 / 实现侧 Coding Agent）
- **零 LLM**：不做推理、不拆任务、不调度 Worker 池

### 1.2 明确不做（v1）

| 不做 | 原因 |
|------|------|
| 调用任何模型 API | 产品定位 |
| 管理 subagent / IDLE / worktree | 实现侧黑盒 |
| Session 互聊 / TUI 热同步 | 非引擎职责 |
| 自动 merge 代码 | 后续可配，非 MVP |
| 多租户云 SaaS | MVP 本地单用户 |

### 1.3 成功标准（MVP 可演示）

1. UI 能通过 API 列出项目、切换项目、看四栏任务  
2. MCP：`create_requirement` / `list_ready_tasks` / `claim_task` / `submit_for_review` / `complete_review`（**无自由 update_status**）  
3. 两个进程同时 `claim` 同一任务，只有一个成功（原子性）  
4. 重启进程后数据不丢  
5. 讨论侧 token **不能** claim / merge  

---

## 2. 推荐技术选型

| 层 | 推荐 | 备选 | 理由 |
|----|------|------|------|
| 语言 | **Rust（已拍板）** | — | 单二进制、事务扎实 |
| HTTP | **axum** | actix-web | 生态成熟 |
| 存储 | **SQLite**（`better-sqlite3` / `libsql`） | Postgres 后期 | 本地单文件、零运维 |
| 迁移 | **Drizzle** 或 **Prisma** | raw SQL | 可回滚 schema |
| MCP | **@modelcontextprotocol/sdk** | 自研 JSON-RPC | 标准 |
| 实时 | **SSE** 或 WebSocket（可选） | 短轮询 2–5s | UI 已按轮询设计，可后补 |
| 鉴权 | 本地 **API Token**（角色） | 无鉴权仅本机 loopback | Planner / Foreman / Admin |
| 打包 | `pkg` / 双进程脚本 | Tauri 壳嵌 UI | 先 `npm start` |

**默认栈（已拍板）：**

```text
Rust + axum + sqlx(SQLite) + MCP(stdio) + Windows UI
```

---

## 3. 总体架构

```text
┌─────────────┐   HTTP/JSON    ┌──────────────────────────┐
│ Web UI      │ ─────────────► │  需求引擎 Server          │
│ (静态/壳)   │ ◄──── SSE ──── │                          │
└─────────────┘                │  ┌────────┐  ┌────────┐  │
                               │  │ HTTP   │  │ MCP    │  │
┌─────────────┐   MCP stdio    │  │ Router │  │ Server │  │
│ 讨论侧 Agent│ ─────────────► │  └───┬────┘  └───┬────┘  │
│ (Codex 等)  │                │      │           │       │
└─────────────┘                │      ▼           ▼       │
                               │  ┌────────────────────┐  │
┌─────────────┐   MCP stdio    │  │ Application Service│  │
│ 实现侧 Agent│ ─────────────► │  │ (无 LLM)           │  │
│ (Grok 等)   │                │  └─────────┬──────────┘  │
└─────────────┘                │            ▼             │
                               │  ┌────────────────────┐  │
                               │  │ SQLite  repositories│  │
                               │  └────────────────────┘  │
                               └──────────────────────────┘
```

**进程模型（MVP）：**

- 单进程同时挂：`HTTP :7420` + 可选 `MCP stdio`（或 MCP 由 `npx req-engine-mcp` 子命令连同一 DB）  
- 更干净：`req-engine serve`（HTTP）+ `req-engine mcp`（stdio，共享 SQLite 文件，靠 SQLite 锁）

---

## 4. 数据模型（DB Schema）

### 4.1 `projects`

| 列 | 类型 | 说明 |
|----|------|------|
| id | TEXT PK | slug，如 `demo-shop` |
| name | TEXT | 显示名 |
| color | TEXT | UI 色点 |
| blurb | TEXT | 一句话 |
| created_at | TEXT ISO | |
| updated_at | TEXT ISO | |

### 4.2 `requirements`

| 列 | 类型 | 说明 |
|----|------|------|
| id | TEXT PK | 如 `REQ-018` |
| project_id | TEXT FK | |
| title | TEXT NOT NULL | |
| description | TEXT | |
| priority | TEXT | P0–P3 |
| status | TEXT | 见状态机 |
| scope_json | TEXT | JSON array |
| non_scope_json | TEXT | |
| acceptance_json | TEXT | |
| dependencies_json | TEXT | id[] 声明依赖 |
| claimed_by | TEXT NULL | 实现侧实例名 |
| progress_summary | TEXT NULL | 上报 |
| blocked_reason | TEXT NULL | |
| external_run_id | TEXT NULL | |
| created_by | TEXT | `planner` / `human` / token 名 |
| created_at / updated_at | TEXT | |

索引：

- `(project_id, status)`  
- `(project_id, updated_at DESC)`  
- 可选 unique：进行中 claim 业务靠事务而非额外表  

### 4.3 `events`（审计 / 时间线）

| 列 | 类型 |
|----|------|
| id | INTEGER PK |
| project_id | TEXT |
| requirement_id | TEXT NULL |
| actor | TEXT |
| kind | TEXT | `created` / `claimed` / `status` / `progress` … |
| message | TEXT |
| payload_json | TEXT |
| created_at | TEXT |

### 4.4 `api_tokens`

| 列 | 类型 |
|----|------|
| token_hash | TEXT PK |
| role | TEXT | `planner` \| `foreman` \| `admin` |
| name | TEXT | 展示用 |
| created_at | TEXT |

### 4.5 状态机（引擎强制）

```text
todo → scheduled → in_progress → review → done
                 ↘ blocked / failed
任意非终态 → cancelled（admin/人）
```

合法迁移表写在代码常量里；非法迁移返回 `409`.

**声明依赖（可选 MVP 规则）：**  
`list_ready` 仅返回：`status=todo` 且 `dependencies` 中每张卡 `status=done`（或依赖为空）。

---

## 5. API 设计

### 5.1 HTTP（UI）

Base：`http://127.0.0.1:7420/v1`  
Header：`Authorization: Bearer <token>`（admin 可读写 UI）

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/health` | 健康检查 |
| GET | `/projects` | 项目列表 |
| POST | `/projects` | 创建项目 |
| GET | `/projects/:id/requirements` | 任务列表（可 query status） |
| POST | `/projects/:id/requirements` | 创建需求 |
| GET | `/requirements/:id` | 详情 + 时间线 |
| PATCH | `/requirements/:id` | 更新字段（限状态规则） |
| POST | `/requirements/:id/claim` | 原子领取 |
| POST | `/requirements/:id/progress` | 上报进度 |
| GET | `/projects/:id/events` | 审计流 |
| GET | `/events/stream` | SSE（可选） |

### 5.2 MCP Tools（按角色）

**Planner（讨论侧）**

- `create_requirement`  
- `update_requirement`（仅 todo 等允许字段）  
- `list_requirements`  
- `get_requirement`  

**Foreman（实现侧）**

- `list_ready_tasks`  
- `get_requirement`  
- `claim_task`  
- `update_status`  
- `report_progress`  
- `submit_result`（→ review 或 done，策略配置）  

**禁止 Planner：** claim / update_status 到 in_progress/done  

MCP 启动示例：

```bash
req-engine mcp --role planner --token "$PLANNER_TOKEN"
req-engine mcp --role foreman --token "$FOREMAN_TOKEN"
```

---

## 6. 关键业务逻辑

### 6.1 创建需求

1. 校验 Zod schema（title 非空、priority 枚举）  
2. 生成 id（项目前缀 + 序号，或 ULID）  
3. status=`todo`  
4. 写 `events`  
5. 可选：广播 SSE `requirement.created`  

### 6.2 Claim（原子）

```sql
BEGIN IMMEDIATE;
UPDATE requirements
SET status='in_progress', claimed_by=?, updated_at=?
WHERE id=? AND status IN ('todo','scheduled') AND claimed_by IS NULL;
-- changes==1 成功 else 409
INSERT INTO events ...;
COMMIT;
```

### 6.3 状态迁移

- 查表 `ALLOWED[from][to]`  
- 校验 token role  
- blocked 时必须 `blocked_reason`  
- 回写 `updated_at` + event  

### 6.4 事件（不做 chat 注入）

引擎只写 DB + SSE。  
实现侧 **自己 poll** `list_ready_tasks` 或订 SSE；**从不**给 Agent 对话窗口塞消息。

---

## 7. 仓库与目录结构（建议）

```text
req-engine/
  package.json
  README.md
  drizzle.config.ts
  src/
    index.ts              # CLI: serve | mcp | init
    config.ts
    db/
      schema.ts
      migrate.ts
      client.ts
    domain/
      states.ts           # 状态机
      ids.ts
    services/
      projects.ts
      requirements.ts
      claims.ts
      events.ts
    http/
      server.ts
      routes/*.ts
      auth.ts
    mcp/
      server.ts
      tools-planner.ts
      tools-foreman.ts
    seed/
      demo.ts             # 对齐前端 Mock 数据
  web/                    # 可选：拷贝/构建现有 HTML 或后续正式前端
  data/
    req-engine.sqlite     # gitignore
```

---

## 8. 实施阶段（建议节奏）

### Phase 0 — 对齐（0.5 天）

- [ ] 冻结 v1 API 的 OpenAPI 草稿（可从本计划摘）  
- [ ] 确认默认端口 `7420`、数据目录 `~/.req-engine/`  
- [ ] 确认 id 规则（`REQ-###` vs ULID）  

### Phase 1 — 骨架 + DB（1–2 天）

- [ ] 初始化 TS 项目、Drizzle schema、migrate  
- [ ] `req-engine init` 建库 + 生成 planner/foreman/admin token  
- [ ] seed 四套 demo 项目（与前端一致，便于联调）  
- [ ] 单元测试：状态机、claim 并发  

### Phase 2 — HTTP API（1–2 天）

- [ ] CRUD projects / requirements  
- [ ] claim / progress / status  
- [ ] 列表筛选与统计字段  
- [ ] 本地 CORS 仅 127.0.0.1  
- [ ] 用 curl / Bruno 跑通happy path  

### Phase 3 — MCP（1–2 天）

- [ ] Planner / Foreman 工具集  
- [ ] 角色鉴权与错误信息友好  
- [ ] 文档：Claude/Codex/Grok 如何配 MCP  
- [ ] 手工：用一个 Agent 写卡、另一个 Agent claim  

### Phase 4 — 接前端（1–2 天）

- [ ] 将 Windows Mock 改为 `fetch('http://127.0.0.1:7420/v1/...')`  
- [ ] 项目切换、四栏、详情、新建走真实 API  
- [ ] 轮询 3s 或 SSE  
- [ ] 加载/错误态  

### Phase 5 — 硬化（1 天+）

- [ ] 备份：`req-engine backup`  
- [ ] 日志与简单 metrics  
- [ ] Windows/macOS 一键启动脚本  
- [ ] （可选）Tauri/WinUI 壳加载 UI  

**合计 MVP：约 5–9 个有效开发日**（单人、含联调）。

---

## 9. 测试计划

| 类型 | 内容 |
|------|------|
| 单元 | 状态机、依赖 ready 过滤、id 生成 |
| 并发 | 10 线程同时 claim 同一 id → 恰好 1 成功 |
| 鉴权 | planner 调 claim → 403 |
| 集成 | seed → HTTP 列表 → MCP create → MCP claim → UI 可见 |
| 回归 | 重启后数据与 token 仍在 |

---

## 10. 配置与部署

```bash
# 初始化
req-engine init
# 输出：admin/planner/foreman tokens 存入 ~/.req-engine/tokens.txt（仅一次）

# 启动 HTTP
req-engine serve --host 127.0.0.1 --port 7420

# MCP（在 Agent 配置里）
req-engine mcp --role planner
```

环境变量：

- `REQ_ENGINE_HOME`（默认 `~/.req-engine`）  
- `REQ_ENGINE_DB`  
- `REQ_ENGINE_TOKEN`（MCP 子进程）  

---

## 11. 风险与缓解

| 风险 | 缓解 |
|------|------|
| SQLite 多进程写锁 | `BEGIN IMMEDIATE`；MCP 与 HTTP 同机短事务 |
| Agent 乱改 status | 角色 token + 迁移表 |
| 前端 Mock 与 API 字段不一致 | 共用 Zod schema / OpenAPI 生成类型 |
| 用户以为引擎会调度 Worker | README 大字边界；API 不提供 worker 接口 |
| 端口占用 | 可配置；启动失败明确报错 |

---

## 12. 建议的第一周排期（可执行）

| 日 | 产出 |
|----|------|
| D1 | 仓库骨架、schema、migrate、seed、init CLI |
| D2 | HTTP：projects + requirements CRUD + 状态迁移 |
| D3 | claim 原子性 + 测试 + events 时间线 |
| D4 | MCP planner + foreman 最小工具集 |
| D5 | 接 Windows UI 真数据；演示「写卡 → claim → 看板变」 |

---

## 13. 你现在可以拍板的 5 个问题

1. **语言：** ~~TS vs Rust~~ → **Rust**  
2. **ID 规则：** 可读 `REQ-018` 还是全局 ULID？  
3. **list_ready 是否强制依赖已完成？**（建议 MVP 开启）  
4. ~~UI 接哪端~~ → **仅 Windows UI**  
5. **MCP 与 HTTP 单进程还是双命令？**（建议双命令共享 SQLite）  

---

## 14. 下一步行动（我建议的默认）

已拍板默认：

1. **Rust + SQLite + MCP + 先接 Windows UI**  
2. **Phase 1 脚手架 + seed**  
3. **Windows UI 改为打本地 API**  
4. 再补 MCP 给真实 Agent 联调  

---

*本文档路径：`C:\Users\yyy\Desktop\需求引擎\后端实现计划_Backend-Plan.md`*  
*前端锁定：Windows 仅顶栏 Tabs 切换项目（见 `req-engine/web/index.html` / `demo/需求引擎_UI_Windows_Fluent.html`）*

---

## 15. 产品拍板：MCP 暴露什么、状态怎么走（v0.2）

> 引擎**无智能**，不知道代码写没写完。  
> **状态迁移 = 调用「意图明确」的 MCP/HTTP 动词**，由引擎校验合法性并落库。  
> **禁止**通用 `update_status`、禁止开发侧硬删进行中的需求。

### 15.1 核心原则

1. **领取即进行中**：`claim_task` 原子地把卡片从可领状态打成 `in_progress`，并写入 `claimed_by`。  
2. **动词化迁移，不暴露自由改状态**：只提供 claim / submit_for_review / complete_review / cancel / release。  
3. **引擎不猜测完工**：进入「审核中」「已完成」必须由客户端**显式调用**对应工具（或人在 UI 点按钮）。  
4. **分角色 Token**：讨论侧 Planner 与实现侧 Foreman 工具集不同。  
5. **删除是高危操作**：MVP 以 **cancel（软取消）** 为主；硬删除仅 Admin，且默认不对 Agent 开放。

### 15.2 状态机（引擎强制）

```text
创建 create_requirement
              │
              ▼
           [todo]  ····················· 看板「需求」
              │
              │ claim_task          （实现侧）
              ▼
      [in_progress]  ················· 看板「进行中」
         │         │
         │         │ release_task（可选，交回队列）
         │         └──► [todo]
         │
         │ submit_for_review   （实现侧，须为 claimant）
         ▼
        [review]  ···················· 看板「审核中」
         │         │
         │         │ complete_review(fail) → [todo] 打回
         │         │
         │ complete_review(pass) 或 UI 人审通过
         ▼
        [done]  ······················ 看板「已完成」

旁路：cancel → [cancelled]（主要限 todo；进行中仅 Admin）
旁路：blocked 可由 report_progress + set_blocked 进入（可选工具）
```

### 15.3 角色 × 工具矩阵

| 工具 | Planner 讨论侧 | Foreman 实现侧 | Admin / Windows UI |
|------|----------------|----------------|--------------------|
| `list_projects` | ✅ | ✅ | ✅ |
| `create_requirement` → todo | ✅ | ❌ | ✅ |
| `update_requirement`（仅 todo 内容） | ✅ | ❌ | ✅ |
| `list_requirements` / `get_requirement` | ✅ | ✅ | ✅ |
| `cancel_requirement` | ✅ 仅 todo | ❌ | ✅ 更宽 |
| **硬删除** | ❌ | ❌ | ✅ 可选 |
| `list_ready_tasks` | 只读可给 | ✅ | ✅ |
| **`claim_task` → in_progress** | ❌ | ✅ | ✅ |
| `report_progress`（不改状态） | ❌ | ✅ | ✅ |
| **`submit_for_review` → review** | ❌ | ✅ 仅自己 claim | ✅ |
| **`complete_review` → done / 打回 todo** | ❌ | 可配置关闭 | ✅ 默认人审 |
| `release_task` → 回到 todo | ❌ | ✅ 仅自己 claim | ✅ |
| 自由 `set_status` | ❌ | ❌ | ❌ |

### 15.4 领取 / 审核 / 完成 —— 产品问答

| 问题 | 设计 |
|------|------|
| 「领任务」怎么做？ | 只暴露 **`claim_task(requirement_id)`**。成功则状态**自动**变为 `in_progress`，记录 `claimed_by`。 |
| 能不能让开发侧自己删进行中？ | **不能硬删**。最多 `release_task`（交回 todo）或 Admin `cancel`。 |
| 进行中如何到审核中？ | 开发侧做完后调 **`submit_for_review`**。引擎校验：当前是 in_progress 且 claimant 匹配。 |
| 审核中如何到已完成？ | **默认人在 Windows UI 点「通过」** → `complete_review(pass)`。可选配置允许 Foreman 机审。 |
| 引擎不知道是否真做完？ | **正确**。引擎只保证状态机与权限；真实性靠 AC、审核、时间线。 |
| 为何不暴露 `update_status`？ | 模型会跳步 todo→done、抢改他人任务；动词化更安全。 |

### 15.5 Planner / Foreman 最小 MCP 集（MVP）

**Planner（讨论侧 skill 只教这些）：**

1. `create_requirement`  
2. `list_requirements` / `get_requirement`  
3. `update_requirement`（todo）  
4. `cancel_requirement`（todo）  

**Foreman（实现侧 skill）：**

1. `list_ready_tasks`  
2. `claim_task`  ← 领取并进入进行中  
3. `report_progress`  ← 可选，状态仍为进行中  
4. `submit_for_review`  ← 进入审核中  
5. `release_task`  ← 可选交回  

**不要**在 skill 里写「把 status 改成 done」。

### 15.6 Skill 示例文案（实现侧）

```text
你是实现侧包工头，通过 MCP 使用需求引擎：
1. list_ready_tasks 查看可领任务
2. claim_task 领取（自动变为进行中）——不要领取已属于别人的卡
3. 按 acceptance_criteria 完成工作，可用 report_progress 汇报
4. 完成后必须 submit_for_review，等待审核；不要声称已删除或直接完成
5. 无法继续时用 release_task 交回，并写明 reason
6. 禁止删除需求；禁止修改他人进行中的任务
```

### 15.7 Windows UI 对应

| UI | API |
|----|-----|
| 顶栏 Tabs 切换项目 | GET /projects |
| 四栏看板 | GET /projects/:id/requirements |
| 需求列悬停新建 | POST requirement（人/admin token） |
| 详情 | GET requirement + events |
| 人点「确认完成 / 通过审核」 | POST complete_review |

Agent 不经过 UI，只走 MCP。

### 15.8 Rust 里程碑（先接 Windows）

| 顺序 | 产出 |
|------|------|
| 1 | `req-engine init` + SQLite schema + 状态机 + claim 事务 |
| 2 | `req-engine serve` HTTP，seed 四项目 |
| 3 | Windows UI 改为请求 `127.0.0.1:7420` |
| 4 | `req-engine mcp --role planner|foreman` |
| 5 | 并发 claim 测试 + README/skill |

```bash
req-engine init
req-engine serve --port 7420
req-engine mcp --role planner
req-engine mcp --role foreman
```

