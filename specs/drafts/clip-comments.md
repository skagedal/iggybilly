# Comments anchored to a point in a clip

Implements [#6](https://github.com/skagedal/iggybilly/issues/6).

The thing worth saying about a clip is almost always about a moment in
it: the entry is late, that chord is wrong, do that again. Saying it in
Discord means saying *when* in words, and the person reading has to go
find it. So a comment here carries the moment with it — you mark a point
in the clip, write what you think, and anyone can reply in that thread.

Every comment is also posted to Discord, because that is where the band
already talks and a comment nobody sees is not worth writing.

Comments resemble wiki pages in shape — user-authored text hanging off
something else, with an author and a time — and borrow their storage
decision wholesale: Markdown source in the column, rendered on read by
each surface. See [the wiki spec](../implemented/label-wiki.md), which
sets out why. The one place they differ is versioning: a wiki page keeps
every revision and a comment does not.

## Functionality

### Where comments live

On the clip's own page, under the waveform, on both clients. Not on the
list rows: a list of thirty clips with their discussions inlined is not a
list any more. A row does say how many comments a clip has, so you can
see there is something to read without opening it.

### The anchor

A thread is anchored to a point in the clip measured in seconds, stored
as a fractional number rather than a whole one. Whole seconds would be
too coarse for what this is for: the clips are often a bar or two long,
and "the entry is late" is a remark about a moment, not about a second.
Anchors keep millisecond resolution, which is finer than anyone can click
and finer than the nudge buttons move. The anchor is set when the thread
is created and never moves.

Clicking the waveform still seeks — that is what a waveform is for, and
taking it over for commenting would be a bad trade. Instead the big
waveform on the clip page gets a **comment strip** directly beneath it,
the same width, carrying a pin for every thread at its point in the clip.
The strip is the commenting surface:

- Clicking the strip sets a pending anchor there and opens the composer.
- The **Comment at 1:04** button next to the player controls does the
  same at the current playhead, which is where you are listening.
- While the composer is open, the pending anchor shows as an outlined pin
  and can be moved by clicking the strip again, or nudged by the two
  arrow buttons in the composer (±0.5 s). The label on the composer reads
  the anchor back as `m:ss`, so what you are about to attach it to is
  never a guess.

Clicking a pin scrolls its thread into view and highlights it for a
moment. Pins that fall on nearly the same pixel simply overlap; there is
no clustering, and the thread list below is the real interface.

A few clips have no stored duration — `duration_seconds` is nullable and
is null for anything symphonia could not decode. They do display, and
already handle this: the player falls back to the duration wavesurfer
reports once it has fetched and decoded the file, which is what
`decodedDuration` in `web/src/player.tsx` is for. The strip uses that
same number, so on those clips it appears when the file has loaded rather
than on first paint. Until then the thread list below is fully usable and
the **Comment at …** button works, because the playhead is known even
when the total is not.

### The thread list

Below the strip, every thread on the clip, **ordered by its anchor**,
earliest point in the clip first; two threads at the same point are
ordered by age, oldest first. The clip is the spine, so the discussion
reads along it rather than by recency. Within a thread, comments are in
the order they were written.

A thread renders as:

- Its timestamp, `m:ss`, as a button. Pressing it loads the clip into the
  player if it is not already there, seeks to the anchor and plays. That
  is the payoff of the whole feature and it is one press from anywhere in
  the list.
- Its comments, oldest first, each one the same: author, relative time on
  the phone and `YYYY-MM-DD HH:MM` on the web (each client's existing
  convention), and the body. The opening comment is laid out exactly like
  the rest — same alignment, no indent, nothing marking it as the first.
  A thread is a list of remarks about one moment, and indenting replies
  under the opener would draw a hierarchy that is not there. Replies do
  not nest either: a reply to a reply is a reply to the thread.
- A **Reply** control at the foot of the thread, which expands into a
  text field with **Post** and **Cancel**.

A thread of more than **five** comments is collapsed to the opening
comment, a **Show 9 more** control, and the last two. Those are the ends
that matter: the first says what the thread is about and the last are
what is being said now. Expanding is local to the visit and not
remembered.

The collapse counts tombstones but never leaves one as a visible end. If
the last comment is a deleted one, the collapse takes in the comment
before it, so what you see is something someone actually wrote.

### Writing, editing, deleting

Any signed-in user may comment on any clip and reply in any thread.

Comments are **Markdown**, stored as source and rendered on read. That is
what the wiki already does, and the reasoning carries over unchanged: see
[the wiki spec](../implemented/label-wiki.md). The stored form is the
text the author typed; the HTML the web serves and the widget tree the
phone builds are both derived from it, by each surface, at read time.

That is the structural form worth keeping. It survives a change of
renderer, it is what an edit reopens, and it is what a diff is taken of.
A syntax tree on disk would be comrak's node types written into the
schema, which is the same idea done worse — the parse is cheap and the
commitment is not.

Rendering goes through the same `src/markdown.rs` the wiki uses, so
comments inherit its safety properties rather than growing a second
pipeline: raw HTML escaped, dangerous URL schemes stripped, `[[label]]`
retargeted to the label's filtered view. The enabled feature set is
narrowed to what a remark needs — emphasis, code, links, lists,
blockquotes — with headings, tables and images left out, since a heading
inside a remark about four seconds of audio is not a thing anyone means.

The composer is a plain text field: no toolbar, no live preview. People
who write Markdown will write it and people who do not will type
sentences and get sentences.

**Mentions are not part of this change**, and the storage is chosen so
they can be added without a migration. `@simon` would be a render-time
extension exactly as `[[label]]` already is — the source keeps what the
author typed and the renderer turns a known username into a link. The
part that needs designing is the notification, which is what anyone
typing an `@` will expect to happen, and that is its own issue.

A body is at most 4 KiB and cannot be empty or only whitespace. Leading
and trailing whitespace is trimmed.

You may edit and delete **your own** comments, with no time limit. The
server enforces this, not just the UI. Editing shows the field prefilled;
saving stamps the comment `edited` next to its time.

An edit posts to Discord too, worded as an edit. The channel is a feed of
what is being said and a correction is part of that; a quiet edit is the
one that misleads the people who read the channel instead of the site,
and a band of five does not produce enough typo fixes to be worth
filtering out. If it does turn out to be noise, the fix is to drop edits
or delay them, and that is one branch in one function.

A delete posts nothing. There is nothing to show.

Deleting is real: the body is erased and the row is kept as a tombstone
rendered as *comment deleted*, so replies that answer it keep their
context and do not reattach themselves to something else. When every
comment in a thread has been deleted, the thread goes too, and its pin
with it. Deleting is confirmed first, on both clients.

Deleting a clip takes its threads and comments with it, as it already
takes its labels.

### Failure and offline

Comments are network-only on both clients; nothing about them is cached.

On the web, the clip page's comments arrive in the page props, so they
are there on first paint with no second request. A failed post leaves the
composer open with its text and the error under it. Posting again is the
retry.

On the phone, the comments section loads with the clip and shows the
usual error-with-retry when it cannot. A post made with no network fails
with the client's existing "Couldn't reach …" wording and keeps the
draft in the field. Nothing is queued for later: a comment that arrives
an hour after you wrote it, out of order with the replies to it, is worse
than one that failed.

### Discord

Every comment is posted: new threads, replies, and edits alike. It uses
the webhook already configured in `IGGYBILLY_DISCORD_WEBHOOK_URL`
and follows the existing rules: fire-and-forget on a spawned task, logged
at WARN on failure, never able to fail or slow the request that caused
it, and silently disabled when no webhook is configured.

A new thread:

    💬 **simon** commented on [Riff](https://iggybilly.example/clips/7) at 0:14
    >>> the entry is late here

A reply:

    💬 **simon** replied to a comment on [Riff](https://iggybilly.example/clips/7) at 0:14
    >>> it is, I'll do it again

An edit:

    ✏️ **simon** edited a comment on [Riff](https://iggybilly.example/clips/7) at 0:14
    >>> it is, I'll do it again properly

Without `IGGYBILLY_BASE_URL` the clip is a bolded name instead of a link,
as uploads already are. The author and the clip name go through the
existing `md_escape`, so a clip called `@everyone` cannot ping the
channel. The body is Markdown the author wrote, which Discord renders as
Markdown of its own — near enough the same dialect that it reads as
intended — so it is **not** escaped, only quoted with `>>>`,
which blockquotes the rest of the message so the body cannot break the
line above it. One thing does need neutralising: `@` and `#` are pings in
Discord and are only text here, so `@everyone`, `@here` and any
`<@…>`-shaped run in a body have a zero-width space inserted after the
sigil. Bodies over 500 characters are truncated with `…`; the whole
message has to fit Discord's 2000-character limit, and the point of the
post is to make you open the clip.

The integration is **one-way**. An incoming webhook can post and cannot
read, so replying in Discord does nothing here. Making it two-way means a
bot, a gateway connection and a process that stays connected, which is a
different kind of program than this one.

## Implementation

### Database

`migrations/0005_clip_comments.sql` — number it whatever is next when it
lands.

```sql
-- Comments on a clip, anchored to a point in it.
--
-- Two tables because the anchor belongs to the conversation, not to each
-- remark in it: a thread is "the discussion about 0:14 of this clip", and
-- its replies inherit that without repeating it. It also means the
-- waveform's pins are a read of one small table.
--
-- Bodies are Markdown source, rendered on read by src/markdown.rs, the
-- same way label_wiki_revisions holds wiki pages. Unlike those, comments
-- are not versioned: a wiki page is a document the band maintains
-- together and its history is the point, while a comment is one person
-- saying one thing and an edit to it is a correction.
CREATE TABLE comment_threads (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    clip_id    INTEGER NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
    -- Seconds into the clip, fractional. REAL because the interesting
    -- moments in a two-bar riff are not on second boundaries; written
    -- rounded to the millisecond.
    at_seconds REAL NOT NULL,
    created_by INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- The clip page reads every thread in anchor order; the list reads a
-- count per clip. Both are this index.
CREATE INDEX idx_comment_threads_clip ON comment_threads(clip_id, at_seconds, id);

-- One remark. The first comment of a thread is simply its oldest; there
-- is no "is the opening one" flag to keep true.
--
-- Deleting erases the body and sets deleted_at rather than removing the
-- row: a reply that answers a comment needs the comment to still be
-- there, even as a tombstone. A thread whose comments are all deleted is
-- removed outright, so the waveform stops showing a pin for a
-- conversation that no longer says anything.
CREATE TABLE comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id  INTEGER NOT NULL REFERENCES comment_threads(id) ON DELETE CASCADE,
    -- Markdown source, as written. Never HTML: rendering happens on read.
    body       TEXT NOT NULL,
    author_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- Set on every edit after the first save; null means never edited.
    edited_at  TEXT,
    deleted_at TEXT
);

CREATE INDEX idx_comments_thread ON comments(thread_id, id);
```

`ON DELETE CASCADE` from `clips` reaches `comments` through
`comment_threads` because `foreign_keys` is on (`src/db.rs`), so
deleting a clip needs no extra statement in `handlers::clips::remove`.

### Domain types and queries

`src/queries/comments.rs`, new, in the shape the other query modules
take: plain domain types with timestamps exactly as SQLite holds them,
and no opinion about how either surface renders them.

`body` is Markdown source and stays source here. That is deliberately the
rule `src/queries/wiki.rs` already states for pages: the web wants HTML
it can drop into the document and the phone renders Markdown itself with
a Flutter widget, so rendering belongs at the edges and the database
holds one form.

```rust
pub struct Thread {
    pub id: i64,
    pub clip_id: i64,
    pub at_seconds: f64,
    pub comments: Vec<Comment>,
}

pub struct Comment {
    pub id: i64,
    pub thread_id: i64,
    pub author_id: i64,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub is_deleted: bool,
}
```

Functions:

- `for_clip(pool, clip_id) -> Vec<Thread>` — one query for the threads
  ordered by `(at_seconds, id)`, one for all their comments ordered by
  `(thread_id, id)`, then stitched. Two queries regardless of how many
  threads there are; the labels reads next door are an N+1 that is not
  worth copying.
- `counts_for_clips(pool) -> HashMap<i64, i64>` — non-deleted comments
  per clip, for the list. One `GROUP BY`.
- `create_thread(pool, clip_id, at_seconds, author_id, body) -> (thread_id, comment_id)`
  — both inserts in one transaction.
- `reply(pool, thread_id, author_id, body) -> i64`.
- `edit(pool, comment_id, author_id, body) -> bool` — `UPDATE … WHERE id
  = ? AND author_id = ? AND deleted_at IS NULL`, returning whether a row
  moved, so the author check is the statement rather than a read followed
  by a write.
- `delete(pool, comment_id, author_id) -> bool` — sets `body = ''` and
  `deleted_at`, then deletes the thread if it has no non-deleted comments
  left, in one transaction.
- `clip_of(pool, comment_id) -> Option<i64>` — the edit and delete routes
  are addressed by comment id and answer with the clip's whole thread
  list, so they need to know which clip that is.

`src/queries/clips.rs` gains `comment_count: i64` on `Clip`, filled by a
`LEFT JOIN` on an aggregate in the existing `CLIP_COLUMNS` queries rather
than a second round trip per clip.

### HTTP

The web's `/api` (`src/handlers/comments.rs`, wired in `src/web.rs`):

| Route | Body | Answers |
| --- | --- | --- |
| `GET /api/clips/{id}/comments` | — | `CommentThread[]` |
| `POST /api/clips/{id}/comments` | `{threadId?: number, atSeconds?: number, body: string}` | `CommentThread[]` |
| `POST /api/comments/{id}` | `{body: string}` | `CommentThread[]` |
| `DELETE /api/comments/{id}` | — | `CommentThread[]` |

One POST covers both cases: with `threadId` it is a reply into that
thread, without it `atSeconds` is required and a new thread is opened.
One endpoint, one payload, and the client does not have to know which
kind of thing it is sending until it sends it.

Every write answers with the clip's full thread list, the same convention
the label endpoints already use — the client replaces its state rather
than guessing what the server made of what it sent. Posting is not
idempotent, so a double-press is two comments; the composer disables
itself while a post is in flight.

Errors: 400 for an empty or oversized body, a missing `atSeconds` on a
new thread, or a non-finite or negative anchor; 403 for editing or
deleting someone else's comment; 404 for a clip, thread or comment that
is not there. `atSeconds` is clamped to the clip's duration when the
server knows it.

The phone's `/api/v1` (`src/api/comments.rs`) mirrors all four at
`/api/v1/clips/{id}/comments`, `/api/v1/comments/{id}`, differing only in
the wire shape: RFC 3339 timestamps as stored, no pre-formatted strings.

Web shape (`camelCase`, as everything there is):

```jsonc
{
  "id": 12,
  "atSeconds": 14.25,
  // "0:14" — the web prints this and has no clock of its own.
  "atLabel": "0:14",
  "comments": [{
    "id": 31,
    "author": "simon",
    // Rendered by src/markdown.rs, as the wiki's HTML is. Safe to embed.
    "bodyHtml": "<p>the entry is <em>late</em> here</p>\n",
    // The source, only on comments the viewer may edit — it is what the
    // composer reopens, and nobody else needs it.
    "body": "the entry is *late* here",
    // Already Stockholm-local, like every other date in the page props.
    "createdAt": "2026-09-12 18:33",
    "edited": false,
    "isDeleted": false,
    // Whether the viewer may edit and delete this one.
    "canModify": true
  }]
}
```

The phone's is the same tree with `createdAt` as `2026-09-12T16:33:07.123Z`,
`editedAt` as a nullable instant rather than a flag, and no `atLabel`. It
also carries `body` and no `bodyHtml`: the app renders Markdown itself
with a Flutter widget, which is already how it shows a wiki page.

`src/markdown.rs` gains a second entry point for this. `render` keeps the
wiki's option set; a new `render_inline` enables the narrower set from
"Writing, editing, deleting" — the same safety flags, minus headings,
tables and images — so the two callers cannot drift apart in what they
consider safe. The wikilink retargeting is shared.

The clip page's props (`ClipProps` in `src/handlers/clips.rs`) gain
`comments: Vec<CommentThread>` so the first paint needs no fetch, exactly
as the index page carries `activeWikis`. `ClipRow` and the v1 `Clip` gain
`commentCount`.

### Discord

`src/discord.rs` gains one method, alongside `clips_uploaded` and
`wiki_edited`:

```rust
pub fn comment_posted(
    &self,
    author: &str,
    clip: (i64, &str),
    at_seconds: f64,
    body: &str,
    kind: CommentPostKind, // New, Reply, Edit
)
```

with a `comment_message` private function built and unit-tested like the
existing two: the verb and the emoji switch on `kind`, the clip goes
through `clip_link`, and the time is formatted `m:ss` by a small helper
here rather than borrowed from the frontend.

The body is the one part that differs from the existing messages. It is
Markdown either way, and Discord's dialect is close enough that it should
be passed through rather than `md_escape`d — escaping it would show
people their own asterisks back. So instead: truncate to 500 characters,
defuse the ping sigils as described above, and prefix with `>>> `. Unit
tests mirror the existing ones and must include a body containing
`@everyone`, a masked link, and a fenced code block with a `>>>` inside
it.

Called from the create, reply and edit paths in both
`src/handlers/comments.rs` and `src/api/comments.rs` — or, better, from a
shared `pub(crate)` function in the handlers module that both call, the
way `ingest`, `remove` and `set_name` are already shared.

### Web frontend

- `web/src/components/Comments.tsx` — new: the strip, the pins, the
  pending anchor, the thread list, the collapse control, the composer and
  the reply forms. It takes the clip, its duration and the initial
  threads, and owns the thread list as state from then on since every
  write hands back a fresh one. A body is `bodyHtml` through
  `dangerouslySetInnerHTML`, as `WikiPanel.tsx` already does and for the
  same documented reason; the scoped lint disable goes with it.
- `web/src/pages/clip.tsx` — render `<Comments …/>` under the existing
  `<Player>`, and add the **Comment at …** button to `.player-controls`.
- `web/src/pages/index.tsx` — the comment count on a row, next to the
  duration, as `💬 3` with a title of "3 comments"; nothing when zero.
- `web/src/api.ts` — `getComments`, `postComment`, `editComment`,
  `deleteComment`.
- `web/src/types.ts` — `CommentThread`, `Comment`, `commentCount` on
  `ClipSummary`, `comments` on `ClipProps`.
- `web/src/styles/app.css` — `.comment-strip`, `.comment-pin`,
  `.comment-pin.pending`, `.comment-threads`, `.comment-thread`,
  `.comment`, `.comment-deleted`, `.comment-replies`, `.comment-compose`.

Seeking to an anchor uses the existing player context: `player.play()` if
the clip is not loaded, then `player.seek(atSeconds / duration)`. The
player's `seek` takes a fraction; a clip with no known duration cannot
compute one, so on those clips the timestamp button only plays, and says
so in its `title`.

### Flutter

- `mobile/lib/src/api/models.dart` — `CommentThread` and `Comment`, parsed
  as defensively as the rest.
- `mobile/lib/src/api/client.dart` — `comments(clipId)`,
  `postComment(clipId, {threadId, atSeconds, body})`,
  `editComment(id, body)`, `deleteComment(id)`.
- `mobile/lib/src/ui/comments.dart` — new: the thread list, the collapse
  control, the composer sheet and the reply field, as a widget the clip
  screen embeds. Bodies go through the same Markdown widget the wiki
  page uses.
- `mobile/lib/src/ui/clip_page.dart` — a Comments section under the
  existing player row, plus a **Comment at m:ss** button beside play.
- `mobile/lib/src/ui/waveform.dart` — an optional `markers: List<double>`
  (fractions, 0–1) painted as thin ticks, and an optional
  `onMarkerTapped`. The painter already has the geometry; this is a
  handful of lines and keeps the pins in the same place they are on the
  web.
- `mobile/lib/src/ui/clips_page.dart` — the count in `_subtitle()`.
- `mobile/test/` — the scripted server grows the comment endpoints, and a
  widget test posts a comment, replies to it, edits it and deletes it.

### Documentation

The root `README.md` "What it does" list gains a line for comments and
for their Discord posts, and the `IGGYBILLY_DISCORD_WEBHOOK_URL` entry —
which currently says "whenever a clip is uploaded or a label's wiki page
is edited" — gains comments and says the integration is one-way.

## Open questions

- **Nothing marks a thread as settled.** A band that uses this heavily
  will want to tick off "fixed that" so the waveform isn't a forest of
  pins for things already done. A `resolved_at` on `comment_threads` and
  a dimmed pin would do it, and it is easy to add later; it is left out
  now because "resolve" implies a workflow nobody has asked for.
- **Replies are flat.** Two levels, and a reply to a reply lands in the
  same list. If a thread ever needs a tree, the thread is a conversation
  that should be in Discord.
- **Discord is one-way and stays that way.** Worth revisiting only if
  people start replying there and expecting it to land here — in which
  case the fix is a bot, not a change to this design.
- **The clip list shows a count and nothing else.** A "recent comments"
  view — everything said across all clips, newest first — would be the
  natural way to catch up, and is a separate page and a separate issue.
- **Mentions are storage-ready but unbuilt.** `@simon` is a render-time
  extension whenever someone wants it. What needs deciding first is what
  a mention *does*: a link only, a Discord ping of the mapped account, or
  something in the app. Its own issue.
- **Markdown in a Discord post is passed through rather than escaped.**
  That is right for the common case and slightly wrong for a body that
  leans on a construct Discord reads differently, such as a fenced block
  inside a `>>>` quote. The alternative is escaping, which is worse for
  every ordinary comment.
