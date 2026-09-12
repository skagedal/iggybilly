# 001. Label wiki pages

Status: **accepted**, and in production since `migrations/0003_label_wiki.sql`.

Shipped before this directory existed, and written down afterwards from
the code. It is here because a later spec argues from it — the comments
draft takes its storage decision from this one — and an argument that
points at a file is weaker than one that points at a written reason.

Read it as a record of what was decided, not as a plan and not as a
description of the system as it stands today. Where the prose says "is",
read "was, when this was written".

## Functionality

Every label can carry one Markdown page. The obvious use is a song: the
lyrics, the chords, who plays what in the bridge, a note that the second
take is the one to learn. A label is already the thing the band groups
clips by, so it is the thing the prose belongs to.

### Where the page appears

Filtering the clip list by a label shows that label's page above the
clips. Filter by several and you get several pages, one per label, above
the intersection of their clips. There is no separate wiki section and no
index of pages: you arrive at a page by filtering for the label it
belongs to, which is the same action as looking at its clips. The page
and the recordings are one screen.

A label with no page yet is not an error and not an empty state to
dismiss. It shows the same panel with nothing in it and an **Edit**
button, because "there is no page here" and "start writing one" are the
same screen and do not need to be two.

### Editing

Any signed-in user may edit any page. There is no ownership and no
locking. A band of five sharing a set list shares the set list, and the
history below is what makes that safe rather than a permission model
nobody would maintain.

Editing swaps the rendered page for a plain textarea holding the Markdown
source, with **Save** and **Cancel**. There is no live preview and no
toolbar. Saving renders the new page in place.

A page's source is capped at 100 KiB. That is far more than anyone will
write and small enough that it cannot be used as storage.

### History

Every save is a revision, kept forever. Nothing is ever overwritten and
nothing is ever deleted: the current page is simply the newest revision,
and the history is the list of all of them, newest first, each showing
its author, when it was saved, and its rendered content.

Any revision can be **restored**. Restoring does not rewind anything — it
writes the old content as a *new* revision, so the act of restoring is
itself in the history and the thing you restored over is still there.
There is no way to lose text, which is what makes "anyone may edit"
tolerable.

A restore is reached from the history screen, which is its own page
rather than a panel, because reading a diff-shaped list of full documents
next to the clips would be two things at once.

### Elsewhere

