# Seeing what's in a MIDI file

Follows [004-midi-files](../implemented/004-midi-files.md), whose "Later"
section this picks up.

> **Draft: alternatives, not yet a decision.** This lays out the ways to
> show a MIDI clip's structure, and recommends one. Pick one first, then
> this becomes a proper spec with the Implementation section filled in.

A MIDI clip now plays like any recording, but the clip page shows it as
a recording too: a waveform of the render. That hides the one thing a
MIDI file has over a recording. It says exactly what is in it: which
parts there are, when each comes in, and the notes. "Where does the
bass come in?" or "what does the lead play in bar 6?" should take a
glance, not opening the file in a DAW.

So, two views, roughly in order of usefulness:

- **Arrangement.** One lane per part, labelled with the track's name
  and instrument, with a block in each bar where that part plays. Bar
  numbers along the top. It reads like a DAW's arrange window: drums
  from bar 1, lead from bar 5.
- **Piano roll.** One part's notes, pitch against time, with bar lines.
  Opened by picking a lane in the arrangement.

Either view has a playhead that follows the player, and tapping or
clicking in it seeks. This works because the rendered MP3 starts at the
same moment as the sequence (004, part 2), so a time in seconds means
the same thing in both.

There are four ways to get there. They aren't exclusive: the
recommendation at the end combines two.

## A. Link to Signal as it's hosted

[Signal](https://github.com/ryohey/signal) (MIT, React, actively
developed) is a full MIDI editor in the browser, hosted at
signalmidi.app. It has arrangement and piano-roll views, a track list,
a tempo editor, and playback through a SoundFont of its own.

Its app can open a file given in the URL, `?open=<url>`. That makes the
cheapest possible version an "Open in Signal" link on the clip page.
But two things rule it out as it stands:

- **The URL must be public.** Our audio URLs need a session cookie or a
  bearer token. We'd have to mint an unauthenticated, expiring link for
  each open.
- **The file goes to Signal's cloud.** `?open=` doesn't fetch in the
  browser: it hands the URL to Signal's Firebase function, which
  downloads the file and stores it there. The band's unreleased
  material would end up on a third party's server, every time someone
  looks at it.

The second point alone is enough. Without a link, people can already
download the file and open it in Signal by hand. A line saying so on the
clip page costs nothing, and the choice to upload stays theirs.

## B. Host our own build of Signal

Build Signal from a pinned commit and serve it from iggybilly, at
`/signal/`. It's same-origin, so it can fetch the clip's file with the
user's cookie and nothing leaves the server.

**What it takes:**

- A git submodule pinned to a Signal commit, built in a new
  `main.Dockerfile` stage. Signal's build needs Node and pnpm; the image
  gets only the static output.
- A small patch to Signal that we keep ourselves:
  - Add a `?src=<path>` parameter that fetches the file from our origin
    directly, beside the `?open=` that goes through its cloud.
  - Have it start without Firebase config. Signal reads six
    `FIREBASE_*` values at build time; everything cloud (sign-in,
    publishing, `?open=`) should switch off when they're missing.
    Whether it already copes with that is the first thing to find out.
- On the web, an "Open in Signal" button on a MIDI clip's page. It
  opens in a new tab: Signal is a full-screen app, not something to fit
  inside the clip page.

**What we'd get:** everything Signal has, more than we'd ever build
ourselves. That includes editing. Saving an edit back into iggybilly,
as a new clip or a new version of this one, would need more patching,
and is a question for later (see Open questions).

**What it costs:**

- **No parity.** Signal is built for a desktop browser with a mouse. In
  the app it would be a WebView, which would also need the bearer
  token passed in, and on a phone it would be hard to use. Mobile gets
  nothing better than it has today.
- **The patch rots.** Signal is mid-refactor (MobX to Jotai, see its
  `ARCHITECTURE.md`), so moving to a new commit will mean redoing the
  patch now and then. If we never update, we're stuck on an old
  version.
- **A second front-end toolchain in the build.** Signal is a pnpm and
  Turbo monorepo built with Vite, and this would add a few minutes to
  the image build.
- **Playback that doesn't match.** Signal plays through its own
  SoundFont in the browser, so what you hear there differs from the
  render everyone else hears. That's fine for an editor, but there are
  two players for the same clip.

