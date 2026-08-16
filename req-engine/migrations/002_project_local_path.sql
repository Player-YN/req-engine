-- Optional local folder binding for a project (empty = unbound).
-- Additive: existing rows get DEFAULT ''.

ALTER TABLE projects ADD COLUMN local_path TEXT NOT NULL DEFAULT '';
