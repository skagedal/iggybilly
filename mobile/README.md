# iggybilly for iOS and Android

The same app as the web version, on a phone: browse and filter clips,
play them, upload, rename, label, and read and write the label wiki.

It talks to `/api/v1` on your own iggybilly server — see `src/api/` in
the crate above for that surface, and for why it is separate from the
JSON the React frontend consumes.

## The player

One `PlayerController` (`lib/src/player/`) lives above the navigator, so
walking from the list to a clip to a wiki page never interrupts playback.
Tapping the bar raises the whole player: the full waveform, both times,
ten-second skips, a repeat toggle, and the switch that keeps the clip on
the phone.

Repeat is `LoopMode.one` on the platform player rather than a seek when
the clip ends. That makes it gapless on both platforms, which is the
whole point when the clip is a two-bar riff.

Two things in here exist because of specific bugs, and are worth knowing
about before either is "simplified":

- **`RecoveringAudioEngine`** throws the platform player away and builds a
  new one when a load fails. A `just_audio` player that has failed once
  can stay unusable for every source after it, which showed up as: press
  play, the bar flashes up and vanishes, and every clip behaves that way
  until the app is force-quit. Force-quitting worked because it was the
  only thing that built a new player.
- **The generation counter in `PlayerController.play`.** Loading is
  asynchronous and can fail long after the fact, so a load that has been
  superseded must not write state for a clip that is no longer on screen.
  Without it, pressing play on a second clip while the first was still
  loading let the first one's failure unload the second.

## Clips on disk

`TrackCache` (`lib/src/cache/`) keeps two kinds of thing in one
directory under the app support directory:

- **Cached clips.** Playing a clip copies it here in the background — the
  first play still streams, so nothing waits on a download. Once the
  total passes the ceiling set on Account → Downloads, the clip nobody
  has played for longest is deleted.
- **Kept clips.** "Keep downloaded", on a clip or in the player,
  downloads it now and keeps it until that is turned off. Kept clips are
  *not* counted against the ceiling and are never evicted: a budget for
  what the app may guess at should not be spent on what was asked for.

A clip already on disk plays from the file, with no request at all. If the
local copy turns out not to play — truncated by a crash, say — it is
forgotten and the clip is fetched from the server instead, so a bad file
costs one slow play rather than a clip that never works again.

The support directory rather than the cache directory, deliberately: iOS
may empty `Library/Caches` whenever it likes, and a clip the user asked to
keep is the one file here that must not vanish. The cost is that the
directory is included in backups.

Everything about the cache is best-effort. No writable directory, a failed
download, a full phone: each of those means the clip streams, which is
what the app did before any of this existed.

## Playing in the background

Both platforms are set up for it: `UIBackgroundModes: audio` in
`ios/Runner/Info.plist`, and `AudioSessionConfiguration.music()` on the
audio session.

Configuring the session is only half of it, though, and the other half is
what made clips stop mid-listen with the phone in a pocket. A call, an
alarm, a navigation instruction or another app taking the session pauses
the platform player, and *nothing starts it again unless the app does*.
`JustAudioEngine` now answers `interruptionEventStream`: it ducks for a
duck, pauses for a pause, and starts again afterwards if it was the one
playing when the interruption began — but never after an interruption the
platform describes as possibly indefinite, because that is how two apps
end up playing at once. It answers `becomingNoisyEventStream` too, so
pulling headphones out pauses rather than switching to the phone's own
speaker.

Fetching audio is *not* what stops playback on iOS: an app with the audio
background mode that is producing audio is allowed to use the network.
Android is the weaker side — there is no foreground service, so the OS is
free to freeze the process once the app is backgrounded. Playing from a
cached file removes the network from the question, but the process itself
is still at the OS's discretion. The fix, when it is wanted, is
`just_audio_background` (a foreground service plus lock-screen controls);
the manifest already carries the `FOREGROUND_SERVICE`,
`FOREGROUND_SERVICE_MEDIA_PLAYBACK` and `WAKE_LOCK` permissions it needs.

## Running it

