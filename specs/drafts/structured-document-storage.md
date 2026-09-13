# Structured storage for authored text

Complements [001](../implemented/001-label-wiki.md), whose storage
decision it revisits.

Spec 001 chose to keep wiki pages as Markdown source and render them on
read. This changes that: the authoritative stored form becomes a parsed
document, a tree of nodes in a closed model of our own, and Markdown
becomes the input format rather than the format of record.

001 is not wrong about where it was aiming. It wanted one durable form
that survives a change of renderer. It picked the wrong one.

## Why the source is not the durable form

Markdown is not a format. It is a family of dialects with a shifting
boundary, and "render this text as Markdown" is a question whose answer
depends on which parser you ask and when you asked it. Keep the source
and you have not stored a document, you have stored an instruction to
reinterpret one later, under rules nobody promised to hold still.

That is not hypothetical here, in two ways.

**The two clients already disagree.** The web renders with comrak. The
phone renders the same source itself with `flutter_markdown_plus`, a
different implementation of a different reading of the syntax. 001 called
that a feature, and for layout it is. For interpretation it is a bug
waiting for someone to write a table, a nested list or a bare URL and see
two different pages. Nothing today makes the two agree, and nothing can,
as long as each is handed text to interpret.

**A parser upgrade rewrites history.** `comrak` moves, and its options
move with it. Bump it and every page ever written is re-read under the
new rules. Most of the time nothing changes; when something does, it
changes silently, in documents whose authors are not present to be
consulted, and the revision history shows no edit because there was none.
A wiki whose whole point is that nothing is ever lost should not have a
rendering that quietly drifts.

The fix is to do the interpreting **once**, when the author is there and
can see the result, and to store what was decided.

## Why not comrak's own tree

Spec 001 argued against storing a parse tree, and that argument was
against the wrong thing. Storing `comrak`'s AST would indeed write a
library's internal model into the schema, and would turn every upgrade of
it into a migration. That is a real objection to a real bad idea.

What this proposes instead is a **document model of our own**: a small,
closed, explicitly versioned set of node types that we define and control,
which comrak happens to be the thing that produces. Swapping comrak for
something else changes one function. The stored documents do not notice.

The model is deliberately narrower than Markdown. It has what a page
about a song needs and nothing else, and anything outside it is not
representable, which is the point of a closed set.

## Functionality

Almost nothing about this is visible on either screen. Someone writing
a page sees what they see today: a textarea of Markdown, a Save button, a
rendered page, a history.

What changes, from the outside:

- **The two clients agree.** A page looks the same on the web and on the
  phone, because both are drawing the same document rather than each
  reading the same text.
- **An old page stays what it was.** Upgrading the parser changes how new
  text is read and leaves every existing page alone.
- **What you typed comes back tidied.** The Markdown is not kept; the
  editor reopens the document written back out as Markdown. A `*` for
  emphasis may come back as `_`, a list marker as `-`, a run of blank
  lines as one. The page itself is unchanged by this, since the page is
  the document, and it happens once, on the save.
- **Unsupported syntax is refused at save time, not silently dropped.**
  If someone writes something the model cannot hold, the editor says so
  while they are still looking at it, naming the line. Today the same
  text is accepted and quietly rendered as something else, or as nothing.

The editor keeps taking Markdown. It is a good input language and people
know it. It stops being the thing we keep: the document is the only
stored form, and the Markdown in the editor is produced from it.

### The model

One version of the model at a time, stamped on every stored document.

Node types are named in `kebab-case`, both in the stored JSON and
wherever they are spelled out elsewhere.

Block nodes: `paragraph`, `heading` (levels 1 to 6), `list` (ordered or
not, with a start number), `list-item`, `code-block` (with an optional
language), `block-quote`, `thematic-break`.

Inline nodes: `text`, `emphasis`, `strong`, `strikethrough`, `code`,
`link` (to a URL), `wiki-link` (to a label), `line-break`.

