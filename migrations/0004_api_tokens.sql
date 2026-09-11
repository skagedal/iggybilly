-- Bearer tokens for the mobile app.
--
-- The browser keeps a session cookie, which is the right thing for a
-- tab: it is scoped to the origin, it is SameSite=Strict, and it goes
-- away. An app install is the opposite — it stays signed in for months
-- on a device that can be lost — so it gets a token instead, one per
-- install, listed and revocable on the account screen.
--
-- Only a SHA-256 of each token is stored. The plaintext is returned once,
-- at login, and never again: a copy of this table is then not a set of
-- working credentials. SHA-256 rather than argon2 because the token is
-- 256 bits of randomness we generated, not a password someone chose —
-- there is no dictionary to run against it, and every authenticated
-- request pays this hash.
CREATE TABLE api_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash   TEXT NOT NULL UNIQUE,
    -- What the user sees in the device list, e.g. "Simon's iPhone".
    device_name  TEXT NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- Date only, and written at most once a day: enough to recognise a
    -- device you no longer use, without a write on every request.
    last_used_on TEXT
);

-- Authentication looks a token up by its hash on every request, so that
-- lookup gets the UNIQUE index above; this one is for the account
-- screen's "my devices" list.
CREATE INDEX idx_api_tokens_user ON api_tokens(user_id, id DESC);
