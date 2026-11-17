# MIDI files

Implements [#55](https://github.com/skagedal/iggybilly/issues/55), requested by Jonas.
Complements [003-audio-formats](003-audio-formats.md), whose playback
copy this reuses.

> Part 1 and part 2 shipped separately, on the same day. The spec moved
> here with part 1, and was brought in line with part 2 when it shipped.

Some of what the band shares isn't a recording. A drum pattern
programmed in a DAW, a bass line someone sketched, an arrangement
exported from notation software: these exist as MIDI files, and today
they travel over chat, outside the place where everything else about
the song lives. The point of this spec is to bring them in, so a MIDI
file sits under the same labels, in the same playlists, next to the
recordings of the same song.

The minimum is upload, download and labels, on the web and in the app
alike. Beyond that, a MIDI file is much more useful if you can hear it
without first dragging it into something on a computer, so this spec
also makes MIDI files playable, everywhere audio clips play. A
structural view (tracks, notes, a piano roll) is left for a later spec;
see the end.

The work splits cleanly in two. **Part 1** is storing MIDI files as
clips: upload, download, labels, playlists, delete, on both clients.
**Part 2** makes them play. Part 1 is complete on its own and can ship
first; nothing in it changes when part 2 lands.

## Functionality

### A MIDI file is a clip

A MIDI upload becomes a clip like any other. It has a name (taken from
the filename, as for audio), an uploader, an upload time, labels, a
place in each of its labels' playlists, a clip page, a Download link and
a Delete button for its uploader. It appears in the main list in upload
order, is filtered by labels the same way, and triggers the same Discord
post. Anything that can be done to a clip can be done to it.

The alternative, a separate kind of thing called "files" with labels of
its own, would mean a second list, a second label join, and the question
of whether a playlist contains files. The band thinks in songs, not in
file types, and a label is a song more often than not. One list it is.

The one visible difference everywhere a clip is shown, both in the list
and on the clip page, is a small **MIDI** badge next to the name. It's
there so nobody presses play on a row expecting a recording and gets a
synthesised piano.

### Uploading

The upload pickers on the web and in the app accept MIDI files alongside
audio (`.mid`, `.midi`, and the MIDI types), and several can be picked
together with audio files in one go, as now.

As with audio, the server decides by content, not name. A file is
accepted as MIDI when it is a Standard MIDI File of type 0 or 1, which
is everything a DAW or notation program exports. `.kar` karaoke files
are Standard MIDI Files and are accepted by the same rule. Type 2
(several independent sequences in one file) is refused, as is RMID (MIDI
wrapped in a RIFF container): neither is produced by anything the band
uses, and both would need their own handling. A refused upload gets the
existing message pattern, naming which it was: *"This file is a MIDI
file with several independent sequences (type 2), which we don't
support at the moment. Talk to Simon if you want to change this. Upload
MP3, M4A, AAC or MIDI."*, or *"… a MIDI file in a RIFF container
(RMID) …"*. A file that starts like MIDI but doesn't parse gets *"This
looks like a MIDI file, but it couldn't be read."* The list of accepted
formats at the end of every refusal message gains "or MIDI".

The size cap stays at 10 MB. A large MIDI file is a few hundred
kilobytes.

MIDI files don't carry a recording date, so the clip has none.

### Downloading

On the web, Download works as for every clip: the exact bytes that were
uploaded, under the original filename.

The app has no Download today. It caches clips for playback, but there
is no way to get the original file out of it, which for a MIDI file is
the whole point: the reason to have it on a phone is to open it in
GarageBand, a notation app, or send it on. So the clip page in the app
gains a **Share file** action, which fetches the original upload and
hands it to the system share sheet, from where it can be saved to Files,
opened in another app, or sent. It applies to every clip, not just MIDI:
having it for one kind and not the other would be odd, and sharing the
original of a recording is useful for the same reasons. While the file
is fetched the action shows progress. It always fetches from the
server, so offline it fails with the same message any other request
gives, even for a clip that is kept downloaded.

### Labels and playlists

Exactly as for audio clips, on both clients. A MIDI clip can be dragged
anywhere in a playlist.

### Playing

A MIDI file plays like any clip: press play on its row or clip page, it
goes into the player bar, shows a waveform, can be scrubbed, repeated
and queued, plays through a playlist, keeps playing as you move around,
and is cached and kept downloaded on the phone like any recording.

What you hear is the file played through a General MIDI instrument set
on the server. That is not what it sounds like in Jonas's DAW, and the
clip page says so under the player, once, in small text: *"Played with
a general-purpose instrument set. It will sound different in your own
setup."* General MIDI is the convention nearly every exported file
follows, so the drum track lands on drums and the bass on a bass, which
is what matters for hearing the part.

A MIDI clip that can't be played says so where the waveform would be,
its play button is disabled, and it is left out of a playlist's queue,
so playing through a playlist goes straight past it. In the app it has
no "keep downloaded" switch, as there is nothing to keep. That happens
when the file has no notes (some exports carry only a tempo map), when
it runs longer than 30 minutes, or when rendering it failed. The text
is *"This MIDI file can't be played here."* The badge, labels, Download
and Share file all still work.

Between part 1 shipping and part 2 shipping, every MIDI clip was in that
state, with the text *"Playback for MIDI files is coming."* When part 2
deploys, the existing MIDI clips become playable without anyone doing
anything.

### Older app versions

An app installed before this ships will show MIDI clips in the list
without a badge, and pressing play on one fails the way any undecodable
clip does today: an error in the player, and in a playlist a skip to the
next clip. Nothing crashes, and updating the app fixes it.

## Implementation

### Storing

Detection gains MIDI: a Standard MIDI File starts with an `MThd` chunk
whose header gives the type, and the file is accepted when the type is 0
or 1 and the whole file parses, with [midly](https://github.com/kovaxis/midly).
Parsing the whole file rather than the header is cheap at MIDI sizes and
refuses a file that only starts like MIDI. midly runs in its lenient
mode, which accepts the small irregularities exported files often have,
and with them a track cut short, which keeps the events before the cut.
Refusing those would turn away files every other program opens. RMID is
recognised by its RIFF header and type 2 by the MIDI header, and both
are named in the refusal.

A MIDI clip records its format as `midi` and its content type as
`audio/midi`, and is stored in the audio directory under a `.mid` name
like any upload. Waveform peaks and duration are left empty in part 1.

The served content type comes from the detected format as for audio, so
the existing guarantee holds: nothing uploaded is served as something a
browser would render.

### Serving

Download is unchanged and always serves the original. Streaming serves a
clip's playback copy when it has one and the original otherwise; for a
MIDI clip without a playback copy it answers 404 instead of streaming a
MIDI file to an audio element that can't decode it. The two share a
route today, so the 404 applies only when it is not asked to download.

Both the page props and the app API gain two facts per clip: its format
(so the clients can show the badge) and whether it is playable. A clip
is playable when it has a playback copy or its original is in an audio
format. The app API keeps sending an `audioUrl` for every clip, as
installed apps expect a string there; for an unplayable clip it points
at the 404, which is exactly the failure older apps already handle.

### The app

The badge, the "can't be played" state, the MIDI extensions in the
picker, and the Share file action. The player leaves unplayable clips
out of any queue it is given, rather than discovering them by failing,
and won't start one on its own.

Share fetches the API's existing download URL with the bearer token
into a temporary file named with the original filename, and passes it
to the platform share sheet through `share_plus`. The file is not
deleted when the sheet closes, because on Android the app it was shared
to may still be reading it. Each share clears the previous one's file
instead, and the system clears the temporary directory in its own time.

The web works the same way: the badge, the same text in place of the
waveform, and unplayable clips left out of a playlist's queue.

### Rendering

A MIDI clip's playback copy is the file rendered to audio on the server:
the sequence played through a SoundFont synthesiser into PCM, then
encoded to MP3. This is the transcoded playback copy 003 anticipated;
the playback copy, its content type, the versioned audio URL and the
app's cache handling of a changed URL all work as they do for ADTS, so
nothing downstream of the file knows it came from MIDI. It also gets
waveform peaks and a duration, computed from the rendered audio by the
existing waveform code.

Rendering runs in the server process, as remuxing does. The synthesiser
is [rustysynth](https://github.com/sinshu/rustysynth), pure Rust, which
reads SoundFont 2 files and sequences Standard MIDI Files itself. The
encoder is LAME, compiled into the binary from source; it adds no
runtime dependency to the image. MP3 because every client plays it and
it's the format with a mature encoder that doesn't need ffmpeg. The
render is 44.1 kHz stereo at a constant 192 kbps, with the LAME tag
written into the first frame, so players know the exact length and the
encoder's delay, which a gapless repeat needs.

The render runs on past the last event while the notes' release and
the reverb ring out, until a block is 60 dB below the loudest moment or
five seconds have passed. Reverb takes much longer than that to fall
silent, and seconds of near-nothing at the end of a clip are worse than
a slightly early cut, especially when it repeats.

rustysynth's output peaks well below a recording's, around −12 dBFS on
a full arrangement, so a MIDI clip would come out noticeably quieter
than the recordings around it in a playlist. The render is normalised:
its loudest sample is brought to −1 dBFS, with the gain capped at 8×
so a nearly silent file isn't turned into noise. That needs the peak
before encoding starts, and holding up to thirty minutes of stereo in
memory to find it isn't worth it, so the synthesis runs twice: once to
measure the peak and where the tail fades, and once to encode at the
right gain. Synthesis is deterministic and much cheaper than encoding.

The instrument set is
[GeneralUser GS](https://schristiancollins.com/generaluser.php), a
General MIDI SoundFont of about 30 MB whose licence permits this use. It
is not committed. One script holds its URL, pinned to a commit of the
author's repository, and its checksum. The image build runs it, and so
does `local/run` the first time it's needed. The server finds it through
`IGGYBILLY_SOUNDFONT` and loads it once at startup. Without one
configured, the server starts, logs a warning and stores MIDI clips
unrendered. One that is configured but can't be loaded stops the server,
because quietly leaving every MIDI clip unplayable would be the harder
problem to notice.

Rendering happens during the upload, after detection, as remuxing does.
In a release build, a twenty-second groove renders and encodes in about
a third of a second and a dense five-minute arrangement in about four,
which is acceptable next to the upload itself. The synthesiser and LAME
are optimised in debug builds too, which keeps `local/run` within two
or three times that. A file whose sequence
runs longer than 30 minutes is not rendered at all, since that is
almost certainly a mistake in the file and the render would hold the
upload for a long time. Such a clip, a file with no notes, and one whose
render fails are all stored without a playback copy and logged.

Each MIDI clip also records which renderer looked at it: a short
version string made of a settings number and the start of the
SoundFont's SHA-256. It is recorded whether or not a copy came of it, so
a file with no notes, or one whose render failed, isn't tried again on
every start. The startup pass that 003 introduced goes through MIDI
clips whose recorded version isn't the current one and renders them in
the background, then removes the copy it replaced. That is how the
clips uploaded during part 1 become playable. A new SoundFont re-renders
every MIDI clip without anyone bumping anything, and a change to the
settings does it once the settings number is bumped.

The version is part of the rendering's filename, and the audio URL's
`v` names the file it streams, so a re-render changes the URL and the
app's cache drops its old copy. Doing this showed that `v` had never
worked for 003's copies: it was the first eight characters of the
streamed file's name, and a copy's name starts with the same stem as
its upload. A clip with a playback copy now gets a short hash of the
copy's whole name instead. The one-time effect is that ADTS clips cached
before 003 are fetched again, which is what 003 meant to happen.

## Later: viewing the structure

Not part of this spec, but worth knowing what it would build on. A MIDI
file is small and fully structured, so the obvious view is a piano roll
per track on the clip page, with track and instrument names, tempo and
time signature, drawn on the client from the original file, and a
playhead that follows the player, since the rendered copy is
sample-aligned with the sequence. That needs nothing stored beyond what
part 1 stores. A cheaper first step is a one-line summary under the
title, *"4 tracks: Drums, Bass, Piano, Strings · 120 BPM · 4/4"*, which
the server could extract at upload. Either is a spec of its own once
people have used MIDI clips for a while and know which they want.

## Alternatives considered

- **Play MIDI on the client.** Browsers can synthesise with a SoundFont
  in WebAudio, and the phones have native synthesisers (Android's
  `MediaPlayer` plays MIDI outright; iOS needs a bundled SoundFont).
  That gives three synthesisers that sound different from each other,
  a 30 MB instrument set downloaded by every browser and shipped in the
  app, and a MIDI player alongside the audio player in both clients:
  none of the player bar, queue, scrubbing, waveform, background
  playback or caching would apply without being rebuilt. Rendering once
  on the server makes MIDI just another audio file to both clients.
- **Render with FluidSynth and ffmpeg.** Better-known, and FluidSynth's
  output is excellent. It also puts two large native tools in the image
  for one feature, which 003 declined to do for remuxing.
- **Render in the background after upload.** Keeps uploads fast however
  long the file, but adds a "rendering" state that both clients would
  have to show and refresh. The 30-minute cap keeps synchronous
  rendering short enough not to need it; the background path exists
  anyway, in the startup pass, if that changes.
- **A separate "files" area.** Discussed above: a second list and a
  second labelling, against a band that organises by song.
- **Accept any file type.** Tempting once the server stores non-audio,
  and it would cover lyric sheets and Guitar Pro files too. But every
  type then needs an answer to "what happens when you press play", and
  the content-type guarantee gets harder. MIDI has a good answer; other
  types can come with theirs.

## Open questions

- **SoundFont.** GeneralUser GS is a reasonable balance of size and
  quality. FluidR3_GM is larger and better known; TimGM6mb is a sixth
  the size and sounds it. Worth rendering one of Jonas's files through
  each before choosing.
- **Share file for audio clips.** Added for parity and because it's
  cheap, but nobody has asked for it. It could be MIDI-only if it
  clutters the clip page.
- **The badge.** "MIDI" in text is clear and boring; an icon is
  prettier and less obvious. Text unless someone minds.
- **Type 2 and RMID.** Refused because nobody seems to produce them.
  If Jonas's tools do, accepting them is a detection change, and
  rustysynth would need help with type 2.
