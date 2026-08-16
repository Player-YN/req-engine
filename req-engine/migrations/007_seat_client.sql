-- Self-reported MCP clientInfo stored with occupancy (not a product registry).
ALTER TABLE seat_presence ADD COLUMN client_name TEXT;
ALTER TABLE seat_presence ADD COLUMN client_title TEXT;
ALTER TABLE seat_presence ADD COLUMN client_version TEXT;
