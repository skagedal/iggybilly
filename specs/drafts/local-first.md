# Local-first, on the phone and the web

Implements [#24](https://github.com/skagedal/iggybilly/issues/24).
Picks up the "keep a whole playlist downloaded" that
[playlists-from-labels](playlists-from-labels.md) leaves out and points
here.

Today the phone keeps audio on disk and nothing else. A clip that is on
the device plays with no network, but the list it sits in is a request,
the labels are a request, the wiki page is a request, and the playlist
order will be one too. Open the app on a train and you get "Couldn't
reach …" for clips whose bytes are already in your pocket.

The web is further from it still. Every navigation is a round trip for
the page's props, so every click waits on the server, and with no
connection the site is not there at all.

The change is that each client — the phone and the browser — holds a copy
of the band's data — clips, labels, which clip carries which label and in
what order, and each label's wiki page — in a database of its own, renders
every screen from that, and syncs with the server in the background.
Writes made offline are queued and land when there is a connection. The
server stays the one shared truth; neither client is a thin client over
it any more.

Four decisions carry the design, and each is argued where it appears:

1. **A client holds a replica of the server's data, and only sync
   writes it.** Every screen reads the replica. Nothing a user does
   touches it directly.
2. **Sync is incremental, from a change log the server keeps, and the
   log holds one row per entity.** A client asks for everything after
   the last row it saw; the server answers with the current state of
   those entities and tombstones for the deleted ones.
3. **What the user has done and the server has not yet confirmed lives
   in a queue, laid over the replica on every read.** A rejection is a
   dropped queue entry, not an undo.
4. **One protocol and one model, two implementations.** The feed, the
   write routes, the overlay rules and the conflict rules are the same for
   both clients and are specified once. What differs is storage — SQLite
   on the phone, IndexedDB in the browser — and the code, in Dart and in
   TypeScript. The two are held to the same behaviour by one set of test
   cases, written as data, that both test suites run.

The issue points at an account of how far this idea can be taken, and
what it costs: a general sync engine is months of work, with partial
sync, access control and schema migration of cached data each a project
of its own. This design is not general. Four entity kinds, one log, no
partial sync, no per-device state on the server, no merging of text. It
is a few hundred lines on the server and on each client.

The issue also asks whether a framework — Jazz, or something like the
sync engine Linear built — should do this. Not here. What those buy is
generality this app does not need: arbitrary schemas, partial sync,
permissions evaluated on the client, CRDT merges of concurrent edits.
What they cost is the server: each wants to own the data model, and this
one already has a SQLite database, a Rust server and two clients in two
languages that no single framework serves well. The architecture Linear
describes is the one this spec follows — a local replica, a monotonic
change feed, an optimistic transaction queue laid over it — written
directly against four tables rather than generalised.

## Ordering

Simon has decided that this work, together with
[structured-document-storage](structured-document-storage.md), is a
**prerequisite** for clip comments
([#6](https://github.com/skagedal/iggybilly/issues/6), spec
[clip-comments](clip-comments.md)). Comments are the feature people
will write offline, and they are where sync and conflict resolution
first really bite: many small writes from several people against the
same clip, some of them replies to each other. Building comments before
the replica and the queue exist would mean building the foundation
twice. So `clip-comments.md` waits for this, and its "Failure and
offline" section — which says comments are network-only and nothing is
queued — is to be rewritten on top of this design when it is picked up.
Comments are not in this spec; the change log is shaped so that adding
them is one more entity kind and nothing else.

## Functionality

### What is on a client

Everything the list, the clip page, the playlist and the wiki page show:
every clip's name, uploader, dates, duration and waveform peaks; every
label; every label-on-clip with its playlist position; and the current
wiki page of every label that has one. Audio is not part of the replica:
on the phone it stays what it is today, a cache plus the clips you asked
to keep, and in the browser it stays streamed.

That is the whole band's data, not a slice of it. There is no partial
sync and no "recent clips only": a band has hundreds of clips, perhaps a
few thousand, and a thousand clips with their peaks is about three
megabytes. Choosing what to sync would cost more than syncing all of it.

### Opening the app

The first time after signing in, one full-screen "Fetching your clips…"
while the replica is filled. It is the only spinner the app has left.
After that every screen paints from the replica immediately, whether or
not there is a network, and a sync runs behind it.

A sync runs at launch, when the app returns to the foreground, on
pull-to-refresh, after every write, and once a minute while the app is
in front. In the browser, "launch" is loading the site, "foreground" is
the tab becoming visible, and pull-to-refresh is the **Sync now** in the
status popover; reloading the page also syncs, since it is a launch. Nothing is pushed from the server: a band of five does not
need a socket held open, and a minute is quicker than anyone notices.

The app bar — the header, on the web — carries a small cloud icon when
the last sync failed. It is the only sign of being offline. Tapping it shows when the phone last
synced, how many changes are waiting to go up, and a **Sync now**
button. There is no banner for the ordinary case of no signal, because
the ordinary case is that everything works.

### Reading offline

The clip list, its filters, the playlist view, the clip page and the
wiki page all work with no network, from the replica. Playing a clip
that is on disk works exactly as it does now. Playing one that is not
fails as it does now, with the player's "Couldn't reach …" error; there
is nothing to be done about bytes that are not there. In the browser
nothing is on disk, so offline you can read everything and play nothing,
and the player says so the way it does today when the server is down.

The label picker's suggestions come from the replica too, so labelling
offline offers the same names it would online, ordered the same way.

Some screens stay online-only, each for a reason:

- **Wiki history and restore.** History is every revision of every
  page, read once in a while, and restore is a read of it. Replicating
  an append-only history to look at it twice a year is not worth the
  bytes.
- **The account screen.** Devices and passwords are credentials. There
  is nothing to render offline that you could act on.
- **Signing in**, on either client, which is a request by definition.
- **Upload**, on either client. A clip is ten megabytes and a new row the server has not
  numbered. Queueing uploads is a real feature — record in the basement,
  upload when you surface — and is left for later, because it changes
  what [recording-in-app](recording-in-app.md) says about a recording
  that has not been saved.

### Writing offline

The same on both clients. Rename, add a label, remove a label, reorder a
playlist, save a wiki page, delete a clip. Each takes effect on the screen the moment you do
it, exactly as if the server had answered, and is queued. The cloud icon
shows how many are waiting. When a sync gets through, the queue is sent
in the order it was written and the replica is refreshed behind it.

Most queued writes land and nothing more is said. The ones the server
turns down are shown: a line above the clip list, "1 change couldn't be
saved", opening a list with what was tried and the server's own reason
for each — "A clip named “Riff” already exists." — and a **Dismiss** on
each. A rejected wiki edit also has **Open your text**, because that one
is the only kind where the user has written something worth keeping.

A rejected change disappears from the screen when it is rejected: the
rename reverts, the label comes off, the clip reappears. The next sync
already showed what the server actually has.

### Deleting offline

The sharp case, so spelled out. Deleting a clip offline hides it at
once, from the list, the playlist and any queue playing from that
playlist, and stops it if it is playing, as a delete does now. On the
phone the audio stays on disk until the server confirms the delete, so a
delete the server refuses puts the clip back with its bytes. Anything queued
earlier against the same clip — a rename, a label — is dropped from the
queue; there is no point sending a rename ahead of a delete.

A clip that was deleted on the server by someone else while you were
offline is gone from your phone after the next sync, and any writes you
queued against it are dropped without a message. There is nothing to
say about a rename of a clip that no longer exists.

### Conflicts

Two people editing offline and both landing. Each case, decided:

**Two people edit the same wiki page.** A save carries the revision the
edit started from. If the page has moved on since, the server refuses
the save and answers with the page as it now stands. The user is told
that someone else edited the page after they started, sees the page as
it is now, and can open their own text against it and save again, doing
the merge by hand. Nothing is lost on either side: the other person's
revision is in force and yours is in your hands. There is no automatic
merge. [structured-document-storage](structured-document-storage.md)
changes what a page is stored as, not how a save works — a save still
replaces the page whole — so the rule survives it unchanged.

This is stricter than the web is today, which as spec 001 notes lets the
second save win outright. The web adopts the same rule, because it now
queues too: a queued edit can be hours old by the time it lands, on
either client, and an hours-old edit silently overwriting a fresh one is
the case the base revision exists for.

**Two people reorder the same playlist.** A move is "put this clip after
that one", a statement about neighbours rather than positions, which is
what makes it survive being replayed late. Two people moving different
clips both land. Two people moving the same clip: whichever lands last
wins, and nobody is told, which is what happens today when two people
drag within the same minute. A move whose neighbour has since left the
playlist, or whose clip has, cannot be applied and is rejected with the
playlist spec's 400: "Couldn't move “bridge-take-3”: the playlist
changed underneath it."

**Two people rename the same clip.** Last to land wins, as on the web. A
rename into a name that is now taken is a 409 and is shown.

**Someone edits a clip you deleted, or deletes a clip you edited.** The
delete wins. A delete is not something to reconcile with.

**Labels.** Adding a label twice is once; removing a label that is gone
is nothing. Add and remove of the same label from two phones resolve to
whichever landed last. None of these produce a message.

### Keep a label downloaded

Phone only. The browser keeps no audio, and this is a promise about
audio; see "Open questions".

The playlist view — the clip list with exactly one filter — gains a
**Keep downloaded** switch in its heading, beside the count and the
length. On, every clip carrying the label is downloaded and kept, in
playlist order, and so is every clip given the label later, on the next
sync. Off, the clips drop into the automatic cache, where they take
their chances, exactly as a clip whose own switch is turned off does
now. The subtitle says what the switch will cost before it is flipped —
"30 clips · 84 MB" — and what it is doing after: "12 of 30 downloaded",
then "Kept on this phone".

This is the thing the playlist spec deferred, and it deferred it because
keeping the audio is not enough: a set list on a phone in a basement
also needs the list. That problem does not exist once the replica does.
The label's membership, its order, its clips' names and waveforms and
its wiki page are on the phone whether or not the label is kept, so
"keep this label" is only a promise about audio. That is why it is a
small feature here and would have been a large one there.

A clip kept because of a label shows its own **Keep downloaded** switch
on and disabled, with "Kept with set-2026" beneath it. The switch on the
clip is about the clip; the way to stop keeping a set is on the set. A
clip kept both ways stays kept until both are off.

The Downloads screen gains a **Kept labels** section above the kept
clips: each label with "12 of 30 · 84 MB" and a stop button. Kept bytes
count both. The automatic cache and its ceiling are untouched: kept
audio has never counted against it and still does not.

Downloads run while the app is open and continue after the next sync if
they were interrupted. Nothing runs in the background: before the
basement, open the app.

### Signing out

Signing out deletes the replica and the queue along with the token, or
on the web along with the session. If
the queue is not empty the app says so first — "2 changes haven't been
saved to the server yet" — and offers to sync now or sign out anyway.
The audio cache is left alone, as it is today. Being signed out by the
server — the device revoked, a password changed, the session expired —
keeps the replica and the queue: signing back in as the same user on the
same server picks the queue up where it was.

### The web

Everything above holds in the browser, with the phone-only parts named
where they appear. Three things are particular to it.

**The site opens with no network.** Once visited, the site's shell and
its scripts are kept by the browser, so opening it on a train shows the
clip list from the replica rather than the browser's offline page. This
is what makes "open the app" mean the same on both clients. Every URL
that works today still works as a link and on reload.

**Clicks stop waiting.** Moving between the list, a clip and a filter is
drawn from the replica, with no progress bar, because there is no
request. That is the visible difference online, and it is most of the
reason to do this on the web at all.

**Several tabs are one client.** Two tabs of the site share one replica
and one queue. A rename in one tab shows in the other at once, only one
tab syncs at a time, and closing a tab with writes waiting loses
nothing: the next tab, or the next visit, sends them.

## Implementation

### Server: the change log

`migrations/0006_change_log.sql` — number it whatever is next when it
lands. One table, written by triggers.

```sql
-- What has changed, for clients that keep a replica.
--
-- One row per entity, not one per change: a REPLACE on the unique key
-- moves the entity's row to a fresh seq, so the table is bounded by the
-- number of entities that have ever existed and a client only ever
-- sees an entity once per sync however many times it changed. Deleted
-- entities keep a row with deleted = 1, which is how a client learns
-- to drop them; tombstones are never pruned, since a row is a few
-- dozen bytes and a client that missed one has a stale clip forever.
--
-- seq is assigned in commit order. SQLite has one writer at a time and
-- AUTOINCREMENT hands out max + 1 inside that writer's transaction, so
-- no row with a smaller seq can appear after a client has read past it.
-- That is the property a changed-since cursor needs and the reason
-- this is a table rather than an updated_at on every row.
--
-- Written by triggers rather than by each handler, because a change
-- that is not logged is a client that never hears about it, and the
-- one place every write goes through is the table.
CREATE TABLE changes (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 'clip' | 'label' | 'clip_label' | 'wiki'
    kind       TEXT    NOT NULL,
    -- The clip or label id as text, 'clip:label' for a membership.
    entity_key TEXT    NOT NULL,
    deleted    INTEGER NOT NULL DEFAULT 0,
    changed_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (kind, entity_key)
);

CREATE TRIGGER changes_clip_insert AFTER INSERT ON clips BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key) VALUES ('clip', NEW.id);
END;
CREATE TRIGGER changes_clip_update AFTER UPDATE ON clips BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key) VALUES ('clip', NEW.id);
END;
CREATE TRIGGER changes_clip_delete AFTER DELETE ON clips BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key, deleted) VALUES ('clip', OLD.id, 1);
END;

CREATE TRIGGER changes_label_insert AFTER INSERT ON labels BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key) VALUES ('label', NEW.id);
END;
CREATE TRIGGER changes_label_update AFTER UPDATE ON labels BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key) VALUES ('label', NEW.id);
END;
CREATE TRIGGER changes_label_delete AFTER DELETE ON labels BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key, deleted) VALUES ('label', OLD.id, 1);
END;

CREATE TRIGGER changes_clip_label_insert AFTER INSERT ON clip_labels BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key)
    VALUES ('clip_label', NEW.clip_id || ':' || NEW.label_id);
END;
CREATE TRIGGER changes_clip_label_update AFTER UPDATE ON clip_labels BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key)
    VALUES ('clip_label', NEW.clip_id || ':' || NEW.label_id);
END;
CREATE TRIGGER changes_clip_label_delete AFTER DELETE ON clip_labels BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key, deleted)
    VALUES ('clip_label', OLD.clip_id || ':' || OLD.label_id, 1);
END;

-- The wiki entity is the label's current page, not the revision. A
-- revision is only ever inserted, and inserting one changes the page.
CREATE TRIGGER changes_wiki_insert AFTER INSERT ON label_wiki_revisions BEGIN
    INSERT OR REPLACE INTO changes (kind, entity_key) VALUES ('wiki', NEW.label_id);
END;

-- Everything that already exists is a change a new client needs to
-- hear about. Labels first, then clips, then memberships, so a client
-- paging through from zero meets a membership's ends before the
-- membership.
INSERT INTO changes (kind, entity_key) SELECT 'label', id FROM labels ORDER BY id;
INSERT INTO changes (kind, entity_key) SELECT 'clip', id FROM clips ORDER BY id;
INSERT INTO changes (kind, entity_key)
    SELECT 'clip_label', clip_id || ':' || label_id FROM clip_labels ORDER BY clip_id, label_id;
INSERT INTO changes (kind, entity_key)
    SELECT 'wiki', label_id FROM label_wiki_revisions GROUP BY label_id ORDER BY label_id;
```

Three things checked against SQLite rather than assumed. `INSERT OR
REPLACE` deletes the conflicting row and inserts a new one, so the
entity does take a fresh `seq`. An integer bound into a `TEXT` column
lands as text, so `'12'` and `12` are the same key. And the cascade that
removes a clip's `clip_labels` rows when the clip is deleted does fire
the membership trigger, so a deleted clip's memberships get tombstones
of their own, a moment before the clip's; the client would drop them
anyway and does, so it is harmless either way.

The users table is not logged. A clip carries its uploader's name
denormalised, and there is no way to rename a user.

Renumbering a playlist — the playlist spec's fallback when a gap is used
up — touches every membership of the label and so logs every one. That
is a few hundred small rows on a rare event, and is why memberships are
their own kind rather than a list inside the clip: a clip's row carries
its peaks, and re-sending a hundred waveforms because a set list was
renumbered would be the wrong shape.

### The sync route

    GET /api/v1/sync?since=0&limit=500
    GET /api/sync?since=0&limit=500

The same handler on both surfaces: bearer token on the phone's, session
cookie on the web's. The answer does not depend on who asks — every
member of the band sees every clip — which is what lets one feed serve
both and lets the answer carry `uploadedBy` rather than a per-user
`canDelete`.

`since` is the last `seq` the client applied, zero on a fresh replica.
`limit` defaults to 500 and is capped at 1000. The answer:

```jsonc
{
  "cursor": 1842,        // the last seq included; the next request's since
  "hasMore": false,      // true when the log had more than limit rows after since
  "labels": [{ "id": 4, "name": "verse-1" }],
  "clips": [{
    "id": 12,
    "name": "Riff",
    "originalFilename": "riff.m4a",
    "contentType": "audio/mp4",
    "sizeBytes": 812331,             // new on this surface; the keep-label switch prices itself with it
    "uploadedAt": "2026-05-22T19:40:08.000Z",
    "recordingDate": "2026-05-22",
    "uploadedBy": 3,                 // an id, not canDelete: the phone knows who it is
    "uploader": "simon",
    "peaks": [0.12, 0.31, /* … */],
    "durationSeconds": 14.2,
    "audioUrl": "/clips/12/audio",
    "downloadUrl": "/clips/12/audio?download=1"
  }],
  "clipLabels": [{ "clipId": 12, "labelId": 4, "position": 2048, "addedAt": "2026-05-22T19:41:00.000Z" }],
  "wikis": [{
    "labelId": 4,
    "revisionId": 31,                // what a save from this page must cite as its base
    "content": "…",                  // Markdown source, for the editor
    "lastEditedBy": "anna",
    "lastEditedAt": "2026-06-01T10:02:11.000Z"
  }],
  "deleted": {
    "clips": [7],
    "labels": [],
    "clipLabels": [{ "clipId": 12, "labelId": 9 }]
  }
}
```

The log rows are read and the live rows joined to them inside one read
transaction, so a page describes one moment. Rows after `since` are
taken in `seq` order up to `limit`; every entity is then loaded from its
live table by the ids in the page, and any entity whose log row says
`deleted` goes under `deleted` instead. A page is applied by the client
as one local transaction, so a page boundary is never visible.

`position` is the playlist spec's column and is present once that has
landed; a client that does not find it orders a playlist newest-first,
which is what the playlist spec's backfill produces anyway. When
[structured-document-storage](structured-document-storage.md) lands,
`wikis` carries `document` beside `content`, exactly as
`/api/v1/labels/{id}/wiki` will.

There is no bootstrap route. `since=0` pages through the log from the
beginning, and the backfill above is what makes that the whole dataset.
A thousand clips with their labels is a handful of pages.

### Wiki saves cite their base

`POST /api/v1/labels/{id}/wiki` and `POST /api/labels/{id}/wiki` gain
an optional `baseRevisionId`:

```json
{ "content": "…", "baseRevisionId": 31 }
```

When present, the save goes through only if the label's newest revision
is that one (`null` meaning "no page yet"). Otherwise the answer is
`409` with the page as it now stands:

```json
{ "error": "This page was edited by anna after you started.", "page": { "labelId": 4, "revisionId": 34, "content": "…", "hasContent": true, "lastEditedBy": "anna", "lastEditedAt": "…" } }
```

`GET /api/v1/labels/{id}/wiki` and the save's own answer, on both
surfaces, gain `revisionId`. The base stays optional on the server so an
old phone build that sends none keeps working; both new clients always
send it.

### Rust

- `migrations/0006_change_log.sql` — as above.
- `src/queries/sync.rs`, new — `page(pool, since, limit) -> SyncPage`,
  the read transaction and the four loads. `SyncPage` holds domain
  types; the wire shape is the api module's.
- `src/queries/wiki.rs` — `Page` and `Revision` gain `revision_id`;
  `save` gains a `base: Option<Option<i64>>` and returns a `SaveOutcome`
  of `Saved` or `Conflict(Page)`. The check and the insert are one
  transaction: `SELECT MAX(id)` for the label, compare, insert. A save
  without a base passes `None` and cannot conflict.
- `src/api/sync.rs`, new — the route, and the `Clip`, `ClipLabel` and
  `Wiki` wire structs. `queries::clips::Clip` gains `size_bytes`
  through `CLIP_COLUMNS`, and `api::clips::Clip` gains `size_bytes` and
  `uploaded_by` so the two shapes are one struct with `can_delete`
  computed on the way out.
- `src/api/labels.rs` — `baseRevisionId` on `save_wiki`, `revisionId` on
  `WikiPage`.
- `src/api/mod.rs` — `.route("/sync", get(sync::page))`.
- `src/handlers/sync.rs`, new, and `src/web.rs` — the same route on the
  session-authenticated `/api`, answering with the same wire structs.
  `src/handlers/labels.rs` gains `baseRevisionId` as the api module does.
- `src/handlers/mod.rs` — the page routes for replica-backed pages stop
  loading their data. See "The web: routing".
- `tests/api_v1.rs` — the backfill produces one row per entity and a
  fresh client paging from zero sees all of it; an insert, an update and
  a delete each move the entity to a new `seq`; a deleted clip arrives
  under `deleted` and never under `clips`; a renumber logs every
  membership; a save with a stale base is a 409 carrying the current
  page and a save with the right base lands. `tests/web.rs` gains the
  sync route under a session.

### The shared cases

`testdata/sync/`, new, holds the behaviour both clients must agree on,
as JSON the Dart and the TypeScript tests both read:

- `overlay/*.json` — a snapshot, a queue, and the snapshot the screens
  must see after the overlay. One file per kind, plus the delete
  collapse, the rename collapse, a provisional label and two moves of the
  same clip.
- `push/*.json` — a queued write, a server status and body, and what
  becomes of the row: pushed, dropped, rejected with this reason, or left
  for the next sync. One per row of the push table and per rejection
  class.
- `apply/*.json` — a replica, a sync page, and the replica after.

The phone's and the web's own tests are then about their storage and
their screens. The rules are in one place, and a change to them fails
both suites until both are changed.

### The phone: the database

`package:sqlite3` with `sqlite3_flutter_libs` for the platform
binaries. Not `sqflite`, which is a platform channel and cannot answer
under `flutter test`, and not `drift`, which is a code generator and an
ORM for what is five tables and twenty statements. `sqlite3` is dart:ffi,
opens a file on the host under `flutter test`, and is synchronous, which
is what the player wants when it asks the store a question between
frames. The file's directory comes in as a `Future<Directory> Function()`
the way the cache's does.

One file per server and user, named from the server's host and port and
the user's id, in the app support directory beside the cache. The token
is for one server and the replica is for one server, and a user who
signs into two instances gets two replicas rather than a merged mess.
The queue lives in the same file as the replica so that the two commit
together.

```sql
PRAGMA user_version = 1;

-- Bookkeeping: 'cursor', 'last_synced_at', 'last_error'.
CREATE TABLE sync_state (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- The replica. Written only by SyncEngine.apply, in one transaction per
-- page, together with the cursor. The shape follows the server's tables
-- because the screens read the same things the server's handlers do
-- and there is no gain in renaming columns on the way. No foreign keys:
-- a membership can arrive a page before its clip when a client is
-- paging from zero and the clip was renamed after it was labelled, and
-- the reads join, so an orphan is invisible rather than an error.
CREATE TABLE clips (
    id                INTEGER PRIMARY KEY,
    name              TEXT    NOT NULL,
    original_filename TEXT    NOT NULL,
    content_type      TEXT    NOT NULL,
    size_bytes        INTEGER NOT NULL,
    uploaded_at       TEXT    NOT NULL,
    recording_date    TEXT,
    uploaded_by       INTEGER NOT NULL,
    uploader          TEXT    NOT NULL,
    peaks             TEXT,             -- JSON, as the server holds it
    duration_seconds  REAL,
    audio_url         TEXT    NOT NULL,
    download_url      TEXT    NOT NULL
);

CREATE TABLE labels (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE clip_labels (
    clip_id  INTEGER NOT NULL,
    label_id INTEGER NOT NULL,
    position INTEGER NOT NULL DEFAULT 0,
    added_at TEXT    NOT NULL,
    PRIMARY KEY (clip_id, label_id)
);

-- The current page per label. No row means no page yet.
CREATE TABLE wiki_pages (
    label_id       INTEGER PRIMARY KEY,
    revision_id    INTEGER NOT NULL,
    content        TEXT    NOT NULL,
    last_edited_by TEXT    NOT NULL,
    last_edited_at TEXT    NOT NULL
);

-- What the user has done and the server has not yet confirmed. Laid
-- over the replica on every read; pushed in id order; a row survives
-- until the pull that follows its push has been applied.
CREATE TABLE pending_writes (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT    NOT NULL,
    payload    TEXT    NOT NULL,        -- JSON, per kind
    created_at TEXT    NOT NULL,
    -- Set the moment the server accepts it. A pushed row is still
    -- overlaid, so nothing flickers between the push and the pull, and
    -- is never pushed again, so a pull that fails costs nothing.
    pushed     INTEGER NOT NULL DEFAULT 0
);

-- What the server turned down, until the user dismisses it.
CREATE TABLE rejected_writes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT NOT NULL,
    payload     TEXT NOT NULL,
    reason      TEXT NOT NULL,          -- the server's own message
    rejected_at TEXT NOT NULL
);

-- The user's promises about audio. These used to be a flag in the
-- cache index; they move here because a label's promise has to be
-- resolved against the replica, and the cache has no replica.
CREATE TABLE kept_clips  (clip_id  INTEGER PRIMARY KEY, kept_at TEXT NOT NULL);
CREATE TABLE kept_labels (label_id INTEGER PRIMARY KEY, kept_at TEXT NOT NULL);
```

The replica tables are disposable and the rest are not. A change to the
replica's shape bumps `user_version`, drops the four replica tables,
recreates them and sets the cursor to zero, and the next sync refills
them; the queue, the rejections and the kept sets are migrated forward
like any schema. That is the answer to "schema migrations on cached
data": the cache is re-fetched and only the phone's own state is
migrated.

### The store

`mobile/lib/src/local/store.dart`, `LocalStore`, a `ChangeNotifier` and
the only thing the screens read. The web's store, below, has the same
methods and the same overlay; the description here is of both.

It reads the four replica tables whole into memory — a `Snapshot` of
clips, labels, memberships and pages — after every applied page and at
open, and lays the queue over it. At this app's scale that is a few
thousand rows and a few megabytes of peaks, and it means every question
a screen asks is answered synchronously from memory with no loading
state. SQLite is here for the apply to be a transaction and for the
cursor, the queue and the data to commit together, not for queries. If
a band ever has enough clips for this to matter, the store's methods
have the same signatures whether they answer from memory or from a
query, and that is the change to make then.

The overlay is applied in queue order to a copy of the snapshot, with
each kind's local effect:

| Kind | Payload | Local effect |
| --- | --- | --- |
| `renameClip` | `clipId, name` | the clip's name |
| `addLabel` | `clipId, name` | a membership at the end of the label's order; a label that does not exist yet gets a provisional row with a negative id, `-(pending write id)`, so it is stable across rebuilds |
| `removeLabel` | `clipId, labelId` | the membership is gone |
| `reorder` | `labelId, clipId, afterClipId` | the clip is moved to after `afterClipId` in the label's order, or to the front when null |
| `saveWiki` | `labelId, content, baseRevisionId` | the page's content, edited by the signed-in user, now |
| `deleteClip` | `clipId` | the clip and its memberships are gone |

The screens' methods on the store are the questions they already ask
the API, answered from the overlaid snapshot: `clips(filters)` with the
same AND semantics and newest-first order the server has,
`playlist(labelId)` in `(position, clipId)` order with the overlay's
moves applied, `clip(id)`, `labelsOf(clipId)`, `wiki(labelId)`,
`suggestLabels(query, excludeClip)` with the server's rule of recent
labels for an empty query and substring matches otherwise, `canDelete`
as `uploadedBy == session.user.id`, and the playlist heading's count and
total length.

Enqueueing is also here: `rename`, `addLabel`, `removeLabel`, `reorder`,
`saveWiki`, `deleteClip` each insert a `pending_writes` row, rebuild the
overlay, notify, and ask the engine to sync. Two collapses on the way
in: a `deleteClip` removes every earlier unpushed write against that
clip, and a `renameClip` replaces an earlier unpushed rename of the same
clip. Everything else stays in order — two moves of the same clip are
both replayed, because the second was dragged against a list that
included the first.

A provisional label — one added offline whose name the server has not
seen — is a chip and a filter and nothing more until it syncs: its wiki
page says "This label hasn't reached the server yet", and its playlist
view has no drag handles and no keep switch, because both need an id the
server issued. It lasts until the next successful sync.

### The sync engine

`mobile/lib/src/local/sync_engine.dart`, `SyncEngine`, and its web
counterpart below. One `sync()`
method, serialised: a call while one is running waits for it and then
runs once more, so a burst of writes is one push. The loop is push, then
pull, then clean up.

**Push.** Unpushed rows in id order, one request each, mapped onto the
routes the app already calls:

| Kind | Request | If the response is lost |
| --- | --- | --- |
| `renameClip` | `POST /api/v1/clips/{id}/name` | sent again; renaming to the same name is a no-op update |
| `addLabel` | `POST /api/v1/clips/{id}/labels` | sent again; the server's insert is `OR IGNORE` |
| `removeLabel` | `DELETE /api/v1/clips/{id}/labels/{labelId}` | sent again; deleting nothing is nothing |
| `reorder` | `POST /api/v1/labels/{id}/order` | sent again; the same neighbour gives the same order |
| `saveWiki` | `POST /api/v1/labels/{id}/wiki` with `baseRevisionId` | sent again and answered 409; if the current page's content is what we sent, it landed |
| `deleteClip` | `DELETE /api/v1/clips/{id}` | sent again and answered 404, which is done |

The right-hand column is the case where the server applied a write and
the phone never heard: a tunnel entrance between the request and the
response. Every kind is either idempotent or has an answer the engine
recognises as "already done", which is why there is no client-side write
id for the server to remember. The responses' bodies are otherwise
ignored: the pull that follows is the one source of what the server made
of the write.

A success marks the row `pushed`. A rejection is decided by status:

- **404**: the thing is gone. The row is dropped silently; the pull
  will bring the tombstone.
- **400, 403, 409**: the row moves to `rejected_writes` with the
  server's message, except the wiki case above. The overlay is rebuilt
  without it, which is what puts the screen back.
- **401**: `Session.handleUnauthorized`, as today, or on the web the
  redirect to `/login` the router already does. The replica and queue
  stay; the queue resumes after the next sign-in as this user.
- **5xx, or no response**: the push stops here, the rows stay unpushed,
  and the pull is skipped, since a server that cannot take a write
  cannot answer a pull either. The next trigger tries again. There is no
  backoff, because syncs are already only triggered by events and a
  one-minute timer.

**Pull.** `GET /api/v1/sync?since=<cursor>` until `hasMore` is false.
Each page is applied in one transaction: upsert labels, clips,
memberships and pages; delete the tombstoned clips and their
memberships, the tombstoned labels and their pages, and the tombstoned
memberships; write the cursor. On the last page the same transaction
deletes every `pushed` row of the queue — the server assigns `seq` in
commit order and our writes committed before our pull read, so the pull
that succeeded has already carried their effects into the replica and
the overlay no longer needs them. A pull that fails leaves the pushed
rows where they are, still overlaid and never re-sent.

After the last page the store rebuilds its snapshot, notifies, and the
keep policy runs. The player's queue, which the playlist spec makes a
live read of the label's list, re-reads from the store like everything
else.

**Triggers.** `IggybillyApp` calls `sync()` at start, on
`AppLifecycleState.resumed`, from pull-to-refresh, from the cloud icon's
button, and from a one-minute `Timer.periodic` that runs only while
resumed. The store calls it after every enqueue.

**The first sync.** With a cursor of zero and an empty replica, the app
shows "Fetching your clips…" full-screen until the first pull finishes,
and an error with retry if it does not. It is the only time the replica
is allowed to be empty and the screens are not shown.

### Keep-downloaded

`mobile/lib/src/cache/keep_policy.dart`, `KeepPolicy`. It owns
`kept_clips` and `kept_labels` and turns them into instructions for the
`TrackCache`, which keeps its `kept` flag per entry as the mechanism and
loses the right to decide it.

After every snapshot rebuild and every change to the kept sets, the
policy computes the wanted set: the individually kept clips plus every
clip carrying a kept label, each resolved through the overlaid snapshot
so that a label added offline counts. Then it reconciles: every wanted
clip not on disk, or on disk and not flagged kept, goes through
`cache.setKept(clip, url, true)`, one at a time, in playlist order per
label; every flagged clip no longer wanted goes through `stopKeeping`,
which drops it into the automatic cache rather than deleting it, as
today. A download that fails is left for the next reconcile, which is
the next sync. A reconcile is only started after a successful pull or a
change to the kept sets, so a phone with no signal is not asked to fetch
thirty clips once a minute.

The cache's index keeps its `kept` field for the entries it holds. On
first open after this ships, every entry flagged kept becomes a
`kept_clips` row, once; from then on the policy is the source and the
flag follows it.

`KeepDownloadedTile` asks the policy rather than the cache: on and
enabled when the clip is in `kept_clips`, on and disabled with "Kept
with ⟨label⟩" when it is wanted only through a label, off otherwise.
The playlist heading's switch writes `kept_labels`; its subtitle is the
sum of the label's `size_bytes` before, the policy's progress during,
and "Kept on this phone" after. `StoragePage` gets the kept-labels
section from the policy, with each label's downloaded count and bytes,
and `keptBytes` comes to mean everything flagged kept on disk, which it
already does.

### Screens

Every screen that today has a `_load()` and a loading state reads the
store instead and rebuilds when it notifies. Concretely:

- `mobile/lib/src/ui/clips_page.dart` — `ListenableBuilder` over the
  store; `_load` goes; pull-to-refresh calls `sync()`; the wiki cards
  come from `store.wiki` for each active filter; the app bar gains the
  cloud icon and its sheet; the rejected-writes line above the list; the
  playlist heading's keep switch.
- `mobile/lib/src/ui/clip_page.dart` — reads `store.clip(id)`; rename,
  label and delete go through the store's enqueue methods and return at
  once. The `changed` result the list page waits on is no longer needed,
  since the list listens.
- `mobile/lib/src/ui/wiki_page.dart` — reads `store.wiki(labelId)`;
  save enqueues with the page's `revisionId` as base. A rejection for
  this label is shown above the page with **Open your text**, which
  opens the editor prefilled from the rejected payload against the
  page's current revision, and removes the rejection. History and
  restore keep calling the API and keep their error state.
- `mobile/lib/src/ui/label_picker.dart` — `store.suggestLabels` instead
  of `api.suggestLabels`; the spinner goes.
- `mobile/lib/src/ui/keep_downloaded.dart`, `storage_page.dart` — as
  under "Keep-downloaded".
- `mobile/lib/src/ui/rejected_writes_page.dart`, new — the list with
  reasons and dismiss.
- `mobile/lib/src/ui/app.dart` — the first-sync screen, the lifecycle
  observer and the timer; `AppScope` gains `store`, `sync` and `keep`.
- `mobile/lib/src/player/player_controller.dart` — `playQueue` re-reads
  the label's list from the store when the store notifies, which is the
  playlist spec's "the queue follows the playlist" with the store as the
  thing to follow.
- `mobile/lib/src/api/client.dart` — `sync(since, limit)` and
  `baseRevisionId` on `saveWiki`; `mobile/lib/src/api/models.dart` —
  `SyncPage`, `ClipLabel`, `sizeBytes` and `uploadedBy` on `Clip`,
  `revisionId` on `WikiPage`.
- `mobile/lib/src/local/database.dart` — opening, the per-user file
  name, `user_version` and the migrations.
- `mobile/lib/main.dart`, `mobile/pubspec.yaml` — wiring and the two
  packages.

Tests, in `mobile/test/`: `shared_cases_test.dart` runs
`testdata/sync/` against the store and the engine; `local_store_test.dart`
for what is not in the cases — a filter that a queued label add makes
true, the snapshot rebuilding on notify; `sync_engine_test.dart` against
`FakeHttpClient` for push-then-pull, a pull that fails after a push, and
a two-page pull applied as two transactions; `keep_policy_test.dart` for a label's
membership growing and shrinking and the cache flags following;
`app_test.dart`'s scripted server gains `/api/v1/sync` and the flows are
re-scripted through it, plus one that edits a wiki page, is answered
409, and opens its text.

### The web: storage

IndexedDB, used directly, one database per user, `iggybilly-<userId>`.
The origin already separates servers. No wrapper library: the calls
needed are open with an upgrade, a readwrite transaction over several
stores, `getAll` and `put`, and a thirty-line promise helper covers them.

Not SQLite compiled to WebAssembly, though it would let the phone's
schema and statements be reused verbatim. The store reads everything
into memory and answers from there, on both clients, so SQL is never
what answers a screen; the database is there for a transactional apply
and for the cursor, the queue and the data to commit together, and
IndexedDB transactions give exactly that. Against it, the WebAssembly
build is a megabyte on first load and needs OPFS, whose synchronous
access is only available in a worker, for a query language nothing uses.

The object stores mirror the phone's tables one for one — `syncState`,
`clips`, `labels`, `clipLabels` keyed by `[clipId, labelId]`,
`wikiPages`, `pendingWrites` with an auto-increment key, and
`rejectedWrites` — with the same columns in camelCase, as the feed spells
them. There are no kept sets. The database version plays the part of
`user_version`: a replica shape change clears the four replica stores
and the cursor in `onupgradeneeded`, and the queue is migrated.

Browsers may evict IndexedDB under storage pressure. The app asks for
`navigator.storage.persist()` after the first sync; if it is refused or
the data is evicted anyway, the replica is refilled from zero on the next
visit, and only an unsent queue is lost — which is the case the signing
out prompt already exists for, and a risk worth stating in the status
popover when persistence was refused.

### The web: store, engine and tabs

`web/src/local/store.ts`, `LocalStore`, and `web/src/local/sync.ts`,
`SyncEngine`, are the phone's two classes in TypeScript: the same
snapshot in memory, the same overlay, the same push table, the same
apply. The store exposes `subscribe` and `getSnapshot` for React's
`useSyncExternalStore`, so a page reads it with a hook, `useStore()`,
and re-renders when it changes.

Tabs are coordinated with two browser primitives and no server help.
`navigator.locks.request("iggybilly-sync", …)` serialises `sync()`
across every tab, so two tabs never push the same row. A
`BroadcastChannel("iggybilly")` carries one message, "changed", posted
after every apply and every enqueue; a tab that hears it re-reads the
snapshot from IndexedDB and notifies. Each tab runs its own one-minute
timer only while visible, and the lock makes the extra ones cheap.

The wiki page renders from the replica, so the browser needs to render a
page itself. Before
[structured-document-storage](structured-document-storage.md) lands that
means Markdown in the browser, which is the disagreement that spec
exists to end; so this depends on it, and the web renders the stored
`document` with `web/src/document.tsx`, a switch over node types
producing React elements, as the phone does with widgets. `content_html`
stops being sent to the web.

### The web: routing

Today the router holds no route table: it fetches each URL's
`{entry, title, props}` from the server. That cannot work offline, and
online it is the round trip this is meant to remove. So the client gains
a route table after all, for the pages that read the replica:

| Path | Entry | Params |
| --- | --- | --- |
| `/` with `?label=` filters | `index` | the filters |
| `/clips/{id}` | `clip` | `id` |

The router matches these first and renders from the store with no
request. Every other path — the wiki history, the account page, login —
keeps the envelope fetch exactly as now, and so keeps working online and
failing offline as it does today.

The server keeps serving the shell at every URL, so links and reloads
are unchanged. For the two replica-backed pages it stops loading data:
their envelope carries only `username` and the entry, and the page reads
the rest from the store. This is a reversal of the router's stated
design, and its comment is rewritten to say so and why: the URLs stay
the server's, the data does not.

`web/src/sw.ts`, new, a service worker built by `build.mjs` to
`static/sw.js` and served from the root so its scope is the whole site.
It does two things. Hashed assets under `/static/dist/` are served cache
first, since a hashed name never changes. Navigations are served network
first, falling back to a cached copy of the shell when offline; the
cached shell's embedded envelope is ignored for replica-backed paths,
which is what the router does anyway. Nothing under `/api` and nothing
audio is cached. A new deploy is picked up on the next navigation that
reaches the network, and the worker is registered from `main.tsx` only
in production builds, so `local/run` is not haunted by a stale one.

### The web: screens

- `web/src/pages/index.tsx` — clips, filters and wiki panels from
  `useStore()`; the uploader stays online-only and posts as now, then
  syncs.
- `web/src/pages/clip.tsx` — reads the clip from the store; rename,
  labels and delete enqueue.
- `web/src/components/WikiPanel.tsx` — renders the document; save
  enqueues with its base; a rejection shows the conflict with
  **Open your text**.
- `web/src/components/LabelInput.tsx` — suggestions from the store.
- `web/src/components/Layout.tsx` — the cloud icon, its popover, and the
  rejected-writes line.
- `web/src/player.tsx` — a playlist queue follows the store, as on the
  phone.
- `web/src/api.ts`, `web/src/types.ts` — `sync`, `baseRevisionId`, and
  the feed's types.
- `web/src/main.tsx` — opens the store, the first-sync screen, the
  worker registration.

Tests: the web has none today. `web/src/local/*.test.ts` run under
`node --test`, which in Node 24 runs TypeScript directly, with
`fake-indexeddb` as the one new development dependency. They run
`testdata/sync/` against the store and the engine, plus the web's own:
an upgrade that clears the replica and keeps the queue, and two stores
over one database seeing each other's writes. `pnpm run check` gains
`test`, so CI runs them where it already runs the lint.

### Documentation

The root `README.md`'s lines under "What it does" gain the replica and
offline writes for both clients, a short "Sync" section describes the
feed and the queue once for both, and `mobile/README.md` gains a "The replica"
section after "Clips on disk" saying what is on the phone, what stays
online, and how the queue and the overlay work; its "How it is put
together" table gains `LocalStore` and `SyncEngine` over a `Database`
they are handed, and `KeepPolicy` over the cache.

### Staging

Five slices, each shippable, in this order.

1. **The feed.** The change log and the sync route on both surfaces, and
   `testdata/sync/apply`. Nothing reads it yet; it can land and be looked
   at with `curl`.
2. **The phone's replica.** The local database, the store without a
   queue, and every screen reading from it. Writes still go straight to
   the server and fail offline as they do now, followed by a pull. The
   app opens on a train. Most of the phone's code and all of its risk.
3. **The web's replica.** IndexedDB, the store, tabs, the client routes
   and the service worker. Writes as in 2. Requires
   structured-document-storage. Clicks stop waiting.
4. **Keep a label.** `kept_labels`, the policy, the heading switch and
   the storage section. Phone only, small on top of the replica, and the
   thing people asked for.
5. **The queue, on both.** Offline writes, the overlay, rejections, the
   wiki's base revision on the server, and the rest of `testdata/sync/`.
   Both clients in one slice, so the rules never differ in production.
   Then comments.

## Open questions

- **A minute of polling, and no push.** Right for five people; wrong if
  the band grows or the server ever runs somewhere that bills per
  request. Server-sent events on the same route would be the change, and
  nothing about the log would move.
- **No background sync.** iOS's background refresh and Android's
  WorkManager would let the phone catch up before you open it, and let
  a kept set list download overnight. Both are plugins with platform
  setup and both are unreliable in ways that need explaining to users.
  Left out until "I opened the app in the basement and it hadn't
  synced" happens to someone.
- **The whole replica in memory.** Chosen because it makes every screen
  synchronous. It stops being right somewhere in the tens of thousands
  of clips, which is not this band.
- **A wiki conflict is a manual merge.** The right answer for prose from
  two people on two phones, and the annoying one. A structured document
  might one day allow a block-level merge; that is not this spec and is
  not the next one.
- **Provisional labels are second-class** until they sync. The
  alternative is a client-issued id the server honours, which is a
  bigger contract for a case that lasts until the next signal.
- **Tombstones are kept forever.** A `changes` row is tiny and a client
  with a very old cursor still gets the right answer. If the table ever
  looks large, the fix is to prune tombstones older than some months and
  have a client whose cursor predates the pruning start from zero, which
  the client already knows how to do.
- **Discord posts land when the write lands.** A wiki edit made in the
  basement is announced when the phone surfaces, with that time. The
  message could say "edited earlier"; it does not seem worth a branch.
- **Two implementations of the same client.** The rules are shared as
  data, not as code, so the Dart and the TypeScript can still drift in
  anything the cases do not cover. The alternative — one client core
  compiled to both, in Rust or in Dart-to-JavaScript — is a build system
  larger than the code it would share.
- **No audio offline in the browser.** The Cache API could hold clips
  and a service worker could serve them, which would bring keep-a-label to
  the web. Browsers evict it under pressure and cap it per site, so it
  would be a promise the browser can break silently. Left out until
  someone wants to rehearse from a laptop without signal.
- **The client route table.** Two routes duplicated between the server
  and the router. If a page is added that reads the replica, it is a line
  in each, and forgetting the client's line only costs that page its
  offline mode.
