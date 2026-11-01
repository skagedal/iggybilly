# Audio formats

Implements [#49](https://github.com/skagedal/iggybilly/issues/49).

A clip is only worth uploading if everyone in the band can play it, on
whatever they happen to be holding. Today that is luck. The server
decides what a file is from its name, stores the bytes as they came, and
serves them with the content type the name implied. When the name is
right and the format is one every client plays, which has so far been
almost always, it works.

It doesn't work for AAC uploaded without a container. Some Android
recorders produce ADTS, the Audio Data Transport Stream: AAC with no
container, a bare run of frames, each with a short header of its own
giving the sample rate, channels and frame length. It was made for
broadcast and streaming, where a listener can tune in mid-stream and
start decoding at the next frame. The price is that nothing in the file
says how long it is or where any given second starts, which an MP4
container records up front. Name such a file `.m4a`, as some
of them do, and it is served as `audio/mp4` with `nosniff`. Chrome and
Android's ExoPlayer look at the bytes, see AAC and play it. Safari and
AVPlayer, both in the browser and in the app, believe the content type,
look for an MP4 container that isn't there, and refuse. The name isn't
a reliable hint either way: the same recorder produces `.aac.m4a` and
plain `.m4a`, which rules out reading the extension more carefully as
a fix. symphonia does read ADTS, so these clips still get waveforms
with correct durations.

Fixing this needs no transcoding. The audio is already AAC, which
every client decodes; the frames only need to be put in an MP4
container, and the bytes of the audio do not change. That is a remux.

## Functionality

### What can be uploaded

An upload is accepted when its **content** is one of these, whatever the
file is called:

| format | typical names | web (Chrome, Firefox, Safari) | iPhone app | Android app | stored and played as |
|---|---|---|---|---|---|
| [MP4](https://en.wikipedia.org/wiki/MP4_file_format) with [AAC](https://en.wikipedia.org/wiki/Advanced_Audio_Coding) | `.m4a`, `.mp4` | yes | yes | yes | as uploaded |
| [MP3](https://en.wikipedia.org/wiki/MP3) | `.mp3` | yes | yes | yes | as uploaded |
| [ADTS](https://en.wikipedia.org/wiki/Advanced_Audio_Coding#Container_formats) AAC | `.aac`, often `.m4a` | unreliably in Safari | no | yes | remuxed into MP4 |

Anything else is refused at upload with a message naming what it was,
saying it isn't supported for now and listing what is: *"This file is
Ogg Vorbis, which we don't support at the moment. Talk to Simon if you
want to change this. Upload MP3, M4A or AAC."* When the content matches
nothing known, the first sentence is *"This isn't an audio file we
recognise."*

That refuses WAV and FLAC, which every client plays but which take
several times the disk space of the same recording in AAC or MP3, for
no difference anyone hears through a phone speaker. It also refuses Ogg
(Vorbis or Opus), WebM, AMR/3GP and MP4 with ALAC, which some client
can't play. Making those playable means transcoding, which is left out
for now; see Alternatives considered. The current extension allow-list
accepts several of these, but none of the clips on the server is in any
of them.

A file whose name and content disagree, such as `.m4a` holding ADTS, is
accepted and treated as what its content is. A file whose name says one
accepted format and whose content is a refused one, such as `.mp3`
holding WAV, is refused for its content. The uploader isn't told about
a mismatch that was accepted: they did nothing wrong, and their phone
named the file.

The upload pickers offer the accepted formats only, so a file that
would be refused can't usually be chosen. The server still decides,
because a name can't be trusted.

### What the band sees

For MP4 and MP3, nothing changes. The clip appears in the list, has its
waveform, plays on every client, and can be downloaded.

For an ADTS upload, the only visible difference is that it now plays on
iPhones. Playing streams an MP4 copy made at upload. **Download** still
gives back the exact bytes that were uploaded, under the original
filename, as the README promises. The band's file is never replaced,
only accompanied.

Upload takes as long as it does now. Remuxing a ten-minute AAC file is
copying its frames into a new file, a few milliseconds of work next to
the upload itself.

### The clips already uploaded

Clips already stored as ADTS are fixed without anyone doing anything:
the server checks every existing clip when it starts and remuxes the
ones that need it. After the deploy they play on iPhones. Existing
clips in a format that is now refused stay as they are; there are none
today, and the server logs any it finds.

On an iPhone whose app has already cached one of those clips, the cached
copy is the broken one. It fails on first play, the app drops it and
streams the fixed copy from the server, and caches that instead.

## Implementation

### Detecting the format

The server identifies a file by its first bytes, after it has been
written to disk and before the clip is created. The extension and the
uploader's `Content-Type` play no part. Each format has a recognisable
signature: MP4 starts with an `ftyp` box, MP3 with an ID3 tag or an MPEG
audio frame header, ADTS with its own frame header. An MP4 is only
accepted when its audio track is AAC, which means reading the track
list, not just the first bytes.

ADTS and MP3 frame headers look alike, and both can follow an ID3 tag,
so the detection looks past an ID3 tag and checks that a second frame
header follows the first where the first says it should before
deciding. Random bytes that happen to start like a frame header are
then not mistaken for audio.

Refused formats with a recognisable signature (Ogg, WebM, AMR, WAV,
FLAC) are named in the refusal message. Everything else gets the
generic one.

Detection gets tests with a few seconds of real audio in each format,
accepted and refused.

The content type a clip is served with comes from the detected format
(`audio/mp4`, `audio/mpeg` or `audio/aac`) and never from the uploader,
which keeps the reason the current allow-list exists: nobody can upload
something the browser would treat as a page. The served type now also
matches the bytes.

### Remuxing ADTS

When the detected format is ADTS, the server writes a second file next
to the original: the same AAC frames in an MP4 container, with the
duration and an index of every frame, so players know the length and
can seek. The index goes before the audio, so a player has it after
the first request. It is done in Rust in the server process, without
ffmpeg, as the waveform decoding already is.

A stream that changes sample rate or channel layout partway can't go
into one MP4 track. No phone produces that; if one does, the clip is
stored without a copy and behaves as it does today, playable everywhere
but on iPhones, and the server logs it.

### What each clip records

Each clip records the format detected from its bytes and, when it has
one, which file is its playback copy and that copy's content type. The
existing content type comes to mean the original upload's type, so the
ADTS clips stored as `audio/mp4` are corrected to `audio/aac`. Files
already on disk keep their names.

Deleting a clip deletes its playback copy too.

### Serving

Playing a clip streams its playback copy when it has one and the
original otherwise. Download always serves the original, with its
original filename.

The audio URL handed to the app identifies the file being played, so it
changes when a clip gains a playback copy. That is what
makes the app notice that its cached copy is stale. The content type the
app is told about is that of the file it plays, so its local copy gets
the `.m4a` name AVPlayer needs.

### The app

The app's cache remembers which of a clip's files each copy is. When
the server's audio URL names a different one, the old copy no longer
counts, the clip streams, and the next download replaces it. A clip
marked "keep downloaded" stays kept through that.

Copies cached before the app recorded this are still used: for almost
every clip the bytes are the same. A cached ADTS copy, the one that
doesn't play on an iPhone, fails on first play, and the player already
answers that by forgetting the copy and streaming from the server,
which now serves the MP4.

### Clips already on the server

At startup the server goes through every clip whose format hasn't been
detected yet, detects it, makes a playback copy where one is needed and
records the result. It runs in the background, does nothing for clips
it has already seen, and needs no command to be run. A clip whose file
is in a refused format is logged and left alone.

## Alternatives considered

- **Serve ADTS as `audio/aac`.** One line, no copy. But bare ADTS has
  no index, so the duration is estimated (ffprobe's estimate for one
  3:09 recording is 4:45), and seeking is approximate, in whatever
  player gets it.
- **Remux in place and drop the original.** Simpler. But the Download
  promise is "the original upload bytes", and keeping that promise
  costs one extra file per ADTS clip.
- **Transcode to one format with ffmpeg.** Would make Ogg, WebM, ALAC,
  WAV and FLAC uploads playable everywhere too. Left out for now for
  simplicity: it adds ffmpeg to the image, and nobody has uploaded any
  of those formats. The playback copy introduced here is where a
  transcoded file would go, so adding it later is a new way of making
  that copy, not a new design.
- **Shell out to `ffmpeg -c copy` for the remux.** Lossless, and handles
  every edge case. Still a large native dependency for a format that is
  simple to parse.
- **Sniff on the client before upload.** The app and the web would both
  need it, and the server would still have to check. The server is the
  one place that sees every file.

## Open questions

- **ALAC.** Refused because Chrome and Firefox can't play it. Voice
  Memos' lossless setting would produce it. It could be accepted with a
  transcoded AAC copy once transcoding exists.
- **Telling the uploader.** A name/content mismatch is silent. If an
  Android recorder is misnaming files there's nothing for the uploader
  to change, so silence seems right, but a line in the server log helps
  with the next "why won't this play".
