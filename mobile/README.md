# iggybilly for iOS and Android

The same app as the web version, on a phone: browse and filter clips,
play them, upload, rename, label, and read and write the label wiki.

It talks to `/api/v1` on your own iggybilly server — see `src/api/` in
the crate above for that surface, and for why it is separate from the
JSON the React frontend consumes.

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
| `Session` | `CredentialStore` → `SecureCredentialStore` |
| `IggybillyApi` | an `http.Client` |

`test/` fakes all three, and the widget tests in `test/app_test.dart`
drive the real screens against a scripted server — signing in, playing,
renaming, filtering, editing a wiki page, being signed out by a revoked
token.

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
  with the screen off.
- Uploads are read into memory, which is fine because the server caps a
  clip at 10 MB and the picker is limited to the formats it accepts.
