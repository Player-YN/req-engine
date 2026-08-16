# req-engine

**English** · [简体中文](README.zh-CN.md)

**A Zero-LLM verb-based requirements store.** Rust + SQLite enforce the lifecycle. A Windows WebView2 board is the human seat. MCP `disc_` / `build_` pair codes bind coding agents to one project and one role. The engine never calls a model.

[![Rust](https://img.shields.io/badge/Rust-axum%20%2B%20rusqlite%20%2B%20rmcp-dea584?logo=rust)](req-engine/Cargo.toml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](req-engine/Cargo.toml)

Real entry: double-click [`启动需求引擎.bat`](启动需求引擎.bat) / [`启动需求引擎.vbs`](启动需求引擎.vbs), or `cargo run -- desktop` from [`req-engine/`](req-engine/).

---

## Why this exists

Writing a requirement and implementing it are different jobs. Discussion agents should create and refine todos. Implementer agents should claim work and submit it. A human should decide done. Most “AI boards” collapse those jobs into a chatbot that can also `PATCH` status to whatever it likes.

`req-engine` is the opposite: a **local state machine with seats**. Intelligence stays in the agent host (Cursor, Claude, Codex, Grok, …). The engine only stores facts, rejects illegal verbs, and shows who is seated.

## Why verbs beat `set_status`

A free-form status API is an invitation to invent workflow. Two agents can mark the same card `done` without claiming it. A planner can skip review. A confused model can write `"in-review"` instead of `"review"`. You then debug the **prompt**, not the product.

Here status is not a field you write. It is the **result of a verb**:

| Verb | From | To | Who (HTTP) |
|------|------|----|------------|
| create | — | `todo` | admin, planner |
| `claim` | `todo` | `in_progress` | foreman, admin — atomic, one winner |
| `report_progress` | `in_progress` / `review` | *unchanged* | claimant or admin |
| `submit_for_review` | `in_progress` | `review` | claimant or admin |
| `complete_review` pass / fail | `review` | `done` / `todo` | **admin only** (desktop / HTTP) |
| `release` | `in_progress` | `todo` | claimant or admin |
| `cancel` | role-scoped | `cancelled` | planner: `todo` · admin: any non-terminal |

There is **no** `set_status` / `update_status` on HTTP or MCP. `done` and `cancelled` are terminal. Soft-cancel only — no hard delete. Claim uses `BEGIN IMMEDIATE` plus `WHERE status = 'todo' AND claimed_by IS NULL`, so two MCP processes cannot both win.

The machine lives in [`req-engine/src/domain/state.rs`](req-engine/src/domain/state.rs). Services apply it; HTTP and MCP never bypass it.

## Why Zero-LLM is a feature

If the store can think, you cannot audit it. A model inside the engine would: invent transitions, hide failures in prose, and couple your board to an API key.

This process does **not** call OpenAI, Anthropic, or any other model. MCP is a **server** the host connects to. Tokens and pair codes are ACL, not “AI keys”. Seat faces are a display map from self-reported `clientInfo.name` (known host or identicon) — **not authentication**.

You can unit-test every illegal transition without a GPU, a network, or a prompt.

---

## Architecture

```
 Human ── WebView2 board ── HTTP /v1 (admin Bearer) ─┐
                                                     │
 Discuss agent ── MCP stdio --pair disc_…  ──────────┼──►  req-engine
 Build agent   ── MCP stdio --pair build_… ──────────┘         │
                                                               ▼
                                              SQLite  (requirements, events,
                                              token hashes, pair hashes,
                                              seat_presence heartbeats)
```

| Surface | Job |
|---------|-----|
| **Domain** | Pure transitions by role. No I/O. |
| **Services** | Verbs + events. Claim is transactional. |
| **HTTP** | Local board + admin review. Bearer → SHA-256 → `admin` / `planner` / `foreman`. |
| **MCP** | Product path: `--pair` binds **one project + one seat**. Debug `--role` + token exists; do not ship it. |
| **Desktop** | Starts the API, serves [`req-engine/web/index.html`](req-engine/web/index.html), opens a **native** window (not a browser tab). Close hides to the tray; **退出** quits. |

`demo/*.html` files are **static mocks**. They are not the runtime UI.

---

## Seats, pair codes, occupancy

Each project has two seats:

| Seat | Pair prefix | MCP surface | May | Must not |
|------|-------------|-------------|-----|----------|
| Discuss | `disc_` | planner | list / get / create / update `todo` / cancel `todo` | claim, submit, implement, `complete_review` |
| Build | `build_` | foreman | `list_ready_tasks`, claim, progress, submit, release | create cards, `complete_review` |

Desktop **Copy discuss / Copy build** puts a pair code plus an onboarding prompt on the clipboard. Paste that into the **agent host** MCP config — the board is not an MCP client.

- SQLite stores **SHA-256** of the code (`discuss_pair_hash` / `build_pair_hash`).
- Plaintext lives in `{home}/pair-codes.json` (gitignored). Rotate invalidates the old code immediately.
- A seated MCP process heartbeats every **4s**. The UI treats a seat as occupied while `last_seen` is within **15s**. Clearing the process (or a different pid) drops the face.

`list_ready_tasks` returns `todo` cards whose dependency ids are all `done`.

---

## Run it

**Windows, recommended**

1. Install [Rust](https://rustup.rs) and the [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (already on most Win10/11).
2. Double-click `启动需求引擎.bat`. First run builds `target/release/req-engine.exe` if needed, then the VBS launcher starts a **hidden-console** desktop on `--home req-engine/data --port 7420`.
3. Create a project on the board. Copy a discuss or build pack. Register it in Cursor / Claude Desktop / Codex / any MCP host:

```json
{
  "mcpServers": {
    "req-engine-discuss": {
      "command": "C:\\path\\to\\req-engine.exe",
      "args": ["mcp", "--pair", "disc_…", "--home", "C:\\path\\to\\req-engine\\data"]
    }
  }
}
```

Host and desktop **must share `--home`**. Templates: [`req-engine/examples/`](req-engine/examples/). Full matrix: [`req-engine/docs/MCP.md`](req-engine/docs/MCP.md).

**From a terminal**

```powershell
cd req-engine
cargo test
cargo build --release

# Native window (auto-inits an empty DB; injects the admin token)
cargo run -- desktop --home ./data
# Optional demo projects:  --seed-if-missing
# or: cargo run -- seed --home ./data

# API only
cargo run -- init --home ./data --seed
cargo run -- serve --home ./data --host 127.0.0.1 --port 7420
```

Without `--home` / `REQ_ENGINE_HOME`, the default is `%USERPROFILE%\.req-engine` (Unix: `~/.req-engine`). The double-click launchers pin `req-engine/data` so the window and MCP stay on the same file.

Smoke (live HTTP): `req-engine/scripts/smoke.ps1`.

---

## HTTP (local)

Auth: `Authorization: Bearer <token>`. CORS allows common localhost static origins. Health is open; everything else is authed.

| Method | Path | Role |
|--------|------|------|
| `GET` | `/v1/health` | none |
| `GET` / `POST` | `/v1/projects` | any / **admin** |
| `PATCH` | `/v1/projects/:id` | admin |
| `POST` | `/v1/projects/:id/archive` · `unarchive` | admin |
| `GET` / `POST` | `/v1/projects/:id/pair-codes` · `…/:seat/rotate` | admin |
| `POST` | `/v1/projects/:id/requirements` | admin, planner |
| `POST` | `/v1/requirements/:id/claim` · `progress` · `submit-review` · `release` | foreman, admin |
| `POST` | `/v1/requirements/:id/complete-review` | **admin** |
| `POST` | `/v1/requirements/:id/cancel` | planner, admin |

Port **7420**. Desktop injects the admin token into the WebView; you do not paste it for the normal path.

---

## Data home

Inside `{home}`:

| File | What |
|------|------|
| `req-engine.sqlite` | Projects, requirements, event log, token hashes, pair hashes, `seat_presence` |
| `tokens.txt` | Bootstrap **plaintext** admin/planner/foreman (local only) |
| `pair-codes.json` | Per-project `disc_` / `build_` plaintext |

Treat those two plaintext files as secrets. Root [`.gitignore`](.gitignore) already excludes `data/`, `data-*/`, `*.sqlite`, `tokens.txt`, and `pair-codes.json`.

---

## Layout

| Path | Role |
|------|------|
| [`req-engine/`](req-engine/) | Crate: `init` / `serve` / `mcp` / `desktop` |
| [`req-engine/web/index.html`](req-engine/web/index.html) | **Runtime** board (API-backed) |
| [`req-engine/src/domain/state.rs`](req-engine/src/domain/state.rs) | Verb machine |
| [`req-engine/src/mcp/`](req-engine/src/mcp/) | Planner / foreman stdio (`rmcp`) |
| [`req-engine/src/services/presence.rs`](req-engine/src/services/presence.rs) | Occupancy TTL |
| [`demo/`](demo/) | HTML mocks — not shipped as the UI |
| [`WEB.md`](WEB.md) | Browser-connect notes (optional) |

Crate-level developer notes: [`req-engine/README.md`](req-engine/README.md).

---

## What this is not

- Not an agent runtime, worker pool, or IDLE scheduler
- Not a cloud multi-tenant SaaS
- Not a model wrapper — **Zero LLM inside the engine is the contract**
- Not a free-form Kanban you can `PATCH` into consistency

---

MIT. Target repo: [github.com/Player-YN/req-engine](https://github.com/Player-YN/req-engine).