**Reusing Signal's parts instead of the whole app** sounds attractive,
but isn't on offer. Its `packages/` (`core`, `player`,
`arrange-editor`, `pianoroll-editor`) are workspace-internal and not
published to npm. They're editing logic, and the canvases we'd want
live in `app/src/features/`, tied to Signal's app state. Taking them
would be a fork in all but name.

## C. Our own read-only views

Draw the two views ourselves, in both clients, from data the server
extracts.

**Server.** midly already parses every MIDI upload. A new endpoint,
`GET /api/v1/clips/{id}/structure` (and the same data in the clip
page's props on the web), returns:

- **Parts.** Each part is a (track, channel) pair that has notes. A
  type 0 file is one track carrying every channel, so splitting by
  channel is what turns it into parts. Each part has:
  - a name (the track name, or the General MIDI instrument if the track
    has none),
  - its program number,
  - whether it's drums (channel 10),
  - its notes, as `[start_seconds, duration_seconds, pitch, velocity]`.
- **Timing.** The tempo and time-signature changes, the duration, and
  the start time of each bar in seconds, so clients never deal with
  ticks.

It's computed per request, not stored. Parsing is fast and the file
never changes, so an `ETag` on the clip's storage filename makes repeat
loads free, and no migration is needed. Ticks to seconds is the same
calculation rustysynth makes from the same tempo map, so notes line up
with the render. A test should check that against a render. Files
timed in SMPTE frames rather than beats are rare from DAWs; they get no
structure, and the clip page looks as it does today.

**Web.** A canvas component on the MIDI clip page, under the waveform,
like the waveform rows already are: arrangement first, and a piano roll
for the part you pick. The playhead follows `player.currentTime`.

**App.** The same two views with a `CustomPainter`, on the clip page.
It scrolls horizontally; pinch to zoom can come later.

**Cost.** A day or two on the server and around the same per client.
It all stays in our own code, with no new dependencies and no
toolchain. It never edits anything, and it doesn't try to.

## D. A drop-in web component

`html-midi-player` has a `<midi-visualizer>` with piano-roll and
waterfall views, and `webaudio-pianoroll` is a small piano-roll element.
Either would give the web a view in an afternoon. Both are web-only, so
there's no parity. Both parse the file in the browser, and neither has
an arrangement view. `html-midi-player` is also built on Magenta.js,
which is heavy for what we'd use, and `webaudio-pianoroll` is an editor
that we'd lock into read-only. Neither is better than C on the web, and C has to
exist anyway for the app.

## Summary line, any time

Whichever we pick, the cheapest useful thing is one line under a MIDI
clip's title, in the list and on the clip page:

> 4 parts: Drums, Bass, Piano, Lead · 100 BPM · 4/4 · 8 bars

It comes from the same extraction as C, so it can ship first, on both
clients, before any canvas exists.

## Recommendation

1. **The summary line first**, with the server half of C behind it.
2. **C's arrangement view, then its piano roll**, on both clients. This
   is the view people will use every day, and it keeps the parity 004
   set out to have.
3. **B later, if anyone wants to edit.** Treat it as an experiment: a
   spike to see whether Signal runs from a static build without
   Firebase, and how big the patch is. If it's small, "Open in Signal"
   on the web is a cheap extra on top of C, not a replacement for it.

A is out because the file would leave our server. D is out because it
serves the web only and C has to exist anyway.

## Open questions

- **Does anyone want to edit?** That's the question that decides
  whether B is worth its upkeep. If it's mainly looking and listening,
  C is the whole answer. If Jonas wants to fix a bass line and share it
  back, saving from Signal into iggybilly becomes a spec of its own. Is
  an edit a new clip, a version of this one, or something else?
- **Waveform or arrangement on the clip page?** For a MIDI clip, the
  arrangement says more than the waveform. Should it replace the
  waveform as the thing you scrub on, or sit below it? The list rows
  keep the waveform either way: they're too small for anything else.
- **Mute or solo parts?** Muting the drums to hear the bass is the
  natural next thing to want from an arrangement view. It needs a
  render per combination, or playback on the client, so it's out of
  scope here. It's worth knowing it's the next thing people will ask
  for.
- **Instrument names.** Do we use General MIDI program names ("Electric
  Bass (finger)") when a track has no name, or the program's family
  ("Bass")? The family is shorter, and usually what you want in a lane
  label.
