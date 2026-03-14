-- Re-introduce an optional user-defined display name for nodes.
ALTER TABLE nodes
    ADD COLUMN IF NOT EXISTS name text;
