# Recording in the app

Implements [#4](https://github.com/skagedal/iggybilly/issues/4).

Today a clip gets in by being a file first: you record in Voice Memos or
on a handheld, find the file, and upload it. The app is where the band
listens, so it should also be where the band records — press a button in
the rehearsal room and the take is on the server with the rest of them.

The recording joins the normal clip flow at the first opportunity. It is
an ordinary clip the moment it is saved: same table, same audio route,
same waveform, same labels, same Discord post. Nothing downstream of the
upload knows or cares that there was no file.

## Functionality

### Web

The home page keeps its Upload section and gains a Record one above it,
because recording is the more common act and the shorter one.

Collapsed it is a single button, **Record**. Pressing it expands the
panel and asks the browser for the microphone. From there the panel has
four states and never leaves them without the user saying so.

**Armed.** The microphone is granted and a live waveform scrolls across
the panel showing input level, so you can tell the difference between a
working microphone and a muted one before you play anything. A big
**Start** button, and a **Cancel** that closes the panel and releases the
microphone. The last selected input device is not remembered — the
browser's own default is used.

**Recording.** The waveform scrolls, a timer counts up, and there is one
button, **Stop**. The timer turns red at nine minutes and recording stops
by itself at ten (see "Limits"). Nothing else on the page is disabled,
but navigating away is guarded: the router's link interception cannot see
into a `beforeunload`, so an in-app navigation while recording shows a
browser confirm, and leaving the site does too.

**Review.** The recording is loaded but not sent. The panel shows its
waveform drawn from peaks computed in the browser, a play/pause button
that plays it back locally, its length, a **Name** field prefilled with
`Recording YYYY-MM-DD HH.MM` in the browser's own time zone, and three
actions: **Save**, **Record again**, **Discard**. Discard asks first —
the audio exists nowhere else.

**Saving.** The button reads "Saving…" and is disabled. On success the
panel closes and the app navigates to the new clip's own page, which is
where you would go next anyway to label it. On failure the panel stays
in Review with the error under the buttons and the recording intact. A
failed save never loses the audio: retrying is pressing Save again.

Playback and recording do not overlap. Starting a recording pauses the
global player if it is playing, and does not resume it afterwards.

**When recording is not possible.** The Record button is hidden, with one
line of explanation in its place, when `navigator.mediaDevices` is
missing or `window.isSecureContext` is false. The second case is the real
one: the app served over plain HTTP from a LAN address — which is how you
reach a dev server from a phone — cannot record, and saying so is better
than a button that always fails. Upload is unaffected.

**When the microphone is refused.** The three failures are told apart and
worded for what the user can do:

- denied (`NotAllowedError`, `SecurityError`) — "iggybilly isn't allowed
  to use the microphone. Allow it in the browser's site settings and try
  again."
- none present (`NotFoundError`, `OverconstrainedError`) — "No microphone
  found."
- busy or broken (`NotReadableError`, anything else) — "The microphone
  couldn't be started. Another app may be using it."

Each leaves the panel open with a **Try again** button, which asks for
the microphone afresh.

### Phone

The list screen's `+` button opens a two-item sheet: **Record** and
**Upload files**. Upload behaves exactly as it does now; Record pushes a
full screen, since recording is a thing you do rather than a dialog you
answer.

The record screen has the same four states as the web, with the platform
differences that matter:

**Asking.** On the first visit the OS permission prompt appears. Denied,
the screen shows "iggybilly needs the microphone to record" and a button
that opens the system settings for the app — on both platforms a denial
is sticky and the in-app prompt will not come back.

**Armed** shows a level meter and **Start**.

**Recording** shows the timer, a level meter, **Stop**, and keeps the
screen awake. The screen's back gesture is trapped: leaving asks whether
to discard, and discarding deletes the part-file.

**Review** plays the recording back through the same `PlayerController`
the rest of the app uses — a local file is a source like any other — with
a name field, **Save**, **Record again** and **Discard**. Saving uploads,
pops back to the list, refreshes it and shows the clip's own page.

An interruption — a call, an alarm, another app taking the audio session
— stops the recording and moves to Review with whatever was captured.
Stopping and keeping is right: a take that was interrupted at 40 seconds
is still a take, and the alternative is losing it. Headphones being
unplugged does not stop a recording.

If the upload fails, the screen stays in Review with the file on disk and
the error shown; Save can be pressed again. The file lives in the app's
temporary directory and is not resurrected after a restart: a recording
that was never saved is gone when the app is. That is a deliberate limit,
not an oversight — see "Open questions".

### What the resulting clip looks like

Identical to an uploaded one:

- **Name**: whatever was in the name field, put through the same
  uniqueness retry as an upload, so a second `Recording 2026-09-12 18.33`
  becomes `Recording 2026-09-12 18.33 (2)`.
- **Original filename**: the name plus the container's extension. The
  clip page shows it, as it does for an upload.
- **Recording date**: today, in the recording device's own time zone,
  sent explicitly rather than dug out of container metadata.
- **Uploader**: whoever was signed in.
- **Labels**: none. Adding them is the next screen, which is why saving
  goes to the clip page.
- **Waveform**: present. See below — this is the part that takes work.
- **Discord**: the ordinary "uploaded a clip" post. A recording is not
  announced differently.

### The waveform problem

The server computes a clip's peaks at upload by decoding it with
symphonia, and symphonia does not decode Opus. Chrome and Firefox record
to WebM/Opus; only Safari records to MP4/AAC. So on two of the three
browsers a recording would arrive as a clip with no waveform anywhere in
the app, which is most of what a clip looks like here.

The recording side already has the audio decoded — the browser can decode
what it just encoded — so the browser computes the peaks and sends them
with the upload. The server still decodes first and uses its own result
whenever it gets one; the client's peaks are a fallback, used only where
the server has nothing. One source of truth, with a second-best that is
better than a flat line.

The phone has no such problem: the `record` package produces AAC in an
MP4 container on both platforms, which symphonia reads. The phone sends
no peaks.

### Limits

- **Ten minutes.** Recording stops itself at 10:00. Opus at the browser's
  default bitrate and AAC at the phone's both land comfortably under the
  10 MB per-clip cap at that length, and a clip longer than ten minutes
  is a rehearsal recording, which belongs in a file.
- **10 MB**, the existing server cap, checked client-side before the
  request so an oversized recording is refused in the panel rather than
  after a slow upload. A recording that is over the cap keeps its Review
  state; the fix is to record a shorter one.

## Implementation

### Database

None. `clips.peaks`, `clips.duration_seconds` and `clips.recording_date`
already exist and are already nullable, and a recording is not
distinguished from an upload anywhere in the schema.

### The upload endpoints

No new route. Both `POST /api/clips` and `POST /api/v1/clips` are already
multipart with one `audio` part per file; they gain three optional text
parts that describe the *next* `audio` part in the body:

| Part | Shape | Meaning |
| --- | --- | --- |
| `peaks` | JSON array of numbers | Normalised amplitude peaks, used only if the server cannot decode the file |
| `durationSeconds` | decimal string | Length, used only alongside accepted `peaks` |
| `recordingDate` | `YYYY-MM-DD` | Overrides the metadata sniffing in `extract_recording_date` |

The ordering rule is explicit: a metadata part applies to the first
`audio` part that follows it, and is cleared once consumed. A metadata
part with no following file is ignored. This keeps `ingest` a single
forward pass over the multipart stream, which is what lets it stream each
file to disk instead of buffering it, and it is unambiguous for the case
that actually sends them — one recording, one file.

`peaks` is validated, not trusted: at most 2000 values, every value
finite and within `[0, 1]`, or the whole request is a 400 naming the
problem. Values outside that would be stored and handed to every client
that draws the clip. `durationSeconds` must be finite and positive.
`recordingDate` must parse as an ISO date. Rejecting loudly is right —
these come from our own clients, so a bad value is a bug worth seeing.

Success response is unchanged: `{clips: [{id, name}]}`.

### Rust

- `src/handlers/clips.rs` — `ingest` gains the pending-metadata pass
  described above. The `if field.name() != Some("audio") { continue; }`
  at the top of the loop becomes a match that reads `peaks`,
  `durationSeconds` and `recordingDate` with `field.text()` into a small
  `PendingMeta`, ignores anything else, and falls through to the existing
  body for `audio`. Where
  it now writes `recording_date` from `extract_recording_date`, it takes
  the pending value first and falls back to the probe; where it writes
  `peaks`/`duration_seconds` from `crate::audio::compute`, it falls back
  to the pending values when `compute` returned `None`.
- `src/audio.rs` — add `pub fn parse_client_peaks(json: &str) ->
  Result<String, String>` next to `peaks_to_json`: parse the array, check
  the bounds above, and re-emit with `peaks_to_json`, or return the
  message the 400 should carry. Parsing and re-emitting rather than
  storing the client's bytes means the column holds one format whoever
  wrote it, and the existing `Option<Box<RawValue>>` pass-through on the
  read side keeps working unchanged.
- `src/api/clips.rs` — no change. It already delegates to
  `web_clips::ingest`.
- `tests/integration.rs` and `tests/api_v1.rs` — one test each: an upload
  carrying `peaks`/`recordingDate` for a file the server can decode keeps
  the server's peaks and takes the client's date; one carrying them for a
  file it cannot keeps the client's peaks. Out-of-range peaks are a 400.

### Web

`wavesurfer.js` is already a dependency and ships the record plugin in
the same package, so `web/package.yaml` does not change. Import it as
`wavesurfer.js/dist/plugins/record.esm.js`.

- `web/src/components/Recorder.tsx` — new, and the whole feature on this
  side. Holds the state machine (`idle | arming | armed | recording |
  review | saving`), owns one `WaveSurfer` instance with
  `RecordPlugin.create({ scrollingWaveform: true, renderRecordedAudio:
  false })` for the armed and recording states, and tears it down on
  every exit — including unmount, including Discard — so the microphone
  indicator goes out when the panel closes.

  Format selection at arm time, first supported wins:
  `audio/webm;codecs=opus`, `audio/mp4`, `audio/ogg;codecs=opus`, then
  the browser's default. Whatever `MediaRecorder.mimeType` reports is
  stripped of its parameters and mapped to an extension — `audio/webm` →
  `.webm`, `audio/mp4` → `.m4a`, `audio/ogg` → `.ogg`, `audio/wav` →
  `.wav`. An unmappable type is refused before recording starts, since
  the server's allow-list would refuse it after.

  On `record-end` the blob is decoded with `AudioContext.decodeAudioData`
  and reduced to peaks by the same rule `src/audio.rs` uses: mix the
  channels to mono, split into 500 buckets, take the maximum absolute
  sample in each, divide by the largest of those. The result feeds the
  Review waveform and rides along with the upload.

  Review playback is a plain `<audio>` over a blob URL plus the existing
  `Waveform` canvas, not a second WaveSurfer instance — the same reason
  clip rows are canvases.
- `web/src/api.ts` — `uploadRecording(file: Blob, filename: string, meta:
  {peaks: number[]; durationSeconds: number; recordingDate: string})`,
  appending the metadata parts *before* the `audio` part.
- `web/src/pages/index.tsx` — render `<Recorder />` in a new section above
  Upload; on save, navigate to the new clip's page.
- `web/src/player.tsx` — no change. The recorder reaches the player
  through `usePlayer()` to pause it.
- `web/src/styles/app.css` — `.recorder`, `.rec-live`, `.rec-timer`,
  `.rec-actions`, `.rec-review`, following the existing `.upload` /
  `.dropzone` block.

### Flutter

`mobile/pubspec.yaml` gains the `record` package (llfbandit), with a
caret range on whatever major is current when this lands, and nothing
else. It brings the platform recorder on both sides and its own
permission check.

The layout rule holds — logic does not touch a plugin — so the platform
arrives behind an interface, as `AudioEngine` does:

- `mobile/lib/src/record/recorder.dart` — new. `abstract class Recorder`
  with `Future<bool> hasPermission()`, `Future<void> start(String path)`,
  `Future<String?> stop()`, `Future<void> cancel()`, `Stream<double> get
  amplitude`, and `RecordPackageRecorder` implementing it with
  `AudioEncoder.aacLc` into an `.m4a`. Injected into `IggybillyApp`
  alongside `engine`, `cache` and `settings`, and exposed on `AppScope`,
  so a widget test can drive the screen with a fake that writes a fixture
  file.
- `mobile/lib/src/record/recording_controller.dart` — new, plain Dart:
  the state machine, the elapsed timer, the ten-minute stop, and the
  rule that an interruption stops and keeps. Tested without a platform.
- `mobile/lib/src/ui/record_page.dart` — new: the screen described above.
- `mobile/lib/src/ui/clips_page.dart` — the FAB opens the two-item sheet
  instead of going straight to the picker.
- `mobile/lib/src/ui/upload.dart` — extract the "upload these files and
  report how many landed" half so the record screen reuses it rather
  than growing a second upload path.
- `mobile/lib/src/api/client.dart` — `upload` takes an optional
  `recordingDate` per file and sends it as a sibling part. `peaks` is not
  sent from the phone.
- `mobile/ios/Runner/Info.plist` — `NSMicrophoneUsageDescription`,
  worded as "iggybilly records audio clips for your band."
- `mobile/android/app/src/main/AndroidManifest.xml` — `RECORD_AUDIO`.
- `mobile/test/` — a fake `Recorder`, and a widget test that records,
  reviews and saves against the scripted server.

The recording is written into the temporary directory, not the support
directory the `TrackCache` uses. The cache holds clips that exist on the
server; an unsaved take does not, and letting the two share a directory
would make the reconcile pass on startup have an opinion about it.

### Caching and offline

Nothing changes. A recording is uploaded, and the clip it becomes is
cached by the same rules as any other the first time it is played. There
is no offline save queue: a save with no network fails, says so, and
keeps the audio in the Review state for as long as the screen is open.

### Documentation

The root `README.md` "What it does" list gains a line about recording in
both clients, and the environment section is untouched.
`mobile/README.md` gains a short section on the recorder alongside "The
player", covering the interface-behind-a-plugin shape and the
interruption rule.

## Open questions

- **Ten minutes** is a guess at what a band records in one press. If
  takes routinely run longer, the cap that actually binds is the server's
  10 MB, and the recorder should show a size estimate instead of a
  length limit.
- **Nothing marks a clip as recorded in the app.** No column, no badge,
  no different Discord wording. If it turns out to be useful to see which
  clips came from a phone in the room, that is a `clips.source` column
  and a line in the Discord message, and neither is hard to add later.
- **An unsaved recording does not survive the app being killed.** Making
  it survive means an inbox of orphaned part-files with their own screen
  and their own eviction rules, which is a feature of its own.
- **No input device picker on the web.** The browser's default
  microphone is used. A band with an interface plugged in may want to
  choose; `enumerateDevices` makes it easy, but it is a control nobody
  has asked for yet.

## Alternatives considered

- **A dedicated recording endpoint.** A clean, single-file contract. But
  it would duplicate `ingest`, and a recording is supposed to be
  indistinguishable from an upload.
- **Decoding Opus on the server.** One source of peaks for every clip.
  But symphonia has no Opus decoder, so it means libopus or ffmpeg as a
  native dependency of the server.
- **Transcoding recordings on the server.** Every clip would be in a
  format symphonia reads. But it needs ffmpeg in the image, and changes
  the bytes the band recorded.
- **Recording MP4/AAC in every browser.** The server could decode every
  recording. But not every browser's `MediaRecorder` produces it, so the
  web would still need a fallback.
- **Always using the client's peaks.** No decoding on the server for
  recordings. But the server's peaks are the source of truth wherever it
  can compute them, and a client bug would then reach every viewer.
- **Metadata as one JSON part or query parameters.** Simpler to parse.
  But mapping it to files by index is more fragile than "applies to the
  next file", and ordered parts keep `ingest` a single streaming pass.
- **A second WaveSurfer instance for review playback.** Consistent with
  the recording view. Heavier than an `<audio>` element and the existing
  canvas, as with clip rows.
- **Keeping unsaved recordings across restarts.** No take is ever lost.
  But it needs an inbox of orphaned files with its own screen and
  eviction, which is a feature of its own.
- **An offline save queue.** Record in the basement, upload later. Its
  place is [local-first](local-first.md), and it changes what an unsaved
  recording means.
- **Writing the phone recorder against platform APIs directly.** No
  plugin dependency. Two native implementations to maintain, where the
  `record` package already covers both.
