-- Per-project agent seat acknowledgements (discussion + implementation).
-- Timestamps set when an agent successfully links after MCP setup.
ALTER TABLE projects ADD COLUMN discuss_agent_at TEXT;
ALTER TABLE projects ADD COLUMN build_agent_at TEXT;
