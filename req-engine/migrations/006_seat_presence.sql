-- Live MCP occupancy (stdio process heartbeat). Not the same as pair-code issue.
CREATE TABLE IF NOT EXISTS seat_presence (
    project_id TEXT NOT NULL,
    seat TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    started_at TEXT NOT NULL,
    pid INTEGER NOT NULL,
    PRIMARY KEY (project_id, seat)
);
