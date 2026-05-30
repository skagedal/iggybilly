-- Each label can have a Markdown "wiki page". We store the full edit
-- history as an append-only list of revisions; the current page is
-- simply the newest revision for a label (MAX(id)). This keeps author
-- and timestamp for every edit, and makes "restore" just another
-- revision that happens to copy older content.
CREATE TABLE label_wiki_revisions (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    label_id  INTEGER NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    content   TEXT NOT NULL,
    edited_by INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    edited_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- Fast "latest revision for a label" and history listing (newest first).
CREATE INDEX idx_label_wiki_label ON label_wiki_revisions(label_id, id DESC);