`wiki-link` is worth its own node rather than being a `link` with a
rewritten target, which is what 001 does. A wiki link points at a label,
not at a URL, and the URL is a rendering of that. Keeping the label means
the phone can route to its own screen instead of following a web path,
and that renaming a label could one day fix its inbound links.

Not in the model: raw HTML in any form, images, tables, footnotes, task
lists, headings deeper than six, autolinked bare text. Tables are out
because they are hard to render well on a phone and nothing a page about a
song needs is tabular. Where the model grows, it should grow towards
nodes that mean something here — lyrics, chords — rather than towards
more of Markdown; that is a follow-up issue, not this spec. Raw HTML is
excluded on the same grounds 001 excluded it, and now it is excluded by
the shape of the storage rather than by a flag on a renderer.

### What this is for, beyond the wiki

The same storage serves clip comments when they arrive. That is most of
the reason to do it now. See "Ordering" at the end.

## Implementation

### Serialisation

The document is stored as SQLite JSONB, in a `BLOB` column, and it is
the only stored form of the page.

JSONB is SQLite's own binary encoding of JSON, available since 3.45; the
SQLite that sqlx bundles here is 3.46. It is not a separate format with
its own library: it is the parse tree SQLite would otherwise build every
time it reads JSON text, stored already built. So it is somewhat smaller
on disk than the text, and any JSON function over it — `json_extract`,
`->>`, a future index on an expression — skips the parse. Performance
and storage efficiency are a stated priority for this project, and that
is the deciding argument.

The encoding is SQLite's internal one and is not meant to be read by
anything else, so it never leaves the database. Writes bind the serde
JSON text through `jsonb(?)`, and reads select `json(document)` and hand
the text to serde. Inspection loses nothing: `select json(document) from
…` in the `sqlite3` shell prints the same readable JSON a `TEXT` column
would have held, and `json_extract` works on either.

MessagePack or CBOR in a plain `BLOB` would be smaller again, but opaque
to SQLite itself, and that is a worse trade than the one it saves.

Nodes are tagged by a `type` field, serde-derived with
`rename_all = "kebab-case"`, with children under `children` and no
positional arrays:

```jsonc
{
  "model": 1,
  "blocks": [
    { "type": "heading", "level": 2,
      "children": [{ "type": "text", "value": "Bridge" }] },
    { "type": "paragraph", "children": [
      { "type": "text", "value": "Second take is the one, see " },
      { "type": "wiki-link", "label": "bridge-take-3", "text": null },
      { "type": "text", "value": "." }
    ]}
  ]
}
```

`model` is the version of the node model, not of the parser. It changes
when the set of node types changes, which should be rare and is always a
migration that rewrites stored documents rather than a branch in the
renderer.

### Database

Two migrations, with a Rust backfill between them. The first adds the
new columns; the backfill converts every existing revision; the second
drops `content` and makes `document` required. Revisions are immutable,
so the backfill is the only time a document is produced other than at
the moment of saving.

The first:

```sql
-- The authoritative form of a page is the parsed document, not the
-- Markdown it was written in. Markdown's reading changes with the
-- parser and with the decade; a document does not. See
-- specs/implemented/NNN-structured-document-storage.md.
--
-- Nullable only until the backfill and the next migration: SQLite cannot
-- add a NOT NULL column without a default, and a default would be a lie.
ALTER TABLE label_wiki_revisions ADD COLUMN document BLOB;
-- The parser and options that produced the document, for forensics
-- when a page reads oddly. Not consulted at render time.
ALTER TABLE label_wiki_revisions ADD COLUMN parser TEXT;
```

The second rebuilds the table, since SQLite cannot add `NOT NULL` to an
existing column. The copy is also the guard: a revision the backfill
missed fails the `NOT NULL`, the migration's transaction rolls back, and
the server does not start with `content` gone and a page lost.

