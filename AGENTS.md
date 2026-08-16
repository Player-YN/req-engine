# 需求引擎 (Requirements Engine)

## One-liner

**Zero-LLM** verb-based requirements board + SQLite engine; Windows kanban UI; MCP planner/foreman for coding agents.

## Goal

Decouple “writing requirements” from “implementing them”: discussion agents create todos; implementer agents claim/submit; humans approve review via UI. Engine only stores state, enforces transitions, exposes HTTP + MCP.

## Hard bans

- No LLM / model API inside `req-engine`
- No free-form `update_status` / `set_status` API
- No hard-delete of requirements in MVP (soft `cancel` only)
- No engine-side Worker/IDLE/subagent scheduling
- Do not log MCP protocol noise to **stdout** (stderr only)
- Do not commit `data/`, `tokens.txt`, `*.sqlite`, secrets

## Layout

| Path | Role |
|------|------|
| `req-engine/` | Rust crate: CLI `init` / `serve` / `mcp` / `desktop` |
| `req-engine/web/index.html` | Windows UI shipped with engine (API-backed) |
| `demo/` | Static HTML UI mocks (Windows / macOS / iOS); not runtime |
| `启动需求引擎.bat` / `.vbs` | Double-click → desktop window; VBS hides the console |
| `req-engine/docs/MCP.md` | MCP stdio for planner/foreman agents |
| `req-engine/examples/mcp.*.example.json` | Host config templates |
| `WEB.md` | Browser open / token / CORS notes |
| `后端实现计划_Backend-Plan.md` | Backend plan + MCP product rules |
| `TASK_BOARD.md` | MVP task checklist A–E |
| `NOTES.md` | Agent landing / phase notes |

## Build / test / run

```powershell
cd req-engine
cargo test
cargo build --release

# API only
cargo run -- init --home ./data --seed
cargo run -- serve --home ./data --host 127.0.0.1 --port 7420

# Native Windows window (starts API + WebView; empty board unless you seed)
cargo run -- desktop --home ./data
# Optional demo data only: --seed-if-missing  or  cargo run -- seed --home ./data
```

Default data home: `%USERPROFILE%\.req-engine` (override `REQ_ENGINE_HOME` or `--home`).

Smoke: `req-engine/scripts/smoke.ps1`

## Status verbs (only)

`create→todo` · `claim→in_progress` · `report_progress` (no change) · `submit_for_review→review` · `complete_review pass→done|fail→todo` · `release→todo` · `cancel→cancelled`

Roles: **admin** | **planner** | **foreman** (Bearer SHA-256 in DB). Product MCP uses per-project pair codes (`disc_` / `build_`), not global tokens.

MCP: `mcp --pair <disc_|build_>` binds one project + seat. Discuss: list/get/create/update/cancel todo only. Foreman: claim/progress/submit/release; no `complete_review` (admin HTTP/UI only). Live occupancy is a SQLite heartbeat (15s TTL); seat face uses MCP `clientInfo.name` (small known-host map, else identicon).

## Ports / UI

- HTTP: `127.0.0.1:7420` (`/v1/*` API, `/` static UI when served)
- UI tokens: localStorage `req_engine_base`, `req_engine_token` (desktop injects admin token)
- Desktop close hides to the tray; quit only from the tray menu

## Pointers

- Product MCP matrix: `后端实现计划_Backend-Plan.md` §15
- UI connect: `WEB.md`
- Domain code: `req-engine/src/domain/state.rs`
- Services: `req-engine/src/services/`
- Pair codes / onboarding copy pack: `services/pair_codes.rs`, `services/onboarding.rs`
- Seat occupancy + host face: `services/presence.rs`, `services/client_host.rs`
