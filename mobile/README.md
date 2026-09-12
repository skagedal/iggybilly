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

It needs `local/devices.env`, which says which phone and which Apple team
to sign with. That file is gitignored, because this repository is public
and those values are personal — copy `local/devices.env.example`, or
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

## Releasing

Push a `mobile-` tag and both apps ship from that commit:

    git tag mobile-0.2.0 && git push origin mobile-0.2.0

`.github/workflows/release.yml` builds the Android APK and attaches it to
the tag's GitHub release, where anyone can download it without an account
or an app store, and builds the iOS app and uploads it to TestFlight.

The tag is the only place the version is written. `--build-name` comes
from the tag and `--build-number` from the run, so a release needs no
commit of its own and `pubspec.yaml`'s version is only what a local build
gets. A tag must be `mobile-` and one to three integers: the workflow
refuses anything else up front, because App Store Connect would refuse it
at the end of a long build. The prefix is the point of the name — the
server and the frontend ship from `main` on every push and carry no
version of their own, so an unprefixed tag would look like it spoke for
the whole repository.

The two jobs are independent, so the APK is published even when the Apple
side fails, and the other way round.

### Setting up the Android key

Once, on your machine:

    ../local/make-release-keystore

It writes a keystore outside the repository and a gitignored
`android/key.properties` pointing at it, and prints the two commands that
give CI the same key. Back the keystore up before anything else. Android
knows an app by its signature, so if that file is lost, every friend who
has the app has to delete it before they can install another build — and
the copy in `key.properties` is the only other one.

Nothing else needs an account: there is no Play Console in this, and no
fee. Your friends allow installs from their browser once, tap the APK on
the release page, and that is the whole flow. What they do not get is
automatic updates, so a new release is a link you send them.

### Setting up the Apple side

It needs the Apple Developer Program, which is the yearly fee. There is
no free path onto someone else's iPhone that they would thank you for.

Two things have to be done by hand first, because App Store Connect has
no API for either:

1. **An App Store Connect API key**, under Users and Access →
   Integrations. Give it the **Admin** role. App Manager is enough to
   upload builds, but not to have a certificate issued, and the script
   below asks for one. The `.p8` downloads once and never again.
2. **The app record** for `tech.skagedal.iggybilly` in App Store
   Connect. An upload has nowhere to land until it exists.

Then point a config file at that key and run one script:

    cp local/appstore.env.example local/appstore.env
    $EDITOR local/appstore.env
    ./local/make-ios-signing

It generates a private key and a signing request, has App Store Connect
issue the Apple Distribution certificate against it, bundles the two into
a `.p12`, registers the bundle id if it is new, makes the App Store
provisioning profile, and sets all seven iOS secrets. No Xcode, no
Keychain Access, no developer portal.

Back up `~/.apple-signing` afterwards, the way you backed up the
keystore. The private key is in there and nowhere else, and a certificate
whose private key is gone is a dead letter.

Then turn the job on:

    gh variable set IOS_RELEASE --body enabled

Until you do, the iOS job does not run and every release page says so —
shipping Android before the Apple side exists should be a green release
rather than half a red one, but a half-built pipeline that quietly ships
one platform is worse than one that fails.

Run `make-ios-signing` again when the profile expires, which is once a
year. It replaces the profile and leaves the certificate alone, which is
the point: an account is allowed very few distribution certificates, and
asking for another while the one you have still works is how you run out.

### What ends up in the repository's secrets

Nine, and nothing should be pasted anywhere it can be scrolled back to.
The two scripts pipe rather than print, for that reason.

| Secret | What it is | Set by |
| --- | --- | --- |
| `ANDROID_KEYSTORE_BASE64` | the keystore, base64 | you, from `make-release-keystore` |
| `ANDROID_KEYSTORE_PASSWORD` | its password | you, from `make-release-keystore` |
| `IOS_DIST_CERT_P12_BASE64` | the distribution certificate and its key | `make-ios-signing` |
| `IOS_DIST_CERT_PASSWORD` | the password that bundle was made under | `make-ios-signing` |
| `IOS_PROVISIONING_PROFILE_BASE64` | the App Store profile | `make-ios-signing` |
| `IOS_TEAM_ID` | the team id, the `IOS_TEAM` in `local/devices.env` | `make-ios-signing` |
| `APP_STORE_CONNECT_KEY_ID` | the API key's id | `make-ios-signing` |
| `APP_STORE_CONNECT_ISSUER_ID` | the issuer id shown above the key list | `make-ios-signing` |
| `APP_STORE_CONNECT_PRIVATE_KEY` | the contents of the `.p8` | `make-ios-signing` |

Doing any of the iOS half by hand instead is perfectly possible: make the
certificate in Xcode under Settings → Accounts → Manage Certificates,
export it from Keychain Access as a `.p12`, make an App Store profile on
the developer portal, and set the secrets yourself. The script exists
because that is an afternoon of clicking that comes back every year.

### Two ways of signing

Xcode signs in one of two modes, and this app uses a different one in
each place. On your machine it is *automatic*: Xcode talks to Apple as
the Apple ID you are logged in as, and makes or renews whatever
certificate and profile a build turns out to need. A runner has nobody
logged in, so there it is *manual*: it is handed one certificate and one
profile and told to use exactly those. `ci/setup-ios-signing` switches
the mode by writing the same gitignored `ios/Flutter/Signing.xcconfig`
that `../local/build-to-phone` writes, with the identity and the profile
named alongside the team. Nothing about building to your own phone
changes.

### Getting it to your friends

In TestFlight, an **internal** tester is someone you have added to the
App Store Connect account, and their builds are available as soon as
processing finishes, with no review at all. An **external** group is
anyone with the public link, and the first build of each version goes
through a beta review, which is a fraction of App Store review and
usually same-day. Later builds of that version skip it.

Either way they install TestFlight, tap a link, and get every build after
that automatically. A build expires ninety days after upload, so a tag
every few months is the cost of them keeping the app.

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