- **Wiki links.** `[[some-label]]` in a page links to that label's
  filtered view, which is that label's page and its clips. `[[some-label|call it
  this]]` sets the link text. This is how one song's page points at
  another, and how a page points at the label holding the takes it talks
  about. A `[[…]]` inside code stays as it is written.
- **Discord.** Editing a page posts a short note to the channel, next to
  the ones for uploads. Restoring posts the same note: it is an edit.
- **The phone.** The app shows and edits pages and shows the history and
  restores from it, the same as the web.

### What it is not

Not a general wiki. There are no free-standing pages, no page titles, no
search across pages, no attachments, no diffs between revisions, and no
comments on a page. A page exists because a label exists, and it is the
one document that belongs to it.

## Implementation

### Database

`migrations/0003_label_wiki.sql`. One table, append-only.

```sql
CREATE TABLE label_wiki_revisions (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    label_id  INTEGER NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    content   TEXT NOT NULL,
    edited_by INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    edited_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX idx_label_wiki_label ON label_wiki_revisions(label_id, id DESC);
```

There is no `label_wiki_pages` table holding the current text. The
current page is `MAX(id)` for the label, and the index makes that read
the same cost a dedicated row would have been. The saving is not in the
query; it is that there is no second place for the truth to be, and so no
way for the page and its history to disagree.

Restore is not an operation on this schema. It is `save` called with an
older revision's content, which is why it needs no column and leaves no
special kind of row.

### The source is Markdown, and stays Markdown

This is the decision the later specs lean on.

The column holds what the author typed. HTML is produced on read, by
whoever is doing the reading, and is never stored. `src/queries/wiki.rs`
says so in its own doc comment and returns source; rendering happens at
the edges.

That is not an accident of the web being first. It is what lets the two
clients differ: `src/markdown.rs` renders to HTML for the browser, and
the Flutter app renders the same source itself with a Markdown widget, so
a page looks native on a phone rather than like a web page in a box. A
stored rendering would have had to be one or the other.

The alternative worth naming, because it comes up, is storing a parse
tree instead. It is the same idea done worse: the parse is cheap, and
writing comrak's node types into the schema turns a library's internal
model into a migration every time it changes. Source text survives a
change of renderer; an AST is a commitment to one.

### Rendering

`src/markdown.rs`, one `render(&str) -> String` over comrak.

Safety is the default rather than a filter applied afterwards.
`render.unsafe_` is left false, so raw HTML in the source is escaped
instead of passed through and dangerous URL schemes are stripped. The
output is therefore safe to embed directly, which is why the web hands it
to `dangerouslySetInnerHTML` and why the two call sites carry a scoped
lint disable pointing at this file rather than a sanitiser of their own.

GFM strikethrough, tables, autolinks and task lists are on, as things
that read naturally in a page about a song.

Wiki links are a rewrite on the AST, not on the text: the document is
parsed, every `WikiLink` node's target is replaced with `/?label=…`
percent-encoded, and the tree is formatted. Doing it on the tree is what
makes `[[verse]]` inside a code span stay text, because it never parsed
as a link in the first place. The unit tests at the foot of the module
are the specification of all of this, including the two escaping ones.

### Queries

`src/queries/wiki.rs`:

| Function | What it does |
| --- | --- |
| `page` | the newest revision for a label, or an empty page |
| `history` | every revision, newest first, each flagged `is_current` |
| `save` | append a revision |
| `revision_content` | one revision's text, looked up within its label |

`page` returning an empty page rather than `None` for a label nobody has
written about is what makes "no page" and "start a page" the same screen
without a branch in every caller.

`revision_content` takes both the label id and the revision id and
matches on both. A restore is addressed by a pair of ids from the URL,
and scoping the lookup means a mismatched pair cannot pull another
label's text into this label's page.

### HTTP

Web (`src/handlers/labels.rs`, wired in `src/web.rs`):

| Route | Purpose |
| --- | --- |
| `GET /api/labels/{id}/wiki` | the current page |
| `POST /api/labels/{id}/wiki` | save a revision, answer with the page |
| `GET /labels/{id}/wiki/history` | the history page |
| `POST /api/labels/{id}/wiki/restore/{rev}` | append a copy of `rev` |

A save answers with the page as it now stands rather than with a status,
so the client replaces its state instead of reconstructing what the
server made of what it sent. Restore answers `204` because its caller
reloads the history page, which is a full page route.

Pages are delivered to the index inside that route's props, as
`active_wikis` — one entry per active filter label that has a label row —
rather than fetched separately, so they are there on first paint.

Phone (`src/api/labels.rs`, wired in `src/api/mod.rs`): the same four,
under `/api/v1`. The module's own doc comment states the one real
difference — these carry Markdown source, not server-rendered HTML,
because Flutter renders Markdown itself. Everything else goes through the
same `queries` functions, so the two surfaces cannot drift in behaviour,
only in shape.

The 100 KiB cap is enforced in both handlers, against the same constant
value in each module.

### Clients

Web:

- `web/src/components/WikiPanel.tsx` — the panel, the textarea, save and
  cancel. It takes the page as a prop and owns it as state afterwards,
  since a save hands back a fresh one.
- `web/src/pages/wiki-history.tsx` — the history page and its restore
  buttons.

Both render `content_html` through `dangerouslySetInnerHTML`, with the
scoped disable described under "Rendering".

Phone:

- `mobile/lib/src/ui/wiki_page.dart` — view and edit, with `MarkdownBody`
  over the source.
- `mobile/lib/src/ui/wiki_history_page.dart` — the history and restore.

### Discord

`src/discord.rs` has `wiki_edited(editor, label)`, called from the save
and restore paths on both surfaces, fire-and-forget on a spawned task and
a no-op when no webhook is configured, like everything else in that
module.

## What was left open, and still is

- **No diffs.** The history shows whole rendered revisions, so seeing
  what changed between two saves means reading both. For pages the length
  these are, that has been fine.
- **No concurrency control.** Two people editing at once means the second
  save wins outright. Nothing is lost, because the first is still a
  revision, but nobody is told. A version stamp on the save would fix it
  and has not been needed by five people.
- **No index of pages.** You cannot ask which labels have pages without
  filtering by each. An index would be a small query and a small screen.
- **Restore is a button, not a confirm.** It appends, so a misclick costs
  one more click to undo, which is why it never grew a dialog.
