# NOTES — agent landing

## Current phase (2026-08-15)

Product MCP path is **per-project pair codes**, not global `tokens.txt`.

- Launch: `启动需求引擎.bat` → `启动需求引擎.vbs` (hidden console, `REQ_ENGINE_SILENT=1`)
- Desktop: WebView2 + self-drawn titlebar; injects admin token; **does not attach to a foreign port**
- Each project: `disc_*` / `build_*` hashes in SQLite; plaintext in `{home}/pair-codes.json`
- UI **复制讨论侧/实现侧接入** = pair code + **portable** onboarding (any MCP client; discover `req-engine` on PATH + `REQ_ENGINE_HOME` / `~/.req-engine`; no issuer machine path, no product brand names)
- Desktop **✕ = hide to tray**; tray **退出** kills the process (HTTP stays up while hidden)
- **已接入** = MCP `initialize` + heartbeat on `seat_presence` (clientInfo.name). Copy pack ≠ 已接入. Panel: UI「接入状态」
- Discuss MCP tools: list/get/create/update/cancel **todo only**. Foreman: list_ready/claim/progress/submit/release
- **在座**: MCP `--pair` process heartbeats `seat_presence` (15s TTL). Copying a pack ≠ seated
- Seat face: MCP initialize `clientInfo.name` → small alias map (`client_host.rs`) or letter identicon. Self-reported, not auth
- Tests: `cd req-engine; cargo test` (82). Smoke: `scripts/smoke.ps1`
- Prompt Launcher is a **sibling product** (`Desktop/Prompt Launcher` PRD; old proto under `prompt-launcher/`). Not part of req-engine
- Static UI mocks: `demo/` only

**Do not** treat `POST .../agents/{seat}/ack` as live occupancy. That only stamps `discuss_agent_at` / `build_agent_at` (configured).

**Existing `./data` / `%USERPROFILE%\.req-engine`** may need a restart so migrations 005–007 run.

## Cold start for a new agent

1. Read root `AGENTS.md`
2. Read `req-engine/README.md` “Demo in 3 minutes”
3. Domain truth: `req-engine/src/domain/state.rs`
4. Do not invent free status PATCH or engine-side worker pool

## Open product constraints

- complete_review: admin/HTTP (and UI 确认完成) — not foreman MCP
- Hard delete: out of MVP
- macOS/iOS mocks exist but **not** on real API yet

## Evidence / history

- Task board: `TASK_BOARD.md` (A–E done)
- Backend plan: `后端实现计划_Backend-Plan.md`
