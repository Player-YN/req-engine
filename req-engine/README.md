# req-engine

Product README (why + how to run): [../README.md](../README.md) · [简体中文](../README.zh-CN.md)

Requirements Engine — a **verb-based** requirement lifecycle store.  
**Zero LLM** in the engine. Soft-cancel only (no hard delete in MVP). Local **SQLite**.

## Demo in 3 minutes

### Recommended: native Windows window

```powershell
cd req-engine
cargo run -- desktop --home ./data
# or double-click ..\启动需求引擎.bat
```

This starts the API + static UI and opens a **WebView2 OS window** (not a browser tab).  
On first run it auto-inits the DB (empty projects — no demo seed by default) and injects the admin token into the UI.  
Demo projects are opt-in: `cargo run -- desktop --home ./data --seed-if-missing` or `req-engine seed`.

Requires **WebView2 Runtime** (preinstalled on most Windows 10/11).

### API-only + browser (optional)

```powershell
cd req-engine

# 1) Init DB + role tokens + demo projects (once)
cargo run -- init --home ./data --seed

# 2) Start API (+ static files from web/)
cargo run -- serve --home ./data --host 127.0.0.1 --port 7420
```

In another terminal (or after API is up):

```powershell
# 3) Automated HTTP lifecycle smoke
.\scripts\smoke.ps1 -SkipStart
```

**Browser UI**

1. Read admin token from `data\tokens.txt` (`admin=...`).
2. Open `http://127.0.0.1:7420/` (runtime UI from `web/`). [`../demo/`](../demo/) HTML files are static mocks, not the shipped board.
3. Paste Base `http://127.0.0.1:7420/v1` + admin token if not auto-injected.
4. Expect 4 seed projects; create cards; **确认完成** as admin.

**Optional MCP** (stdio; logs on stderr) — full guide: [`docs/MCP.md`](docs/MCP.md)

```powershell
# Copy a seat pack from the desktop (pair code + onboarding prompt), then:
cargo run -- mcp --pair disc_… --home ./data
cargo run -- mcp --pair build_… --home ./data
```

Host templates: `examples/mcp.planner.example.json`, `examples/mcp.foreman.example.json`.

Discuss tools: `list_requirements`, `get_requirement`, `create_requirement`, `update_requirement` (todo only), `cancel_requirement` (todo only).  
Foreman tools: `list_ready_tasks`, `get_requirement`, `claim_task`, `report_progress`, `submit_for_review`, `release_task`.  
**Not on MCP:** `complete_review` (desktop/HTTP admin only), free-form `set_status`, hard delete.

**Token ≠ AI key** — admin HTTP ACL. Product MCP uses per-project `disc_` / `build_` pair codes. Desktop injects admin automatically.

---

## Status machine (verbs only)

There is **no free-form `update_status` / `set_status` API**. Status changes only via verbs:

| Verb | From | To | Notes |
|------|------|-----|--------|
| *(create)* | — | `todo` | Always starts as todo |
| `claim_task` | `todo` | `in_progress` | Sets `claimed_by`; atomic (one winner) |
| `report_progress` | `in_progress` / `review` | *(unchanged)* | Progress note only; claimant or admin |
| `submit_for_review` | `in_progress` | `review` | Claimant or admin |
| `complete_review` pass | `review` | `done` | Domain: planner/admin · HTTP MVP: **admin only** |
| `complete_review` fail | `review` | `todo` | Clears claim · HTTP MVP: **admin only** |
| `release_task` | `in_progress` | `todo` | Claimant or admin; clears claim |
| `cancel` | role-scoped | `cancelled` | Soft cancel only |

**Cancel (MVP):**
- **planner**: only from `todo`
- **admin**: from `todo` / `in_progress` / `review`
- **foreman**: cannot cancel

**Terminal:** `done`, `cancelled` — no further transitions.

**Roles:** `admin` | `planner` | `foreman` (Bearer token → SHA-256 hash in `api_tokens`).

## Data home

| | |
|--|--|
| Env | `REQ_ENGINE_HOME` |
| Default (Windows) | `%USERPROFILE%\.req-engine` |
| Default (Unix) | `~/.req-engine` |
| Demo / smoke | `./data` via `--home ./data` |

