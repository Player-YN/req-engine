# Smoke test & known limits

## Quick smoke (HTTP)

From the `req-engine` repo root (PowerShell):

```powershell
# Full path: init/seed if needed, start serve if down, run lifecycle, stop server we started
.\scripts\smoke.ps1

# Reuse an already-running server on 7420
.\scripts\smoke.ps1 -SkipStart

# Custom home / port
.\scripts\smoke.ps1 -DataHome .\data -Port 7420 -KeepServer
```

**What it checks**

| Step | Endpoint | Role |
|------|----------|------|
| health | `GET /v1/health` | none |
| list projects | `GET /v1/projects` | admin |
| create requirement | `POST /v1/projects/:id/requirements` | admin |
| claim | `POST /v1/requirements/:id/claim` | foreman |
| submit-review | `POST /v1/requirements/:id/submit-review` | foreman |
| complete-review | `POST /v1/requirements/:id/complete-review` `{pass:true}` | admin |
| detail | `GET /v1/requirements/:id` | admin |

Exit code `0` = all PASS; non-zero = FAIL.

**Port already in use:** if something else answers `/v1/health` on that port, the script reuses it. If the port is taken by a non-req-engine process, health fails and the script reports FAIL.

**Unit / integration tests** (no server needed):

```powershell
cargo test
```

## MCP manual smoke (optional)

stdio MCP is awkward to script; service mapping is covered by unit tests.

1. `cargo run -- init --home ./data --seed` (or use existing data)
2. Client config with planner token → `list_projects` / `create_requirement`
3. Foreman token → `list_ready_tasks` → `claim_task` → `report_progress` → `submit_for_review`
4. **complete_review is not on MCP** — use HTTP with admin token

## Known limits (MVP)

| Limit | Detail |
|-------|--------|
| **No hard delete** | Soft-cancel only (`cancel` → `cancelled`). Rows stay in SQLite. |
| **No free-form status** | No `PATCH` / `set_status`. Status changes only via verbs. |
| **`complete_review` not on MCP** | Foreman MCP has claim / progress / submit / release only. Review pass/fail is **admin HTTP** (or domain planner/admin; HTTP MVP = admin only). |
| **Planner cannot claim** | HTTP returns 403; MCP planner surface has no claim tool. |
| **Planner cancel** | Only from `todo`. Admin can cancel `todo` / `in_progress` / `review`. Foreman cannot cancel. |
| **Claim atomic** | One winner; second claim fails. |
| **Tokens** | Plaintext only in `tokens.txt` (local dev). DB stores SHA-256 hashes. |
| **CORS** | Localhost static ports `5500`, `3000`, `5173`, `8080`, plus `null` (`file://`). |
| **Zero LLM** | Engine never calls a model. |
| **Single process / SQLite** | Local file DB; not multi-writer production HA. |

See also [WEB.md](../WEB.md) for the Windows Fluent board UI path.