```sql
-- The Markdown is gone: the editor is given the document written back
-- out as Markdown. document is JSONB; read it with json(document).
CREATE TABLE label_wiki_revisions_new (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    label_id  INTEGER NOT NULL REFERENCES labels(id) ON DELETE CASCADE,
    document  BLOB    NOT NULL,
    parser    TEXT    NOT NULL,
    edited_by INTEGER NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    edited_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO label_wiki_revisions_new (id, label_id, document, parser, edited_by, edited_at)
    SELECT id, label_id, document, parser, edited_by, edited_at FROM label_wiki_revisions;
DROP TABLE label_wiki_revisions;
ALTER TABLE label_wiki_revisions_new RENAME TO label_wiki_revisions;
CREATE INDEX idx_label_wiki_label ON label_wiki_revisions(label_id, id DESC);
```

Nothing references `label_wiki_revisions`, so dropping and renaming it
inside the migration's transaction needs no change to `foreign_keys`.

The backfill runs in Rust, not in SQL: it is a parse per revision, and a
migration that needs the application's own parser is not something SQL
can express. `sqlx::migrate!` has no hook for running code between
migrations, but it does not need one: `db::connect` can run the embedded
migrations in two passes with the backfill between.

```rust
static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

let first = Migrator {
    migrations: MIGRATOR.iter().filter(|m| m.version <= ADD_DOCUMENT).cloned().collect(),
    ignore_missing: true,
    ..Migrator::DEFAULT
};
first.run(&pool).await?;
if !applied(&pool, DROP_CONTENT).await? {
    document::backfill(&pool).await?;
}
MIGRATOR.run(&pool).await?;
```

`ADD_DOCUMENT` and `DROP_CONTENT` are the two migrations' versions, and
`applied` looks the second up in `_sqlx_migrations`. `ignore_missing`
lets the first pass run against a database that has already applied
later migrations, and the check on `DROP_CONTENT` skips the backfill once
`content` no longer exists. On every start after the first, both passes
are no-ops.

`Migrator`'s fields are public so that `migrate!` can build one in a
constant, and sqlx marks them hidden and exempt from semver. Building a
`Migrator` by hand therefore leans on sqlx 0.8's internals. The lock file
pins that, and a test in `tests/` that migrates a database holding a
pre-change revision to the end is what notices when an upgrade breaks it.
Once no database older than the change exists — every deploy has run it
and backups before it have aged out — the two passes and the backfill can
be deleted, and `connect` goes back to one `run`.

This is one deploy with no manual step, and it is also right for a
database restored from a backup taken before the change: it is brought
to the first migration, converted, and taken the rest of the way, in
order, the first time the server opens it.

`backfill` selects the revisions whose `document` is null and writes a
document for each. A revision whose source does not fit the model — a
table, an image, raw HTML written before this change — cannot be parsed,
and dropping its text would lose it. It becomes a document holding the
source verbatim as a single `code-block`, with `parser` recording that
it was a fallback, and is logged at WARN. The page is readable, visibly
unconverted, and the text is all there to be fixed by the next edit.

### Rust

`src/document.rs`, new. The model, its serde derives, and:

```rust
pub fn parse(source: &str) -> Result<Document, ParseError>;
pub fn to_markdown(doc: &Document) -> String;
pub fn to_html(doc: &Document) -> String;
```

`parse` is comrak's AST walked into our nodes. Anything comrak produces
that the model does not have is a `ParseError` naming the line, which is
what surfaces to the author as "unsupported syntax" rather than being
dropped. That includes raw HTML, so the safety property 001 got from
`unsafe_ = false` now comes from the model having no node that can hold
markup. Link targets are still scheme-checked, because a `link` node can
hold any string. comrak's table extension stays on even though the model
has no table: with it off, a table would parse as a paragraph of pipes
and slip through, rather than being recognised and refused.

`to_markdown` writes a document back out as Markdown, in one fixed
style, for the editor and for restore. Its contract is that it loses
nothing the model holds: `parse(to_markdown(d)) == d` for every
document, which is what makes dropping the source safe. The text is
normalised; the document is not changed by the trip.

`to_html` replaces `markdown::render` for pages. `src/markdown.rs` is
reduced to the comrak call and the AST walk, or absorbed into
`document.rs` entirely, which is probably cleaner — the module's whole
purpose was rendering, and rendering moves.

