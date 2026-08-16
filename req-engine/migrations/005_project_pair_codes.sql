-- Per-project MCP pairing codes (hashes only). Plaintext lives in pair-codes.json.
ALTER TABLE projects ADD COLUMN discuss_pair_hash TEXT;
ALTER TABLE projects ADD COLUMN build_pair_hash TEXT;
