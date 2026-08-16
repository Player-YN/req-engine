-- Soft-archive projects (hidden from default list; data retained).
ALTER TABLE projects ADD COLUMN archived_at TEXT;