Inside the home directory:

- `req-engine.sqlite` — SQLite database  
- `tokens.txt` — bootstrap plaintext tokens for local dev (gitignored)

## One-shot path

```powershell
# Build
cargo build

# Initialize data dir + DB + migrations + three role tokens + demo seed
cargo run -- init --home ./data --seed

# If DB already exists and you only need demo projects:
cargo run -- seed --home ./data

# Re-init (destructive — wipes DB + tokens)
cargo run -- init --home ./data --seed --force

# Start HTTP API
cargo run -- serve --home ./data --host 127.0.0.1 --port 7420
```

`init` will:

1. Create the home directory  
2. Create SQLite DB and run migrations  
3. Generate **admin**, **planner**, **foreman** tokens  
4. Store **SHA-256 hashes** in `api_tokens`  
5. Print plaintext once and write them to `tokens.txt` (local dev only)  
6. With `--seed`, insert four demo projects and sample requirements  

## HTTP API (`/v1`)

Auth: `Authorization: Bearer <plaintext-token>`  
CORS: allows common localhost static origins (`5500`, `3000`, `5173`, `8080`, and `null`).

| Method | Path | Role |
|--------|------|------|
| GET | `/v1/health` | any (no auth) |
| GET | `/v1/projects` | any authed |
| POST | `/v1/projects` | admin (`local_path` optional) |
| PATCH | `/v1/projects/:id` | admin (`name`/`color`/`blurb`/`local_path` optional) |
| GET | `/v1/projects/:id/requirements` | any authed |
| POST | `/v1/projects/:id/requirements` | admin or planner |
| GET | `/v1/requirements/:id` | any authed (includes `events` timeline) |
| POST | `/v1/requirements/:id/claim` | foreman or admin |
| POST | `/v1/requirements/:id/progress` | foreman or admin |
| POST | `/v1/requirements/:id/submit-review` | foreman or admin |
| POST | `/v1/requirements/:id/complete-review` | admin only |
| POST | `/v1/requirements/:id/cancel` | planner (todo) or admin |
| POST | `/v1/requirements/:id/release` | foreman (claimant) or admin |

### curl / PowerShell examples

```powershell
$homeDir = ".\data"
$admin   = (Select-String -Path "$homeDir\tokens.txt" -Pattern '^admin=').Line -replace '^admin=',''
$foreman = (Select-String -Path "$homeDir\tokens.txt" -Pattern '^foreman=').Line -replace '^foreman=',''

# Health (no auth)
curl.exe -s http://127.0.0.1:7420/v1/health

# List projects
curl.exe -s -H "Authorization: Bearer $admin" http://127.0.0.1:7420/v1/projects

# Create requirement on seeded project
curl.exe -s -X POST http://127.0.0.1:7420/v1/projects/demo-shop/requirements `
  -H "Authorization: Bearer $admin" `
  -H "Content-Type: application/json" `
  -d "{\"title\":\"New feature\",\"description\":\"...\",\"priority\":\"high\",\"scope\":[\"api\"],\"acceptance_criteria\":[\"tests pass\"]}"

# Claim → submit → complete (replace <REQ_ID>)
curl.exe -s -X POST http://127.0.0.1:7420/v1/requirements/<REQ_ID>/claim `
  -H "Authorization: Bearer $foreman"
curl.exe -s -X POST http://127.0.0.1:7420/v1/requirements/<REQ_ID>/submit-review `
  -H "Authorization: Bearer $foreman"
curl.exe -s -X POST http://127.0.0.1:7420/v1/requirements/<REQ_ID>/complete-review `
  -H "Authorization: Bearer $admin" `
  -H "Content-Type: application/json" `
  -d "{\"pass\":true,\"reason\":\"lgtm\"}"
```

## Windows UI

See **[WEB.md](../WEB.md)** for the Fluent board:

- File: `web/index.html` (runtime) or `../demo/需求引擎_UI_Windows_Fluent.html` (static mock)
- Paste admin Bearer token; Base `http://127.0.0.1:7420/v1`
- CORS-friendly origins: Live Server `5500`, `3000`/`5173`/`8080`, or `file://`

## MCP server (`req-engine mcp`)

