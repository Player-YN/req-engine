# 产品/后端完成度与概念说明（2026-08）

## 后端完成度（摘要）

| 能力 | 状态 |
|------|------|
| 状态机动词（claim / submit / complete_review / cancel / release…） | ✅ |
| HTTP `/v1` + Bearer 角色 | ✅ |
| MCP stdio planner / foreman | ✅ |
| SQLite 持久化 + migrations | ✅ |
| 项目 CRUD + `local_path` + **软归档** | ✅（归档 `POST .../archive`） |
| 默认不 seed demo 项目 | ✅ |
| **运行时**（按项目 cwd 起 agent / 沙箱） | ❌ 未做 — `local_path` 仅元数据 |
| **讨论册**（会话/笔记流） | ❌ 未做 |
| 文件系统级项目隔离 | ❌ 未做 — 仅 DB 按 `project_id` 隔离 |
| 安装包 / 单 exe | ⏸ 延期 |

---

## 1. 绑定本机文件夹意味着什么？能提供运行时吗？

**现在（已实现）**

- `local_path` 是项目的**可选元数据**：存在 SQLite，API/UI 可读写。
- 含义：**「这个需求板对应磁盘上哪个工作区」的声明**，方便人看见路径、以后给 Agent 当默认根目录。
- **不**会：自动 `cd`、起进程、挂载沙箱、同步 git、监听文件。

**「运行时」若指：**

| 期望 | 现状 | 后续可做 |
|------|------|----------|
| Agent 默认在该目录改代码 | 未接 | MCP 工具参数 `cwd=local_path`；CLI wrapper 设工作目录 |
| 引擎在该目录跑命令 | 不做（零 LLM/零执行） | 外置 runner / foreman CLI |
| 路径隔离多项目文件 | 未做 | 策略：agent 只能访问绑定路径下文件 |

**结论：** 绑定路径 = **工作区契约的第一期（标签）**，不是运行时容器。要「提供运行时」需要另做 agent launcher + 可选沙箱，不在当前零 LLM 引擎内核里。

---

## 2. Agent / CLI：讨论册？怎么加需求？

**没有「讨论册」实体。** 产品当前只有：

- **项目** → **需求卡片**（状态机）→ **事件时间线**（动词日志）

讨论/设计若要落板，应变成 **planner 创建的 requirement**（或以后加 notes 表）。

**实现侧 / 规划侧怎么加：**

```text
# Discuss seat (stdio) — copy pack from desktop, then:
req-engine mcp --pair disc_… --home <data>
# tools: list/get/create/update/cancel requirement (todo only)

# Build seat
req-engine mcp --pair build_… --home <data>
# tools: list_ready_tasks, claim, progress, submit_for_review, release
```

HTTP 等价：`POST /v1/projects/{id}/requirements`（admin/planner）。

产品接入：桌面复制 **匹配码 + 接入 Prompt**，宿主 `mcp --pair disc_|build_`。`docs/MCP.md`。

**座位显示：** `--pair` 进程握手后写 `seat_presence` 心跳；UI 徽章「在座」。`clientInfo.name` 能映射则显示已知宿主色标，否则色块字标。不是推送、不是 ack。

**缺口：** UI 内无「讨论串」。引擎不会向实现侧推送新 todo。

---

## 3. 项目隔离：如何确定性互不影响？

**已有（数据层）**

- 所有需求带 `project_id` FK；列表/动词均按项目过滤。
- 归档项目从默认列表消失，避免误操作。
- Token 是**角色**隔离，不是项目级 ACL（任一 admin 可见全部项目）。

**没有（强隔离）**

- 无 per-project token / 无 per-project DB 文件。
- `local_path` 不阻止 agent 读写别的盘符路径。
- 多 agent 同 home 同库 — 靠 `project_id` + 流程约定。

**确定性隔离建议（产品）**

1. **逻辑隔离（现状）：** 卡片与事件始终带 `project_id`；UI/MCP 必须带项目上下文。  
2. **路径约定（下一期）：** foreman 工具强制 `cwd = project.local_path`。  
3. **硬隔离（可选）：** 每项目独立 `REQ_ENGINE_HOME` / 独立 sqlite（运维级）。

---

## 4. 取消项目 / 弹窗 / 一键取消待办

| 项 | 行为 |
|----|------|
| 取消项目 | Tab 上 ✕ → 二次确认 → `POST /v1/projects/{id}/archive`（软归档） |
| 取消单条需求 | 抽屉「取消需求」→ **应用内居中确认框**（不再用 OS `confirm` 顶栏） |
| 需求列一键取消 | 「取消全部待办」→ **两次**应用内确认 → 逐条 soft cancel |

---

## 每项目两个职位（产品命名）

| 职位 | 中文名 | MCP role（协议） | 干什么 |
|------|--------|------------------|--------|
| 讨论侧 | **讨论 Agent** | `planner` | 讨论、拆解、创建/改需求 |
| 实现侧 | **实现 Agent** | `foreman` | 领取、实现、交审、释放 |

配置：桌面 UI **一键复制接入 Prompt** → 粘贴给用户自己的 coding agent → 由 agent 写 MCP 配置并 `POST .../agents/discuss|build/ack` → UI 显示 **已配置**。

## 人机分工（UI 约定）

| 动作 | 人（桌面 UI） | Agent（MCP / HTTP） |
|------|----------------|---------------------|
| 建项目 / 建需求 | ✅ | 讨论 Agent ✅ |
| **领取 claim** | ❌ 不暴露 | 实现 Agent ✅ |
| **交审 submit** | ❌ 不暴露 | 实现 Agent ✅ |
| **释放 release** | ❌ 不暴露 | 实现 Agent ✅ |
| **确认完成** | ✅ review | admin HTTP only（MCP 无 complete_review） |
| **驳回** | ✅ 须填原因 | admin HTTP |
| 取消需求 / 归档项目 | ✅ | 讨论 Agent 可 cancel todo |

引擎 HTTP 仍允许 admin 调 claim 等（调试/脚本）；**产品 UI 不提供这些按钮**。

## 推荐下一步（非本文件承诺）

1. MCP `create_requirement` 展示/校验 `local_path`；文档写清 `project_id` 必填。  
2. 可选 notes/discussion 表（真·讨论册）。  
3. 项目级 API key 或 per-home 多实例隔离。  
4. Agent launcher：读 `local_path` 设 cwd。