The Flutter SDK is pinned in `.fvmrc` and managed with
[fvm](https://fvm.app), so run Flutter through `fvm`:

    fvm install
    fvm flutter pub get
    fvm flutter run

The first screen asks for a server, a username and a password. Point it
at a local server — `http://10.0.2.2:9020` from the Android emulator,
`http://localhost:9020` from the iOS simulator — and sign in with a user
made by `./local/run create-user <name>`.

## On a real phone

`../local/build-to-phone` builds and installs, with no Xcode involved:

    ../local/build-to-phone            # release
    ../local/build-to-phone --debug

It needs `local/device.env`, which says which phone and which Apple team
to sign with. That file is gitignored, because this repository is public
and those values are personal — copy `local/device.env.example`, or
symlink your own from wherever you keep such things. The team reaches
Xcode through a generated `ios/Flutter/Signing.xcconfig`, also gitignored,
which `Debug.xcconfig` and `Release.xcconfig` include optionally so
simulator builds work without it.

Point the app at your machine, by LAN address or by Bonjour name — both
work, and `scutil --get LocalHostName` gives you the latter:

    http://192.168.1.20:9020
    http://your-mac.local:9020

Two things make this work, and both are in `Info.plist`. iOS blocks
cleartext HTTP, so `NSAllowsLocalNetworking` opens it for local
destinations only. And iOS asks the user once for local network
permission, on the app's first request rather than at launch, showing
`NSLocalNetworkUsageDescription` when it does.

If it still cannot connect right after you grant that permission, quit
and reopen the app: the grant does not reliably reach a process that is
already running. Testing the same URL in Safari is not a check on any of
this — Safari does not use the app's ATS policy, so it will load an
address the app cannot.

## Checks

    fvm flutter analyze
    fvm flutter test

Both run in CI. `analyze` must be clean: fix the lint rather than
silencing it.

## How it is put together

The rule the layout follows is that **logic does not touch a plugin**.
Platform channels do not answer under `flutter test`, so anything that
reaches one directly cannot be tested, and the interesting decisions in
this app — what pressing play on the already-loaded clip does, whether a
failed request means "sign in again" or "you are on a train" — are
exactly the ones worth testing.

So each platform thing arrives behind an interface, injected by
constructor:

| Plain Dart | The platform behind it |
|---|---|
| `PlayerController` | `AudioEngine` → `JustAudioEngine` |
| `RecoveringAudioEngine` | another `AudioEngine` |
| `Session` | `CredentialStore` → `SecureCredentialStore` |
| `TrackCache` | a `Directory` it is handed, and an `http.Client` |
| `PlayerController` | `Settings` → `PrefsSettings` |
| `IggybillyApi` | an `http.Client` |

`RecoveringAudioEngine` is on that list for a reason: taking the platform
as a factory for another `AudioEngine`, rather than reaching for
`AudioPlayer` itself, is what makes "a failed load is retried on a new
player" a test rather than a hope. The cache takes its directory as a
`Future<Directory> Function()` for the same kind of reason — the real one
comes from `path_provider`, a test's comes from `systemTemp`.

`test/` fakes all of them, and the widget tests in `test/app_test.dart`
drive the real screens against a scripted server — signing in, playing,
opening the player, renaming, filtering, editing a wiki page, being
signed out by a revoked token.

**The player is one object, above the navigator.** `PlayerBar` sits
outside the `Navigator` in `ui/app.dart`, so walking from the list to a
clip to a wiki page never interrupts playback. This is the same reason
the web version moved its player out of the router's outlet.

**Waveforms are drawn, not played.** A row paints the peaks the server
computed at upload (`ui/waveform.dart`). Thirty clips is thirty paints,
not thirty media players.

**The token is a credential.** It lives in the iOS keychain and Android's
encrypted storage, never in preferences; the server address and the last
username, which are not secrets, live in preferences. Each install has
its own token, so one lost phone is one row to revoke on the account
screen rather than a password change that signs out everything.

## Platform notes

- iOS declares the `audio` background mode and Android takes the
  media-playback foreground-service permissions, so a clip keeps playing
  with the screen off. See "Playing in the background" above for what
  that does and does not cover.
- The local copy of a clip is given the extension its content type
  implies. AVPlayer takes the extension of a local file as its first hint
  at the container and gets less forgiving the less it has to go on.
- Uploads are read into memory, which is fine because the server caps a
  clip at 10 MB and the picker is limited to the formats it accepts.