stdio MCP server via the official Rust SDK ([`rmcp`](https://crates.io/crates/rmcp)).  
**stdout is the MCP wire** — logs go to **stderr** only. No LLM calls inside the engine.

Product path is a **per-project pair code** from the desktop (**Copy discuss** / **Copy build**). `--role` + `--token` is a debug back door — do not ship it.

```powershell
cargo run -- mcp --pair disc_… --home ./data
cargo run -- mcp --pair build_… --home ./data
```

| Seat | Pair | Tools |
|------|------|--------|
| **discuss** (`disc_`) | planner surface | `list_requirements`, `get_requirement`, `create_requirement`, `update_requirement` (todo only), `cancel_requirement` (todo only) |
| **build** (`build_`) | foreman surface | `list_ready_tasks`, `get_requirement`, `claim_task`, `report_progress`, `submit_for_review`, `release_task` |

**Not exposed on MCP:** free-form `set_status`, `complete_review` (admin/HTTP only), hard delete.

`list_ready_tasks` returns `todo` requirements whose dependency ids (if any) are all `done`.

### Client config example

```json
{
  "mcpServers": {
    "req-engine-discuss": {
      "command": "path/to/req-engine",
      "args": ["mcp", "--pair", "disc_…", "--home", "path/to/data"]
    },
    "req-engine-build": {
      "command": "path/to/req-engine",
      "args": ["mcp", "--pair", "build_…", "--home", "path/to/data"]
    }
  }
}
```

## Smoke & tests

```powershell
cargo test                          # unit + HTTP + MCP mapping (no live server)
.\scripts\smoke.ps1                 # live e2e against ./data (starts serve if needed)
.\scripts\smoke.ps1 -SkipStart      # assume server already on :7420
```

Details and **known limits**: **[SMOKE.md](./SMOKE.md)**.

Includes:

- Pure state-machine unit tests (illegal transitions, roles, terminal states)
- Create → `todo`
- Claim atomicity: second claim fails; only one `claimed_by`
- Service happy path: create → claim → submit → complete pass
- HTTP: create → claim → submit-review → complete-review pass
- HTTP: double claim only one succeeds
- HTTP: planner cannot claim (403)
- MCP auth: role/surface matrix; admin allowed either
- MCP tool mapping: planner create/update/cancel; foreman claim/progress/submit/release
- `update_requirement` only when todo; `list_ready_tasks` respects dependencies

## CLI

| Command | Status |
|---------|--------|
| `req-engine init [--seed] [--force]` | Implemented (seed is opt-in) |
| `req-engine seed` | Implemented (idempotent-ish) |
| `req-engine serve --host 127.0.0.1 --port 7420` | Implemented |
| `req-engine desktop [--seed-if-missing]` | Implemented (no demo seed by default) |
| `req-engine mcp --pair disc_\|build_…` | Implemented (stdio; product path) |
| `req-engine mcp --role planner\|foreman` | Implemented (stdio; **debug only**) |

Binary name: **`req-engine`**.

## Schema (MVP)

- **projects** — id, name, color, blurb, local_path (empty = unbound), created_at, updated_at  
- **requirements** — id, project_id, title, description, priority, status, scope_json, non_scope_json, acceptance_json, dependencies_json, claimed_by, progress_summary, blocked_reason, external_run_id, created_by, created_at, updated_at  
- **events** — id, project_id, requirement_id (nullable), actor, kind, message, payload_json, created_at  
- **api_tokens** — token_hash, role, name, created_at  

## Library layout

```
src/
  domain/state.rs   # pure transitions by role
  domain/models.rs
  db/               # rusqlite + migrations
  services/         # create, claim, progress, review, release, cancel, seed
  http/             # axum REST /v1 + Bearer auth + CORS
  mcp/              # stdio MCP (planner | foreman) via rmcp
  paths.rs          # REQ_ENGINE_HOME / defaults
  main.rs           # clap CLI
scripts/
  smoke.ps1         # live HTTP e2e
```

## What is not in MVP

- Free-form PATCH status / `set_status`
- Hard delete
- Any LLM calls
- `complete_review` on foreman MCP (admin/HTTP only)
- Multi-tenant / remote auth provider
