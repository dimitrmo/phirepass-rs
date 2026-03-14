-- Step 2: challenge-response authentication storage

CREATE TABLE IF NOT EXISTS auth_challenges (
    node_id uuid PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
    challenge text NOT NULL,
    expires_at timestamptz NOT NULL
);

CREATE INDEX IF NOT EXISTS auth_challenges_expires_at_idx
    ON auth_challenges (expires_at);