Its existing tests move with it, rewritten against the new path: raw
HTML, `javascript:` links, wiki links, and wiki links inside code. Those
four are the spec of the safety properties and must not be lost in the
move. New tests: every node type round-trips through serde and through
JSONB, `parse(to_markdown(d)) == d` over a set of documents exercising
every node type and every escape `to_markdown` needs (a `*` in text, a
line starting with `#` or `1.`), an unsupported construct is an error
and not a silent drop, the backfill's fallback keeps the source whole,
and a document stored by an older model version still loads.

`src/queries/wiki.rs` replaces `content` with `document` on `Page` and
`Revision`, and `save` takes the parsed document, which the handler
produces so that a parse failure is a 400 before anything is written.
`revision_content` becomes `revision_document`, and restore saves that
document as the new revision without a trip through Markdown.

### The clients

Web: `WikiPage.content_html` keeps its name and meaning and is now
produced by `document::to_html`; `content` is `document::to_markdown`. `WikiPanel.tsx` and `wiki-history.tsx`
do not change at all, including their scoped lint disables, whose comment
should now point at `document.rs`.

Phone: this is the real work. `/api/v1` stops sending `content` as the
thing to render and sends `document`. `mobile/lib/src/ui/wiki_page.dart`
replaces `MarkdownBody` with a renderer over the node model — a
`switch` over node types building Flutter widgets, which is a few hundred
lines and no dependency. `flutter_markdown_plus` comes out of
`mobile/pubspec.yaml`.

That is a larger change on the phone than on the web, and it is also the
point: it is what makes the two agree. The editor still posts Markdown,
so `saveWiki` is unchanged and a device with an old build keeps working,
since the server is what parses. After a save, both editors take the
page from the save's answer rather than keeping what was typed, so the
normalised text is what the author sees next.

### API shape

`/api/v1` sends both: `content`, produced by `to_markdown`, for the
editor, and `document` for rendering.
The web's `/api` sends `content` and `content_html` as it does now, and
does not need the document yet, since it cannot render one any better
than the server can. That changes with
[local-first](local-first.md), where the browser renders from its own
replica and so renders the document itself.

A parse failure on save answers 400 with the line and a message, on both
surfaces, and both editors show it against the text rather than as a
toast that loses it.

## Ordering

Simon has decided that this spec and the local-first work
([#24](https://github.com/skagedal/iggybilly/issues/24)) are both
**prerequisites** for clip comments
([#6](https://github.com/skagedal/iggybilly/issues/6), spec
`clip-comments.md`).

The reasoning is worth writing down, because the dependency is not
obvious from the issues. Comments multiply both problems. They are
authored text, so every argument above applies to them and they would
otherwise arrive storing Markdown source, which is the thing being
undone. They are also the feature people will write offline, so they are
where sync and conflict resolution first bite. Building comments first
means building both foundations twice, once badly.

So `clip-comments.md` should not be started until this and #24 are
implemented, and its storage section — which currently says comments
follow the wiki in keeping Markdown source, citing 001 — is superseded by
this document and needs rewriting when it is picked up.

## Open questions

- **`to_markdown`'s style.** Which emphasis marker, which list marker,
  how code blocks are fenced. Any fixed choice satisfies the round-trip;
  the one to pick is whatever most of the existing pages already use,
  so that the first save of each changes as little as possible. Worth
  looking at the backfilled pages' `to_markdown` against their old
  source before fixing it.
- **History shows normalised text.** An old revision's source is gone
  after the second migration, so history shows each revision as
  `to_markdown` writes it, not as it was typed. Nothing a reader of
  history wants is in the difference.
- **Whether the phone renderer is worth its size.** A few hundred lines
  of widget-building replaces a package. It is the only way to make the
  two clients agree, but it is also the kind of code that grows a long
  tail of layout bugs.
- **What happens to a page that fails to parse under a future model.**
  The loader falls back and logs, which is right for a bug and wrong as a
  strategy. A real answer is a model migration that rewrites stored
  documents, and the first time the model version moves is when that gets
  designed.
