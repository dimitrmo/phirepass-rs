-- Drop legacy PAT-era node columns now that node auth is keyed by public_key.
ALTER TABLE nodes
    DROP CONSTRAINT IF EXISTS nodes_token_id_fkey;

ALTER TABLE nodes
    DROP COLUMN IF EXISTS token_id,
    DROP COLUMN IF EXISTS name;
