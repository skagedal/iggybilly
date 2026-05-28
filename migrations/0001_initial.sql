-- Users authenticate with a username + argon2-hashed password.
-- is_admin gates the user-management subcommands (reset-password etc.)
-- when those are wired into the web UI later; today they're CLI-only.
CREATE TABLE users (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    username     TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    is_admin     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Audio clips. storage_filename is a uuid + original extension so two
-- uploads with the same display name don't collide on disk;
-- original_filename keeps the upload's filename for the Download link.
-- recording_date is whatever lofty extracted from ID3/MP4 metadata on
-- upload (nullable: not every file has it).
CREATE TABLE clips (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    name               TEXT NOT NULL,
    storage_filename   TEXT NOT NULL UNIQUE,
    original_filename  TEXT NOT NULL,
    content_type       TEXT NOT NULL,
    size_bytes         INTEGER NOT NULL,
    uploaded_by        INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    uploaded_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    recording_date     TEXT
);

CREATE INDEX idx_clips_uploaded_at ON clips(uploaded_at DESC);

-- Case-insensitive uniqueness on clip names. The upload handler retries
-- with " (2)", " (3)", ... suffixes when a name collides, and the
-- rename handler returns 409.
CREATE UNIQUE INDEX idx_clips_name_unique ON clips(name COLLATE NOCASE);

-- Labels are lower-kebab-case strings validated in the labels handler
-- (Unicode lowercase letters + digits + hyphens between groups).
CREATE TABLE labels (
    id   INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

-- Labelling is many-to-many. added_by lets us show who first labelled
-- a clip, even if that's not surfaced in the UI yet.
CREATE TABLE clip_labels (
    clip_id   INTEGER NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
    label_id  INTEGER NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    added_by  INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    added_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (clip_id, label_id)
);

CREATE INDEX idx_clip_labels_label ON clip_labels(label_id);
