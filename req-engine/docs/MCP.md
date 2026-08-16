# MCP — 给编码 Agent 的接口

引擎是 **零 LLM** 状态机。MCP 只是把**动词**暴露给外部智能体（Cursor / Claude / Codex / Kimi / Grok Build 等），引擎内部不调用模型。

## 我们是 Server 还是 Client？

| 角色 | 是谁 |
|------|------|
| **MCP Server** | `req-engine.exe mcp …`（本仓库已实现） |
| **MCP Client** | 你的 coding agent 宿主（Cursor / Claude Desktop / Codex CLI / 支持 MCP 的 Grok/Kimi 等） |

桌面 UI **不是** MCP Client，也 **不是** MCP Server 配置面板。  
接 Agent = 在 **Agent 宿主** 里登记我们的 stdio 命令，不是在需求引擎窗口里点「连接 AI」。

## 产品路径：每项目两枚匹配码

桌面打开项目 → **复制讨论侧接入 / 复制实现侧接入**。剪贴板是 **匹配码 + 接入 Prompt**（MCP 接法、引擎简介、角色边界）。贴给对应 Coding Agent Session。

库里只存 SHA-256；明文在 `{home}/pair-codes.json`（勿提交）。作废重发后旧码立刻失效。

```powershell
# 讨论侧（只能读本项目、加/改/取消 todo）
req-engine.exe mcp --pair disc_… --home <home>

# 实现侧（只能领本项目活、交审）
req-engine.exe mcp --pair build_… --home <home>
```

- **stdout** = MCP JSON-RPC 线
- **stderr** = 日志（不要把日志打到 stdout）
- 全局 `tokens.txt` 的 planner/foreman 只用于调试 HTTP，**不算**项目已接入。
- 握手里的 `clientInfo.name` 只用来画座位脸（已知宿主或色块），不作鉴权。

`--role` + `--token` 仍可作为调试后门，文档和桌面不再推荐。

## 角色与工具

| 座位 | 典型使用者 | 工具（摘要） | 不能做 |
|------|------------|--------------|--------|
| **讨论** (`disc_`) | 写需求的 agent | `list_requirements`, `get_requirement`, `create_requirement`, `update_requirement`（todo）, `cancel_requirement`（todo） | claim / 交审 / 改进行中的卡 / 下场写代码 |
| **实现** (`build_`) | 干活的 agent | `list_ready_tasks`, `claim_task`, `report_progress`, `submit_for_review`, `release_task` | **complete_review** / 建需求 |
| **admin** | 人 / 桌面 UI | HTTP 全量 + 审核完成 + 看码/轮换 | — |

**审核完成（pass→done / fail→todo）只给人**：桌面「确认完成 / 驳回」或 admin HTTP，**不在 foreman MCP**。

## 宿主配置示例

见仓库：

- [`examples/mcp.planner.example.json`](../examples/mcp.planner.example.json)
- [`examples/mcp.foreman.example.json`](../examples/mcp.foreman.example.json)

把 `command` 改成你本机 `req-engine.exe` 的绝对路径，`--pair` 换成桌面复制出来的码。

### Cursor / 兼容 `mcpServers` 形态

```json
{
  "mcpServers": {
    "req-engine-discuss": {
      "command": "C:\\Users\\yyy\\Desktop\\需求引擎\\req-engine\\target\\debug\\req-engine.exe",
      "args": ["mcp", "--pair", "disc_…", "--home", "C:\\Users\\yyy\\Desktop\\需求引擎\\req-engine\\data"]
    },
    "req-engine-build": {
      "command": "C:\\Users\\yyy\\Desktop\\需求引擎\\req-engine\\target\\debug\\req-engine.exe",
      "args": ["mcp", "--pair", "build_…", "--home", "C:\\Users\\yyy\\Desktop\\需求引擎\\req-engine\\data"]
    }
  }
}
```

### 各宿主大致放哪

| 宿主 | 你改哪里 |
|------|----------|
| **Cursor** | 设置 → MCP → 编辑 `mcp.json`，或项目 `.cursor/mcp.json` |
| **Claude Desktop** | `claude_desktop_config.json` 的 `mcpServers` |
| **Codex CLI** | 按其文档的 MCP servers 配置（stdio command + args） |
| **Grok Build / Kimi 等** | 若支持 MCP：同样登记 **command = req-engine.exe** + `mcp --pair …`；若不支持 MCP，只能用 HTTP `/v1` + token 调 REST |

**注意：** 桌面窗口与 MCP **共用同一个 `--home` 数据库**，才能在 UI 里看到 Agent 创建/推进的卡片。

## 与桌面 / HTTP 的关系

```
人 ──桌面 UI──► HTTP /v1  (admin token，自动注入)
Agent ──MCP stdio──► 同一 SQLite 引擎  (planner | foreman token)
```

Token **不是**「接 AI」；是 **谁可以执行哪些动词**。  
引擎无智能；智能在 Agent 宿主一侧。

## 常见问题

**Q: 为什么要 token？**  
多客户端（人 + 多个 agent）共用一台引擎时的角色门禁，防止任意进程把需求标成 done。

**Q: 桌面为什么还显示高级配置？**  
仅连接失败或调试时需要。正常桌面启动会注入 admin，状态栏应显示「本地引擎已连接」。

**Q: 能否同时开 planner 与 foreman？**  
可以：两个 MCP 进程、不同 role/token、同一 `--home`。
