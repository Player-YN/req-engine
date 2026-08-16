-- Requirements Engine schema (MVP)

CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    color       TEXT NOT NULL DEFAULT '#6366f1',
    blurb       TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS requirements (
    id                TEXT PRIMARY KEY NOT NULL,
    project_id        TEXT NOT NULL REFERENCES projects(id),
    title             TEXT NOT NULL,
    description       TEXT NOT NULL DEFAULT '',
    priority          TEXT NOT NULL DEFAULT 'medium',
    status            TEXT NOT NULL DEFAULT 'todo',
    scope_json        TEXT NOT NULL DEFAULT '[]',
    non_scope_json    TEXT NOT NULL DEFAULT '[]',
    acceptance_json   TEXT NOT NULL DEFAULT '[]',
    dependencies_json TEXT NOT NULL DEFAULT '[]',
    claimed_by        TEXT,
    progress_summary  TEXT,
    blocked_reason    TEXT,
    external_run_id   TEXT,
    created_by        TEXT NOT NULL,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_requirements_project
    ON requirements(project_id);
CREATE INDEX IF NOT EXISTS idx_requirements_status
    ON requirements(status);

CREATE TABLE IF NOT EXISTS events (
    id              TEXT PRIMARY KEY NOT NULL,
    project_id      TEXT NOT NULL REFERENCES projects(id),
    requirement_id  TEXT REFERENCES requirements(id),
    actor           TEXT NOT NULL,
    kind            TEXT NOT NULL,
    message         TEXT NOT NULL DEFAULT '',
    payload_json    TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_project
    ON events(project_id);
CREATE INDEX IF NOT EXISTS idx_events_requirement
    ON events(requirement_id);

CREATE TABLE IF NOT EXISTS api_tokens (
    token_hash  TEXT PRIMARY KEY NOT NULL,
    role        TEXT NOT NULL,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
