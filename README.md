# iggybilly

Self-hosted audio clip sharing for a band. Rust + Axum + SQLite on the
back, React + TypeScript on the front, deployed as a single container to
`iggybilly.skagedal.tech`. There is also a Flutter app for iOS and
Android in [`mobile/`](mobile/), talking to the same server.

## What it does

- Users sign in with username + password (argon2-hashed, session cookies).
- Each user can change their own password; admin resets are CLI-only.
- Anyone can upload an audio file (mp3, m4a, anything browsers play),
  give it a name, and see it in a reverse-chronological list.
- Each user can delete clips they uploaded (from the clip's own page);
  the row, its labels, and the audio file on disk all go with it. You
  can't delete someone else's clip.
- Each clip shows its waveform, and playing one loads it into a single
  player bar fixed to the bottom of the page. The bar keeps playing as
  you move around the site; playing another clip takes it over.
- Clicking the bar opens the player: the whole waveform, both times, skip
  buttons, and a repeat toggle. Repeat is remembered between visits, and
  when the player holds a single clip it is the media element's own
  looping, so it is gapless — which matters when the clip is one bar
  long. When a playlist is playing, repeat wraps from its last clip back
  to the first instead, and the element does not loop.
- Each clip can have any number of labels (lower-kebab-case, with
  Unicode letters allowed: `verse-1`, `pålägg`, `café-version`). Adding
  a label autocompletes against existing labels and offers to create a
  new one when the input is a valid format and doesn't exist yet.
- Clips can be filtered by clicking labels (AND semantics with multiple).
- Filtering by exactly one label shows that label's playlist: its clips
  in an order anyone can drag them into, shared by everyone. Pressing
  play there plays on through the rest of the list, with previous, next
  and the queue itself in the player. See
  [`specs/implemented/002-playlists-from-labels.md`](specs/implemented/002-playlists-from-labels.md).
- Each clip has a download link that serves the original upload bytes
  with `Content-Disposition: attachment` and the original filename.
- Each label can have a Markdown wiki page, with full edit history and
  restore. Filtering by a label shows its page above the clips.
- The phone app does all of the above, and adds a list of the devices
  you are signed in on, any of which you can sign out from any other.
- The phone app also keeps clips on disk: played clips are cached up to a
  size you choose and evicted least-recently-played first, and any clip
  can be marked "keep downloaded" to stay put and play with no network.
  See [`mobile/README.md`](mobile/README.md).

## How the frontend fits together

Every screen is a real server route with its own URL. Asked for it as a
browser would, a route returns a small HTML shell:

    templates/page.html   the only template left: <title>, the
                          <script>/<link> tags, an empty #root, and a
                          <script type="application/json"> page-data blob

That blob is an *envelope* — `{entry, title, props}` — saying which page
module renders this URL and what data it needs. `web/src/main.tsx` reads
it, imports `web/src/pages/<entry>.tsx`, and renders.

Asked for the *same URL* with `Accept: application/json`, the route
returns the bare envelope instead. That's what makes client-side
navigation possible: `web/src/router.tsx` intercepts link clicks,
fetches the target URL for its envelope, and swaps the page in place.

The point of not doing full page loads is the player. A real navigation
tears down the document, and with it any playing `<audio>` — there is no
way around that. So the player bar lives above the router's outlet
(`web/src/player.tsx`) and simply never unmounts, and audio keeps going
as you move between pages.

Two things are worth noticing about this router:

- **It holds no route table.** To navigate anywhere it asks the server
  what that URL is. Adding a route needs no change in `router.tsx`, and
  the server stays the only place URLs are defined.
- **It fails safe.** A redirect (an expired session bouncing to
  `/login`), a non-JSON answer, a network error, an unknown entry — all
  fall back to a real browser navigation. Worst case you get the plain
  multi-page behaviour, never a dead link.

Anything that changes state goes through the JSON API under `/api`
(`web/src/api.ts` wraps it; `src/handlers/` implements it). Session auth
rides on the same `SameSite=Strict` cookie the pages use, so there's no
separate token to manage.

`web/build.mjs` bundles with esbuild into `static/dist/`, which the Rust
app serves through its existing `ServeDir`. There are two entry points —
`main.tsx` and the stylesheet — and each page becomes its own lazily
fetched chunk. Filenames carry a content hash, and a generated
`manifest.json` maps `"main.js"` → `"/static/dist/main-TPA4AHVK.js"`;
the server reads it at startup (`src/assets.rs`) and also uses it to
`modulepreload` the current page's chunk, so a cold load fetches the
entry and the page together instead of discovering one from the other.

Waveforms on clip rows are a plain `<canvas>` drawn from the stored
peaks (`web/src/components/Waveform.tsx`). Only the bar runs an actual
wavesurfer instance — a list of thirty clips costs thirty bar charts,
not thirty media players.

Adding a page means: a new `web/src/pages/foo.tsx` default-exporting a
component, a line in the `pages` map in `web/src/pageData.ts`, and a
route in `src/web.rs` calling
`handlers::page(&state, format, title, "foo", &props)`.

## Two clients, one server

The browser and the app are different enough that pretending otherwise
would cost more than admitting it.

**The browser** keeps a session cookie, and its screens are server
routes: each returns `{entry, title, props}`, naming a React module and
handing it the data for that page. The props are shaped for the page —
dates already formatted for Stockholm, labels already turned into
`/?label=…` links.

**The app** sends `Authorization: Bearer` and talks to `/api/v1`, which
is the same data in a shape a phone can use: RFC 3339 instants it can
format in the device's own zone, Markdown source rather than rendered
HTML, ids and names rather than hrefs, and an explicit URL for each
clip's audio. See `src/api/`.

Neither is the canonical shape. Both are serialisations of the domain
types in `src/queries/`, which is where the SQL lives, so there is one
implementation of "list the clips carrying all of these labels" and two
presentations of it. Writes work the same way: uploading, deleting,
renaming and setting a password are functions both routes call.

### Tokens

A browser tab should hold a session; an app install should not, because
it stays signed in for months on a device that can be lost. So each
install gets its own bearer token (`src/tokens.rs`), which the account
screen lists and can revoke one at a time.

Only a SHA-256 of each token is stored — SHA-256 rather than argon2
because the token is 256 bits we generated, not a password someone
chose, so there is no dictionary to run against it and every request
pays that hash. Changing a password revokes every token the user has,
since the reason to change it is usually that someone else might know
it; the device that made the change is handed a replacement so it is not
signed out by its own request.

The audio route takes either credential and fails the way its caller can
act on: 401 for a request that presented a token, a redirect to the
login page for a browser that presented nothing. A media player that
followed that redirect would try to decode the login page.

## Local dev

`./local/run` is the entry point. It installs the frontend dependencies
and builds the bundles if either is missing or out of date, then hands
everything after it to `cargo run --`:

    ./local/run create-user simon --admin
    ./local/run serve             # listens on :9020 by default

It works from any directory and skips both build steps when they'd be
no-ops, so re-running after a Rust-only change costs nothing beyond
cargo's own check. It needs `pnpm` and `cargo` on PATH and says so if
either is missing.

When you're working on the frontend itself, a rebuild-on-change loop in
a second terminal is nicer than restarting the server each time:

    cd web && pnpm install && pnpm run dev

Then reload the browser — there's no HMR, `pnpm run dev` just rebuilds.
Note that the server reads the asset manifest once at startup, so if the
bundle hashes change you need to restart the server too.

`cd web && pnpm run check` runs `tsc` and ESLint over the frontend; CI
runs it along with `cargo fmt --check` and the test suite.

The phone app lives in [`mobile/`](mobile/) and has its own README. Its
checks — `flutter analyze` and `flutter test` — run in CI too, against
the SDK version pinned in `mobile/.fvmrc`.

The frontend uses **pnpm**, not npm, and its manifest is
`web/package.json5` — pnpm reads JSON5 natively, so there is no
`package.json` to keep in sync.

The Rust tests don't need the frontend built — without a manifest the
shell falls back to unhashed bundle paths, which is fine for asserting
on the props blob.

Data lives in `./data/` (SQLite DB + `audio/` files). `./static/` is
entirely generated and gitignored.

Environment:

- `IGGYBILLY_DATA_DIR` — where the SQLite DB and audio files live
  (default `./data`).
- `IGGYBILLY_LISTEN_ADDR` — `host:port` to bind (default `0.0.0.0:9020`).
- `IGGYBILLY_STATIC_DIR` — where the frontend build output lives
  (default `./static`, **relative to the working directory**). `serve`
  refuses to start if `<static_dir>/dist/manifest.json` isn't there,
  since without it every page would render a shell pointing at bundle
  URLs that 404 — a blank screen with nothing in the console to explain
  it. If you get that error, either you're running from the wrong
  directory or you haven't run the frontend build.
- `IGGYBILLY_DISCORD_WEBHOOK_URL` — a Discord [incoming webhook][webhook]
  URL. When set, a message is posted to that channel whenever a clip is
  uploaded or a label's wiki page is edited. Unset (the default) disables
  the notifications entirely. Posts are fire-and-forget: a Discord outage
  never blocks or fails an upload or edit, it's just logged.
- `IGGYBILLY_BASE_URL` — the public origin the app is served from, e.g.
  `https://iggybilly.skagedal.tech`. Used only to turn clips and labels
  into clickable links in the Discord posts; without it the posts carry
  plain names.

[webhook]: https://support.discord.com/hc/en-us/articles/228383668-Intro-to-Webhooks

## Updating dependencies

`./update` moves the whole tree forward at once — `Cargo.lock`,
`web/pnpm-lock.yaml`, `mobile/pubspec.lock`, the Flutter SDK pinned in
`mobile/.fvmrc`, and the actions in `.github/workflows`:

    ./update                 # all of it
    ./update cargo mobile    # only those parts
    ./update --dry-run       # print the commands, run none of them

Each part is that ecosystem's own update command; the script is only the
thing that knows where they all live. No manifest is written by it.
`Cargo.toml` and `mobile/pubspec.yaml` carry ranges, so their lock files
move underneath them and crossing a major bound stays a deliberate edit.
`web/package.json5` asks for `"latest"` instead — there the lock file is
the whole pin, and pnpm crosses majors on its own. The two things with no
lock file at all work the same way: for the SDK pin and the actions the
pinned version *is* the range, so those move across majors too.

The `"latest"` convention is the general one for JS manifests here; see
`codestyle/dependency-versions.md` in
[skagedal-tools](https://github.com/skagedal/skagedal-tools).

Actions are pinned to full commit SHAs by [pinact][pinact], with the
version kept in a trailing comment. A tag can be moved to point at
different code; a SHA cannot. `dtolnay/rust-toolchain@stable` is exempted
in `.pinact.yaml`, because that branch defaults the toolchain input to
stable and every tagged release makes the input required instead.

Two of the ecosystems can wait a few days before taking a newly published
version, long enough for a compromised one to be noticed, and both are set
to three days: pnpm through `minimumReleaseAge` in
`web/pnpm-workspace.yaml`, which guards a plain `pnpm install` too, and
pinact through `min_age` in `.pinact.yaml`. cargo, pub and fvm have
nothing equivalent.

[pinact]: https://github.com/suzuki-shunsuke/pinact

Run `git pkgs init` once in the checkout and the script will also finish
with a package-level summary of what moved; see
[git-pkgs](https://github.com/git-pkgs/git-pkgs).

## Admin

    iggybilly create-user <username> [--admin]
    iggybilly reset-password <username>

Both print the new random password to stdout. Send it to the user over
whatever channel you'd send a password.

## Deploy

- CI builds `skagedal/iggybilly:latest` on push to `main` and rolls the
  Deployment in the `iggybilly` namespace.
- K8s manifests live in the [skagedal.tech](https://github.com/skagedal/skagedal.tech)
  repo under `kubernetes/iggybilly/`. The `apply-kubernetes` workflow in
  that repo applies them on every push to its `main`.
- The hostPath `/var/lib/iggybilly` on the cluster node holds the
  SQLite DB and uploaded audio. Add it to the nixos tmpfiles config so
  it exists with the right ownership before the pod starts.
- GitHub Actions secrets required on this repo: `DOCKERHUB_USERNAME`,
  `DOCKERHUB_TOKEN`, `KUBECONFIG_BASE64`. The kubeconfig is the same
  cluster-admin one used by skagedal.tech, blogdans, and bonband —
  copy the value across repos rather than minting a new namespace-
  scoped token.
- The phone apps ship on a `mobile-` tag instead of on push: `git tag
  mobile-0.2.0 && git push origin mobile-0.2.0` puts the Android APK on
  that tag's GitHub release and sends the iOS build to TestFlight. It has secrets
  and one-time setup of its own — see "Releasing" in
  [`mobile/README.md`](mobile/README.md).
