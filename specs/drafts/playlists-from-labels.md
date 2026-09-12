# Playlists from labels

Implements [#20](https://github.com/skagedal/iggybilly/issues/20).

The clips carrying a label are already a set the band thinks of as a
thing — `set-2026-spring`, `takes-of-the-bridge`, `songs-we-can-play`.
What they are missing is an order and a play button that keeps going. So
a label *is* a playlist: viewing the label is viewing the playlist,
dragging a clip reorders it for everyone, and pressing play on a track
plays from there to the end of the list.

Three decisions carry the design, and each is argued where it appears
below:

1. **Order belongs to the membership, not to the clip.** A clip on three
   labels sits in three playlists and has an independent place in each.
   This is settled, not open.
2. **Order is sparse integers and a move writes one row.** Two people
   dragging different clips at the same time do not clobber each other.
3. **Repeat stays a two-valued toggle** and comes to mean the queue,
   keeping the media element's own gapless looping wherever gaplessness
   is achievable.

## Functionality

### What counts as a playlist view

A playlist view is a label-filtered clip list with **exactly one** label
active. `/?label=verse-1` on the web, one filter chip on the phone.

With two or more filters the list stays exactly as it is today: reverse
chronological, no drag handles, no queue. "The clips carrying all of
these labels" is an intersection, and an intersection has no order —
there are two orders to choose between and no reason to prefer either.
Pressing play there behaves as it does now: one clip, no queue.

There is no new URL and no new screen. The issue says viewing the label
is viewing the playlist, and that is the filtered list we already have.

### The playlist view

With one label active, the list changes in four ways.

- The clips are in **playlist order** rather than newest-first.
- Each row gets a **drag handle** on the left, and the list can be
  reordered by dragging. On the phone this is a long-press drag on the
  handle; on the web it is a pointer drag, with the handle also
  focusable and reorderable by keyboard (arrow up/down while held).
- A heading above the list names the playlist: the label, the number of
  clips, and their total length. The label's wiki page still sits above
  that, as it does now.

Pressing play on any row starts the whole label as a queue from that row
and continues into the next when it ends.

Reordering is optimistic: the row moves as you drop it and the request
goes out behind it. If the request fails — no network, or the server
renumbered underneath you — the list springs back to the server's order
and a message says why. A queue playing from this label is reordered
along with it (see "The queue follows the playlist").

Anyone signed in can reorder. There is no per-user order: the issue asks
for one order that affects everyone, and a band that shares a set list
shares it.

### The order of a clip that has several labels

A clip's place is a property of the pair, not of the clip. `bridge-take-3`
can be second in `bridge` and ninth in `2026-04-11-rehearsal`, and moving
it in one has no effect on the other. Anything else would mean dragging a
clip in one playlist silently reorders another, which is the kind of
behaviour people stop trusting the feature over.

A clip newly given a label goes to the **end** of that label's playlist.
A clip that loses a label loses its place in that playlist; if the label
is added again later it lands at the end, not where it used to be.

### Playing a playlist

Pressing play on a row loads the label's whole clip list as the player's
queue, positioned at that row, and starts it. When a track ends the next
one starts. At the end of the queue, playback stops with the last track
loaded and wound back — unless repeat is on.

Pressing play on a clip anywhere else — the clip's own page, an
unfiltered or multiply-filtered list — loads a queue of one. Every
playback is a queue; most queues have one track.

The queue shows in the player view as "Playing from **verse-1** — 3 of
8", which expands into the list of tracks; tapping one jumps to it.

**Next** and **previous** controls appear in the player view when the
queue has more than one track. Previous within the first three seconds of
a track goes to the previous track; after that it restarts the current
one, which is what every other player does and what the hand expects.
Next on the last track goes to the first when repeat is on and stops
otherwise.

The player view's track list is the playlist, live: a drag landing while
the list is open moves the row.

### The queue follows the playlist

A queue playing from a label is that label's playlist, not a copy of it
taken at press time. Drag a clip while the playlist is playing and what
comes next changes to match. Add a clip to the label and the queue grows;
take the label off a clip and the clip leaves the queue.

The playlist is a thing the band agrees on, and the whole point of a
shared order is that changing it changes what happens. A queue that
ignored the drag you just watched land would be the surprising one.

What never changes underneath you is the **track that is playing**. A
reorder moves it to its new place in the queue and playback carries on
untouched; only what follows it is different. The same holds for the clip
playing when its label is removed: it finishes, and the queue continues
from where that clip now sits. Nothing reloads, reseeks or restarts.

Three consequences, spelled out because they are the cases people hit:

- **A clip is dragged while it is playing.** It keeps playing. Its
  position in the queue moves, so "next" now means whatever follows it
  where it landed.
- **A clip is deleted.** If it is the one playing, playback stops, as it
  does today. Otherwise it simply leaves the queue. A clip that vanishes
  between being queued and being reached fails to load and is skipped,
  and a whole pass of failures stops playback with the error rather than
  spinning through a dead list.
- **A label is removed from the clip that is playing.** It finishes. The
  queue continues without it.

The queue is held as the label and a current clip id rather than as an
array with an index, which is what makes all of this fall out: the list
is re-read, the playing clip is found in it by id, and next is the row
after it. An index into a list someone else is editing is the thing that
would need repairing.

### Repeat

Repeat stays the toggle it is today, off or on. What changes is what it
repeats: **the queue**. With repeat on, the end of the last track wraps
to the first instead of stopping.

There is deliberately no third "repeat this one track" mode. A queue of
one track is the common case — every clip page, every unfiltered list —
and there repeating the queue *is* looping the clip, so the way to put a
single take on a loop is to play it from its own page, which is where you
already are when you want that. A mode that exists to reproduce what
another screen already does is a button people have to think about.

The glyph stays `⟳` on the web and `Icons.repeat` on the phone, lit when
on, exactly as now.

**Gaplessness is preserved.** The README is explicit that repeat today is
the media element's own looping because a one-bar riff repeated with a
hole in it is a different sound, and that must not regress. The rule is:

> the media element loops itself when repeat is on **and** the queue
> holds exactly one track.

Otherwise looping is off and the player advances on the end-of-track
event, wrapping to the first track when repeat is on. So every case where
gapless looping is achievable stays gapless — the single-clip case is
bit-for-bit what it is today — and the only case that gets a seam is the
wrap from the last track of a real playlist back to the first, where a
seam is a track change anyway.

Note that this makes the loop flag depend on the queue's length, so it
has to be recomputed when the queue changes and not only when the toggle
moves.

Repeat is remembered between visits exactly as it is now, in the same
`iggybilly.repeat` key with the same `"1"`/`"0"` values. Nothing about
the stored setting changes and there is no migration.

### The web player view

Part of this work is making the web's player pane more like the phone's,
which is the better of the two. The pane gains, in order down the panel:

- the track name, and under it a subtitle of uploader and recording date,
  as the phone's sheet has;
- the waveform and both times, unchanged;
- a controls row of **repeat · previous · play · next · forward**, with
  play as the large filled button in the middle and the skips folded onto
  the outer buttons as they are on the phone. Previous and next are
  hidden, not disabled, when the queue holds one track;
- the "Playing from …" line, expandable into the queue;
- "Open clip", unchanged.

The bar itself is untouched except for the repeat glyph. There is no next
button on the bar, on either client: the bar is one line and the pane is
one press away.

### On the phone

The same, in the phone's idiom. The clip list with one filter chip
becomes a reorderable list; the player sheet gains previous and next
between repeat and the skips, and the "Playing from …" line above
"Keep downloaded".

Offline, the playlist view cannot be opened at all — the clip list is a
request — which is unchanged from today. Playback of an already-loaded
queue continues from the cache exactly as single clips do now: each track
in turn is played from disk if it is there and streamed if it is not, so
a queue of kept clips plays through with no network.

A drag made with no network fails and the list springs back, with the
client's existing "Couldn't reach …" wording.

## Implementation

### Database

`migrations/0005_playlist_order.sql` — number it whatever is next when it
lands.

```sql
-- The clips carrying a label form a playlist, and a playlist has an
-- order. That order is a property of the *membership*, not of the clip:
-- a clip on three labels sits in three playlists and has an independent
-- place in each, so the column belongs on the join table and nowhere
-- else.
--
-- Positions are sparse: a gap of 1024 between neighbours. Moving one
-- clip then writes exactly one row — its new position is the midpoint
-- between the two it was dropped between — so two people dragging
-- different clips in the same playlist do not overwrite each other's
-- work, and a drag costs one UPDATE rather than a rewrite of the list.
-- When a gap is used up (the neighbours are less than two apart) the
-- label's rows are renumbered 0, 1024, 2048, … in the same transaction;
-- see queries::labels::reorder.
--
-- Existing rows are backfilled in the order the filtered list shows
-- them today — newest upload first — so nothing appears to move on the
-- day this ships. From then on the order is whatever people drag it to.
ALTER TABLE clip_labels ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

UPDATE clip_labels
SET position = 1024 * (
    SELECT COUNT(*)
    FROM clip_labels peer
    JOIN clips pc ON pc.id = peer.clip_id
    JOIN clips self ON self.id = clip_labels.clip_id
    WHERE peer.label_id = clip_labels.label_id
      AND (pc.uploaded_at > self.uploaded_at
           OR (pc.uploaded_at = self.uploaded_at AND pc.id > self.id))
);

-- Reading a playlist is "this label's rows, in position order", which is
-- this index; the existing idx_clip_labels_label stays for the filter
-- joins that do not care about order.
CREATE INDEX idx_clip_labels_order ON clip_labels(label_id, position);
```

Positions are not unique and are not required to be contiguous. Ties —
two rows that end up equal through a race — are broken by `clip_id`
everywhere they are read, so the list is always totally ordered even when
the column is not.

### Queries

`src/queries/labels.rs`:

- `add_to_clip` sets `position` to `COALESCE(MAX(position), -1024) + 1024`
  for that label, inside the transaction it already opens, so a newly
  labelled clip lands at the end.
- `playlist(pool, label_id) -> Vec<i64>` — the label's clip ids in
  `(position, clip_id)` order.
- `reorder(pool, label_id, clip_id, after_clip_id: Option<i64>) -> Vec<i64>`
  — the whole of the ordering logic, in one transaction:
  1. read the label's rows in order;
  2. find the target slot — after `after_clip_id`, or first when it is
     `None` — and the positions either side of it;
  3. if the gap is at least 2, write the moved row's position as the
     midpoint and commit;
  4. otherwise renumber every row of the label to `0, 1024, 2048, …` in
     the intended order and commit that;
  5. return the resulting order.

  A `clip_id` that does not carry the label, or an `after_clip_id` that
  does not, is an error rather than a silent no-op — it means the client
  is working from a list that no longer exists.

`src/queries/clips.rs`:

- `list` gains the ordering. Its signature becomes
  `list(pool, active: &[&str], order: ListOrder)` with
  `ListOrder::Recent` (today's `ORDER BY c.uploaded_at DESC`) and
  `ListOrder::Playlist { label_id }` (`ORDER BY cl.position, c.id` on the
  join it already makes). The caller decides, because "is this a
  playlist view" is a question about the request, not about the data.
- `Clip` gains nothing. A clip's position is a property of the list it
  was read as part of, and putting it on the clip would be the per-clip
  ordering this design is explicitly not doing. The index is the row's
  place in the returned `Vec`.

### HTTP

One new route on each surface:

    POST /api/labels/{id}/order
    POST /api/v1/labels/{id}/order

Request:

```json
{ "clipId": 12, "afterClipId": 7 }
```

`afterClipId` is `null` to move the clip to the front. Response is the
label's resulting order, which the client applies rather than trusting
its own optimistic one:

```json
{ "order": [3, 12, 7, 19] }
```

The request says "put this clip after that clip", not "put it at index
4", deliberately. An index is a statement about a list that may have
changed since it was read; a neighbour is a statement about content, and
still means the right thing when someone else has inserted a clip
meanwhile. It is also exactly what a drag produces — the client already
knows which row it was dropped below.

Errors: 404 for a label that does not exist, 400 for a clip that does not
carry it, 400 for `clipId == afterClipId`.

The index page's props gain, on `IndexProps`:

```jsonc
{
  // Present only when exactly one label filter is active and that label
  // exists. Its absence is what the page keys "not a playlist" on.
  "playlist": { "labelId": 4, "labelName": "verse-1", "totalSeconds": 812.5 }
}
```

and each `ClipRow` in a playlist view keeps its array index as its
position; nothing is added to the row shape.

The phone gets the playlist from a route of its own,

    GET /api/v1/labels/{id}/playlist

answering `{labelId, labelName, totalSeconds, clips: [...]}` with the
clips in playlist order and in the shape `/api/v1/clips` already sends
them. `GET /api/v1/clips` is left exactly as it is: making it return an
object instead of an array when it happens to be given one label would
break every existing install the moment the server rolled, for no gain.
The app calls the playlist route when it has one filter and the clips
route otherwise.

### Rust modules

- `src/queries/labels.rs`, `src/queries/clips.rs` — as above.
- `src/handlers/clips.rs` — `list` resolves the single-filter case to a
  label id, chooses the `ListOrder`, and fills `playlist` in `IndexProps`.
- `src/handlers/labels.rs` — the `order` handler.
- `src/api/labels.rs` — the v1 `order` handler and the `playlist` read.
- `src/web.rs`, `src/api/mod.rs` — the two routes.
- `tests/integration.rs` — the backfill order, a move to the front, a
  move to the end, a move into a gap that triggers a renumber, and that a
  clip's move under one label leaves its position under another
  untouched. That last one is the design; it should fail loudly if
  anyone ever puts the column back on `clips`.

### The player, on both clients

The shared model is the same on each side, and it is worth keeping the
names the same:

    Queue { source: string | null, tracks: Track[], currentClipId: number }

`source` is the label name, or null for a queue that came from somewhere
else. The current track is found by id, not held as an index, so a
reorder arriving under a playing queue needs no repair: `tracks` is
replaced and the position falls out. `play(track)` becomes sugar for a
queue of one with no source.

A queue whose `source` is a label re-reads `tracks` whenever that label's
clip list changes — after a local drag, after a reorder response, and
after an add or remove of the label. A clip that leaves the list while it
is the current track keeps playing; it is simply no longer found, and the
queue ends when it does.

**Web** (`web/src/player.tsx`):

- `PlayerContext` gains `queue`, `next`, `previous`,
  `playQueue(tracks, clipId, source)`, `jumpTo(clipId)` and
  `setQueueTracks(tracks)`, the last of which is how a reorder reaches a
  playing queue. `repeat` and `setRepeat` keep their present shape and
  meaning.
- The `finish` handler, which today only sets `isPlaying` to false, gains
  the advance: find the current clip in `tracks`, take the row after it;
  at the end, wrap to the first when repeat is on, otherwise stop.
  Advancing sets the track, which the existing effect turns into a new
  WaveSurfer instance — that is already the one path that loads a track,
  and it stays the only one.
- The looping effect changes from `media.loop = repeat` to the rule in
  "Repeat" above, so it gains `queue.tracks.length` alongside `repeat`
  and `track` in its dependencies.
- The stored setting is untouched: `iggybilly.repeat`, `"1"`/`"0"`, the
  same try/catch.
- `PlayerPane` is rebuilt as described in "The web player view".
- `web/src/pages/index.tsx` — drag handles, the playlist heading, and
  `playQueue` on a row's play button. A drop calls `setQueueTracks` when
  the playing queue's source is this label, so the player and the list
  never disagree. The drag is hand-rolled with HTML5 drag-and-drop on the
  handle; no library.
- `web/src/api.ts` — `reorderPlaylist(labelId, clipId, afterClipId)`.
- `web/src/types.ts` — `PlaylistInfo`, `playlist` on `IndexProps`.
- `web/src/styles/app.css` — `.playlist-head`, `.clip-card .drag-handle`,
  `.clip-card.dragging`, `.pp-queue`, `.pp-queue-row`.

**Phone** (`mobile/lib/src/player/`):

- `PlayerController` gains the queue, with
  `playQueue(List<Clip>, int clipId, {String? source})` and
  `setQueueTracks(List<Clip>)` beside the existing `play`. `play(clip,
  url)` keeps its signature and makes a queue of one, so every existing
  call site is unchanged. Repeat keeps its bool.
- `_onCompleted` grows the advance rule. It currently handles repeat by
  seeking to zero and playing again, with a comment noting that the
  platform's own looping should have meant it never fires — that stays
  for the queue-of-one case, and the advance is the new branch.
- `AudioEngine.setRepeat(bool)` becomes `setLoopCurrent(bool)`, called
  with the "gaplessness" rule's answer rather than with the user's
  toggle, and recomputed when the queue changes as well as when the
  toggle moves. `JustAudioEngine` still maps it to `LoopMode.one` /
  `LoopMode.off`; `LoopMode.all` is never used, because just_audio's
  queue is not the one we are keeping.
- `Settings` is untouched.
- `mobile/lib/src/ui/clips_page.dart` — `SliverReorderableList` when
  there is exactly one filter, the playlist heading, `playQueue` from a
  row, and `setQueueTracks` after a reorder that lands under a queue
  playing from this label.
- `mobile/lib/src/ui/player_sheet.dart` — previous and next in
  `_Controls`, and the "Playing from …" line that expands into the queue.
- `mobile/lib/src/ui/player_bar.dart` — unchanged apart from the queue's
  effect on what next does.
- `mobile/lib/src/api/client.dart` — `playlist(labelId)` and
  `reorderPlaylist(labelId, clipId, afterClipId)`.
- `mobile/test/` — the controller tests are where the interesting rules
  live: advancing at the end of a track, wrapping when repeat is on, the
  loop flag for a queue of one and its recomputation when the queue grows
  past one, a reorder landing under a playing queue without disturbing
  the current track, the current clip leaving the list, skipping a track
  that fails to load, and stopping after a whole pass of failures.

### Caching and offline

Nothing about the `TrackCache` changes. A queue plays each track through
the same path a single clip takes, so a kept playlist plays with no
network and a partly cached one streams the rest.

There is no prefetch of the next track. It would be the obvious next
improvement — fetching track *n+1* while *n* plays — and it is left out
because the cache's eviction policy is "least recently played" and a
prefetch is a play that never happened. Getting that right is its own
change.

Marking a **whole playlist** as "keep downloaded" is the change people
will actually ask for: a set list is exactly the thing you want on your
phone before a rehearsal in a basement. It is left out here on purpose.
Done properly it means keeping the label's membership and the clips'
metadata offline too, not just the audio, and at that point it is the
first step of making the app local-first rather than a playlist feature.
That is issue #24, and it should be designed as a whole.

### Documentation

The root `README.md` "What it does" list gains playlists, and the
paragraph about repeat being the media element's own looping is extended
with the rule above rather than replaced — it is still true, and now it
is true conditionally.
`mobile/README.md`'s "The player" section gains the queue and the same
rule.

## Open questions

- **Ties are broken by clip id.** Two people dragging the same clip to
  the same gap in the same second can produce equal positions, and the
  list is then ordered by id within the tie until someone drags again.
  The alternative is a unique index and a retry loop, which is more
  machinery than a band of five needs.
- **The playlist heading shows total length.** That needs every clip's
  duration, which some clips do not have. Those are excluded from the sum
  and the heading reads "8 clips · 13:32 (2 unknown)", which is honest
  and slightly ugly.
- **No prefetch of the next track**, as above.
- **Drag on the web is hand-rolled.** HTML5 drag-and-drop on a handle,
  with a keyboard path. If it turns out to be fiddly on touch screens the
  answer is a pointer-events implementation, not a dependency.
- **Nothing lets you play a playlist from a label's wiki page or from a
  Discord link.** `/?label=x` is the one entry point. A "play all" button
  in the playlist heading would be the obvious addition and is left out
  until someone misses it.
- **A live queue makes the playing row a moving target.** Reordering the
  playlist you are listening to is meant to work, but two people dragging
  in the same minute will see the queue rearrange under them. If that
  turns out to be unpleasant rather than useful, the smaller fix is to
  keep the queue live for your own drags and re-read on the next track
  boundary for other people's, not to go back to a frozen queue.
