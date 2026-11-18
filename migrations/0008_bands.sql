-- Several bands on one instance.
--
-- Until now "the band" was the whole database: every user was a member
-- of it because they existed, and every clip was its because there was
-- nowhere else for a clip to be. That is now written down — a band is a
-- row, membership is a row — and once it is written down there can be
-- more than one of it. See specs/drafts/multiple-bands.md.
--
-- The slug is the band's whole name. It appears in every URL for the
-- band's content, so it is validated as lower-kebab-case (the same rule
-- and the same code as a label name) before it ever gets here, and it is
-- NOCASE-unique as a second line of defence: two slugs differing only in
-- case would be two bands with one set of URLs between them.
CREATE TABLE bands (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    slug       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    -- A Discord webhook of this band's own, or NULL to post to the
    -- instance-wide one from IGGYBILLY_DISCORD_WEBHOOK_URL. Defaulting
    -- rather than requiring keeps an instance that has never thought
    -- about bands behaving exactly as it did.
    discord_webhook_url TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Membership, and nothing else. No role: the one privileged actor is the
-- site owner at the command line, nothing in the app would read a role,
-- and a column nothing reads is a guess about a decision we have not
-- made yet. `users.is_admin` stays what it is, site-wide.
CREATE TABLE band_members (
    band_id  INTEGER NOT NULL REFERENCES bands(id) ON DELETE CASCADE,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (band_id, user_id)
);

-- "Which bands is this user in?" is asked on every request that resolves
-- a band from a URL; the primary key above answers the other direction.
CREATE INDEX idx_band_members_user ON band_members(user_id);

-- The band a user was last in. A preference, not a permission: it
-- decides where to send someone who has not said where they want to go,
-- and nothing is ever scoped by it — except the un-banded API routes
-- that apps already installed still call, which is why it has to be a
-- column and not something the session holds. A bearer token from an old
-- app build has no session to put it in.
--
-- ON DELETE SET NULL, because losing the band you were last in should
-- leave you looking at nothing in particular, not break every request.
ALTER TABLE users ADD COLUMN last_band_id INTEGER REFERENCES bands(id) ON DELETE SET NULL;

-- Clips belong to a band. Nullable only because SQLite cannot add a NOT
-- NULL column without a default and a default band id would be a lie on
-- a second band's instance: every insert sets it, and the backfill below
-- fills every row that exists. A clip that somehow had NULL would appear
-- in no band's list rather than in everyone's, which is the failure
-- direction to prefer.
ALTER TABLE clips ADD COLUMN band_id INTEGER REFERENCES bands(id) ON DELETE RESTRICT;

-- The band everything migrates into. `default` is a working slug and
-- nothing anywhere checks whether it has been renamed — `rename-band` is
-- a command the site owner runs when it suits them, not a step this
-- migration or the deploy waits for.
INSERT INTO bands (slug) VALUES ('default');

UPDATE clips SET band_id = (SELECT id FROM bands WHERE slug = 'default');

INSERT INTO band_members (band_id, user_id)
SELECT (SELECT id FROM bands WHERE slug = 'default'), id FROM users;

UPDATE users SET last_band_id = (SELECT id FROM bands WHERE slug = 'default');

-- A clip name means one recording *within a band*. Two bands can both
-- have a Take 3, and the " (2)" suffixing that resolves a collision on
-- upload now resolves it against the uploading band's clips alone.
DROP INDEX idx_clips_name_unique;
CREATE UNIQUE INDEX idx_clips_band_name_unique ON clips(band_id, name COLLATE NOCASE);

-- Reading one band's clips newest-first is the main list, on both
-- clients, and it is now always filtered by band.
CREATE INDEX idx_clips_band_uploaded_at ON clips(band_id, uploaded_at DESC);

-- Labels have to be rebuilt rather than altered. `name` carries a
-- column-level UNIQUE, which SQLite implements as an automatic index
-- that cannot be dropped, and that index is exactly what would stop two
-- bands both having a label called `verse-1`.
--
-- The two tables that reference a label are rebuilt with it, and dropped
-- *before* it, which is not tidiness but the whole point. Both reference
-- `labels` with ON DELETE CASCADE, and dropping a table performs an
-- implicit DELETE of every row in it — so dropping `labels` while those
-- rows still pointed at it would cascade, and take every label on every
-- clip and the entire wiki history with it. (`defer_foreign_keys` does
-- not help: it defers the *check* that a reference is satisfied, not the
-- cascade that fires on delete.) With the children gone first, dropping
-- `labels` has nothing left to cascade to.
--
-- Every id is preserved, so a link to a label still resolves and the
-- rows copied back into the children still point at the label they were
-- always on.
CREATE TABLE labels_new (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    name    TEXT NOT NULL COLLATE NOCASE,
    band_id INTEGER NOT NULL REFERENCES bands(id) ON DELETE RESTRICT,
    -- A label name means one song within a band, and says nothing about
    -- any other band's labels.
    UNIQUE (band_id, name)
);

INSERT INTO labels_new (id, name, band_id)
SELECT id, name, (SELECT id FROM bands WHERE slug = 'default') FROM labels;

-- Identical to the original, including the sparse `position` the
-- playlist order relies on. A row pairs a clip with a label, and both
-- are in one band — nothing here enforces that they are the same band,
-- because nothing can across two tables in SQLite. What keeps it true is
-- that a label is only ever resolved within the band of the request that
-- names it.
CREATE TABLE clip_labels_new (
    clip_id   INTEGER NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
    label_id  INTEGER NOT NULL REFERENCES labels_new(id) ON DELETE CASCADE,
    added_by  INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    added_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    position  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (clip_id, label_id)
);

INSERT INTO clip_labels_new (clip_id, label_id, added_by, added_at, position)
SELECT clip_id, label_id, added_by, added_at, position FROM clip_labels;

CREATE TABLE label_wiki_revisions_new (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    label_id  INTEGER NOT NULL REFERENCES labels_new(id) ON DELETE CASCADE,
    content   TEXT NOT NULL,
    edited_by INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    edited_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO label_wiki_revisions_new (id, label_id, content, edited_by, edited_at)
SELECT id, label_id, content, edited_by, edited_at FROM label_wiki_revisions;

DROP TABLE clip_labels;
DROP TABLE label_wiki_revisions;
DROP TABLE labels;

-- `labels` first: renaming it rewrites the REFERENCES clauses in the two
-- tables that point at `labels_new`, so they land on the right name
-- without being touched again.
ALTER TABLE labels_new RENAME TO labels;
ALTER TABLE clip_labels_new RENAME TO clip_labels;
ALTER TABLE label_wiki_revisions_new RENAME TO label_wiki_revisions;

-- The indexes the originals carried, recreated on the rebuilt tables.
CREATE INDEX idx_clip_labels_label ON clip_labels(label_id);
CREATE INDEX idx_clip_labels_order ON clip_labels(label_id, position);
CREATE INDEX idx_label_wiki_label ON label_wiki_revisions(label_id, id DESC);

-- Listing and suggesting labels is always within one band.
CREATE INDEX idx_labels_band ON labels(band_id, name COLLATE NOCASE);
