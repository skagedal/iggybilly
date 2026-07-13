# iggybilly

Self-hosted audio clip sharing for a band. Rust + Axum + SQLite, deployed
as a single container to `iggybilly.skagedal.tech`.

## What it does

- Users sign in with username + password (argon2-hashed, session cookies).
- Each user can change their own password; admin resets are CLI-only.
- Anyone can upload an audio file (mp3, m4a, anything browsers play),
  give it a name, and see it in a reverse-chronological list.
- Each clip shows a waveform preview; pressing play sends it to a single
  global player bar (wavesurfer.js) fixed to the bottom of the page.
  Navigation is boosted (htmx) so the bar keeps playing as you move
  around the site, and playing another clip takes over the bar.
- Each clip can have any number of labels (lower-kebab-case, with
  Unicode letters allowed: `verse-1`, `pålägg`, `café-version`). Adding
  a label autocompletes against existing labels and offers to create a
  new one when the input is a valid format and doesn't exist yet.
- Clips can be filtered by clicking labels (AND semantics with multiple).
- Each clip has a download link that serves the original upload bytes
  with `Content-Disposition: attachment` and the original filename.

## Local dev

    cargo run -- create-user simon --admin
    cargo run -- serve            # listens on :9020 by default

Data lives in `./data/` (SQLite DB + `audio/` files).

Environment:

- `IGGYBILLY_DATA_DIR` — where the SQLite DB and audio files live
  (default `./data`).
- `IGGYBILLY_LISTEN_ADDR` — `host:port` to bind (default `0.0.0.0:9020`).
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
