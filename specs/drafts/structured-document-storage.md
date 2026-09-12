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

Nothing about this is visible on either screen, and that is the
requirement. Someone writing a page sees exactly what they see today:
a textarea of Markdown, a Save button, a rendered page, a history.

What changes, from the outside:

- **The two clients agree.** A page looks the same on the web and on the
  phone, because both are drawing the same document rather than each
  reading the same text.
- **An old page stays what it was.** Upgrading the parser changes how new
  text is read and leaves every existing page alone.
- **Unsupported syntax is refused at save time, not silently dropped.**
  If someone writes something the model cannot hold, the editor says so
  while they are still looking at it, naming the line. Today the same
  text is accepted and quietly rendered as something else, or as nothing.

The editor keeps taking Markdown. It is a good input language and people
know it. It stops being the thing we keep.

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
more of Markdown; that is a follow-up issue, not this spec. Raw HTML is excluded on
the same grounds 001 excluded it, and now it is excluded by the shape of
the storage rather than by a flag on a renderer.

### What this is for, beyond the wiki

The same storage serves clip comments when they arrive. That is most of
the reason to do it now. See "Ordering" at the end.

## Implementation

### Serialisation

The document is JSON, in a `TEXT` column, alongside the source.

The alternatives are worth naming, since the obvious instinct is a blob.
SQLite has JSONB, a binary encoding it reads faster than text, and a
`BLOB` of MessagePack or CBOR would be smaller again. Neither earns its
place here. Pages are a few kilobytes and are read one at a time on a
request that also touches the network, so the decode cost is not
measurable. What is measurable is what you can do at three in the
morning: `sqlite3 iggybilly.db "select document from …"` prints something
a person can read, `json_extract` works in a query, a backup diff shows
which page changed, and the format needs no tool to inspect. A blob costs
all of that to save nothing anyone will notice.

JSONB stays available if this is ever wrong. `json(document)` and
`jsonb(document)` convert in place, and nothing above the storage layer
would change.

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

A new migration adds two columns to `label_wiki_revisions`, and a
backfill fills them. Revisions are immutable and always have been, so
the backfill is the only time a stored document is produced from source
other than at the moment of saving.

```sql
-- The authoritative form of a page is the parsed document, not the
-- Markdown it was written in. Markdown's reading changes with the
-- parser and with the decade; a document does not. See
-- specs/drafts/structured-document-storage.md.
--
-- `content` stays, and stays exactly what the author typed: it is what
-- the editor reopens and what a history reader wants to see. It is no
-- longer what anything renders from. The invariant is that `document`
-- is `content` parsed by `parser` at the moment the revision was
-- written, and since revisions are never updated, the two cannot drift.
ALTER TABLE label_wiki_revisions ADD COLUMN document TEXT;
-- The parser and options that produced it, for forensics when a page
-- reads oddly. Not consulted at render time.
ALTER TABLE label_wiki_revisions ADD COLUMN parser TEXT;
```

Both are nullable because SQLite cannot add a `NOT NULL` column without a
default, and a default here would be a lie.

The backfill runs in Rust, not in SQL: it is a parse per revision, and a
migration that needs the application's own parser is not something SQL
can express. `sqlx::migrate!` has no hook for running code alongside a
migration — its migrations are SQL files and nothing else — but it does
not need one. `db::connect` already runs the migrations on every start,
so the backfill goes directly after that call:

```rust
sqlx::migrate!("./migrations").run(&pool).await?;
document::backfill(&pool).await?;
```

`backfill` selects the revisions whose `document` is null, parses each
and writes the result. It is idempotent by construction: once every row
is filled, the select returns nothing and the call costs one query. There
is no manual step after merging, and no deploy that can forget one; a
database restored from an old backup is filled the first time the server
opens it.

A revision whose source does not fit the model — a table, an image, raw
HTML written before this change — cannot be parsed, and the backfill must
not refuse to start the server over it. It leaves that row's `document`
null and logs the revision at WARN. The loader, finding a null, serves the
source as a single `code-block`, so the page is readable and visibly
unconverted, and the next save of that page, which must parse, fixes it.

### Rust

`src/document.rs`, new. The model, its serde derives, and:

```rust
pub fn parse(source: &str) -> Result<Document, ParseError>;
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

`to_html` replaces `markdown::render` for pages. `src/markdown.rs` is
reduced to the comrak call and the AST walk, or absorbed into
`document.rs` entirely, which is probably cleaner — the module's whole
purpose was rendering, and rendering moves.

Its existing tests move with it, rewritten against the new path: raw
HTML, `javascript:` links, wiki links, and wiki links inside code. Those
four are the spec of the safety properties and must not be lost in the
move. New tests: every node type round-trips through serde, an
unsupported construct is an error and not a silent drop, and a document
stored by an older model version still loads.

`src/queries/wiki.rs` gains `document` on `Page` and `Revision`, and
`save` takes both the source and the parsed document, which the handler
produces so that a parse failure is a 400 before anything is written.

### The clients

Web: `WikiPage.content_html` keeps its name and meaning and is now
produced by `document::to_html`. `WikiPanel.tsx` and `wiki-history.tsx`
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
since the server is what parses.

### API shape

`/api/v1` sends both: `content` for the editor, `document` for rendering.
The web's `/api` sends `content` and `content_html` as it does now, and
does not need the document, since it cannot render one any better than
the server can.

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

- **JSON text over JSONB or a binary encoding.** Argued above, and the
  argument is about inspectability rather than correctness. If the
  database ever gets big enough that this matters, the conversion is a
  migration and nothing above it changes.
- **The source column stays.** It is what the editor reopens, and the
  alternative is serialising the document back to Markdown, which would
  normalise everyone's typing on every edit. Keeping it means one row
  holds the same content twice, in two forms, which is a duplication with
  a reason but still a duplication.
- **Whether the phone renderer is worth its size.** A few hundred lines
  of widget-building replaces a package. It is the only way to make the
  two clients agree, but it is also the kind of code that grows a long
  tail of layout bugs.
- **What happens to a page that fails to parse under a future model.**
  The loader falls back and logs, which is right for a bug and wrong as a
  strategy. A real answer is a model migration that rewrites stored
  documents, and the first time the model version moves is when that gets
  designed.
