# iggybilly

Self-hosted audio clip sharing for a band. Rust + Axum + SQLite on the
back, React + TypeScript on the front, deployed as a single container to
`iggybilly.skagedal.tech`.

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
- Each clip can have any number of labels (lower-kebab-case, with
  Unicode letters allowed: `verse-1`, `pålägg`, `café-version`). Adding
  a label autocompletes against existing labels and offers to create a
  new one when the input is a valid format and doesn't exist yet.
- Clips can be filtered by clicking labels (AND semantics with multiple).
- Each clip has a download link that serves the original upload bytes
  with `Content-Disposition: attachment` and the original filename.

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

## Local dev

Two processes. In one terminal:

    cd web && npm install && npm run dev    # rebuilds on change

and in another:

    cargo run -- create-user simon --admin
    cargo run -- serve            # listens on :9020 by default

Then reload the browser — there's no HMR, `npm run dev` just rebuilds.
Note that the server reads the asset manifest once at startup, so if the
bundle hashes change you need to restart `cargo run` too.

`cd web && npm run typecheck` runs `tsc` over the frontend; CI runs it
along with `cargo fmt --check` and the test suite.

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
