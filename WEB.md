# Windows Fluent 看板 · 连接真实 API

看板文件（运行时）：[`req-engine/web/index.html`](./req-engine/web/index.html)  
静态 demo：[`demo/需求引擎_UI_Windows_Fluent.html`](./demo/需求引擎_UI_Windows_Fluent.html) · [demo 索引](./demo/需求引擎_UI_索引.html)

API 默认：`http://127.0.0.1:7420/v1`  
鉴权：`Authorization: Bearer <admin token>`

## 一键验收步骤

### 1. 初始化并 seed

在 `req-engine` 目录：

```powershell
cd C:\Users\yyy\Desktop\需求引擎\req-engine

# 使用仓库内 data 目录（已有 tokens 也可 --force 重建）
cargo run -- init --home ./data --seed
# 若 DB 已存在且只想补 seed：
# cargo run -- seed --home ./data
```

`init --seed` 会：

- 创建 SQLite + migrations  
- 生成 admin / planner / foreman token，写入 `data/tokens.txt`  
- 插入 4 个演示项目：`demo-shop`、`trace-sight`、`req-engine`、`mobile-h5` 及样例需求  

### 2. 启动 HTTP

```powershell
cargo run -- serve --home ./data --host 127.0.0.1 --port 7420
```

健康检查（无需 token）：

```powershell
curl http://127.0.0.1:7420/v1/health
```

### 3. 读取 admin token

打开 `req-engine/data/tokens.txt`，复制 `admin=` **后面**的整串，例如：

```
admin=req_xxxxxxxx...
```

### 4. 打开看板 HTML

**推荐（Live Server / 任意静态服务）**，Origin 为 `http://127.0.0.1:5500` 等，后端 CORS 已放行：

| 端口 | Origin |
|------|--------|
| 5500 | VS Code Live Server 等 |
| 3000 / 5173 / 8080 | 常见本地静态服 |

也可 **直接双击** 用 `file://` 打开：后端 CORS 允许 `Origin: null`。

若用 Python 起静态服（在桌面「需求引擎」目录）：

```powershell
cd C:\Users\yyy\Desktop\需求引擎
python -m http.server 8080
# 浏览器打开 http://127.0.0.1:8080/demo/需求引擎_UI_索引.html
```

### 5. Token（角色门禁，不是 AI 密钥）

- **桌面模式**：启动时自动注入 admin token，一般**不用填**；仅连接失败时点「高级」
- **浏览器调试**：展开顶部配置条，粘贴 `data/tokens.txt` 里 `admin=` 后的值  
- Base 默认：`http://127.0.0.1:7420/v1`  
- Token 含义：admin / planner / foreman 身份，用于动词权限（与引擎「零 LLM」不矛盾）

localStorage 键：

| Key | 含义 |
|-----|------|
| `req_engine_base` | API 前缀，默认 `http://127.0.0.1:7420/v1` |
| `req_engine_token` | Bearer 明文 token |

MCP 接入：见 [`req-engine/docs/MCP.md`](./req-engine/docs/MCP.md)。

### 6. 预期结果

1. 顶栏 Tabs 出现 **4 个项目**（seed）  
2. 切换项目后四栏看板有 seed 需求（多为 `todo`）  
3. 悬停「需求」列标题 → **新建需求** → 创建后出现在 **需求（todo）** 列  
4. 对处于 **审核中（review）** 的卡片打开抽屉 → **确认完成** → `POST .../complete-review` `{pass:true}` → 卡片进入 **已完成**  
5. 可选：每 **3 秒** 静默轮询刷新  

> Seed 需求初始全是 `todo`。若要测「确认完成」，需先用 foreman/admin 走 claim → submit-review，或用 curl 把某条推到 `review`。

### 用 curl 把需求推到 review（测完成按钮）

```powershell
$homeDir = "C:\Users\yyy\Desktop\需求引擎\req-engine\data"
$admin = (Select-String -Path "$homeDir\tokens.txt" -Pattern '^admin=').Line -replace '^admin=',''
$foreman = (Select-String -Path "$homeDir\tokens.txt" -Pattern '^foreman=').Line -replace '^foreman=',''

# 列出 demo-shop 需求，记下 id
curl -s -H "Authorization: Bearer $admin" http://127.0.0.1:7420/v1/projects/demo-shop/requirements

$reqId = "<粘贴某条 id>"
curl -s -X POST -H "Authorization: Bearer $foreman" "http://127.0.0.1:7420/v1/requirements/$reqId/claim"
curl -s -X POST -H "Authorization: Bearer $foreman" "http://127.0.0.1:7420/v1/requirements/$reqId/submit-review"

# 回到 UI 刷新，在「审核中」打开卡片 → 确认完成
```

## CORS 说明

后端允许：

- `http://127.0.0.1:5500` / `localhost:5500`  
- `3000` / `5173` / `8080` 同源变体  
- `null`（`file://`）  

其他 Origin 会浏览器拦请求；请换上述端口或 `file://`。

## UI 能力对照

| UI | API |
|----|-----|
| 项目 Tabs | `GET /v1/projects` |
| 看板卡片 | `GET /v1/projects/:id/requirements` |
| 抽屉详情/时间线 | `GET /v1/requirements/:id`（含 `events[]`） |
| 新建需求 | `POST /v1/projects/:id/requirements` |
| 确认完成 | `POST /v1/requirements/:id/complete-review` `{pass:true}` |
| 驳回 | 同上 `{pass:false, reason?}` |
| 新建项目（Tabs +） | `POST /v1/projects`（需 admin） |

**不做：** 自由改 status；状态只由引擎动词驱动。
