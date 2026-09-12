# TestFlight uploads through asc

Draft for [#21](https://github.com/skagedal/iggybilly/issues/21). Nothing
here supersedes an earlier spec.

The issue asks whether to replace `xcrun altool` in
`ci/upload-to-testflight` with [`asc`](https://asccli.sh), and the title
says *evaluate*, so this document has to reach a verdict before it can
describe anything. The verdict is: **swap it**, for
`asc publish testflight`, installed with the author's `setup-asc` action
at an exact version. The argument is in "Why asc" below; everything before that is
what a release then does, and everything after it is how.

Read against `asc` 5.2.1, the current release. Everything asserted about
the tool is from its documentation or its source; the handful of things
that could not be confirmed are in "Open questions" and nowhere else.

The project names its own `--help` as the source of truth for flags, and
the exact spelling of the notes flags here was read from source rather
than from a published page. The command group and `--group`, `--wait`,
`--submit` and `--confirm` are confirmed on the project's front page; the
notes flags are not. So the first thing to do on picking this up is to
install 5.2.1 and run `asc publish testflight --help`, before writing any
of the script below.

## The gap

`altool --upload-app` hands Apple an ipa and stops. It cannot set the
"What to Test" notes on a build and cannot put a build in front of a
tester group, because neither is part of the delivery it performs. So a
tag ships, and some hours later an app on somebody's phone is a version
further along with nothing said about why. For a band sharing an app that
is not a disaster, but it is the one thing in the iOS path that is
missing rather than merely plain.

## What a release does

A `mobile-` tag still starts the release, and the two jobs are still
independent. The iOS job changes only at its last step.

After the ipa is built it is uploaded, and the job then waits for App
Store Connect to finish processing it — five to thirty minutes, usually,
occasionally much longer. Waiting is new, and it is not optional: notes
attach to a *build*, and there is no build record to attach them to until
Apple has made one. Once there is, the notes are written, the build is
given to the tester group, the group is notified, and — for the first
build of a new version only — the build is submitted for beta review.

The runner is therefore held for the length of Apple's processing queue.
This repository is public, so GitHub-hosted macOS minutes cost nothing,
and the wait buys the ability to report the outcome truthfully. A job
that goes green at the moment of upload has told you the upload
succeeded, which is the least interesting half of what you want to know.

### What the tester sees

A push notification from TestFlight, the new build in the list, and under
it a paragraph saying what changed since the last one. That is the whole
of the change as far as anyone outside the repository is concerned.

### Where the notes come from

They come from the tag message.

The tag is already the only place the version is written. Making it the
place the notes are written too keeps that property: there is still
nothing to commit before releasing, and still one thing to get right.

A release becomes one command, `local/release`, which works out the next
version itself and writes the message into an annotated tag:

    ./local/release "Repeat is gapless now. Try a two-bar riff."
    ./local/release minor "Playlists."

It is described under "Implementation". Tagging by hand keeps working;
the script exists so that the version arithmetic and the `-a` are not
things to get right.

The alternatives are worse for this repository rather than worse in
general. A `CHANGELOG.md` is a file that must be edited and committed
before the tag, which trades the one-command release for a two-step one
and adds a file that will be forgotten on the release where it matters.
Commit subjects are free and need no discipline at all, but they are
written for whoever reads `git log` — "Address review on the comments and
playlists specs" tells a tester nothing, and the honest description of
what that produces is noise with a version number on it.

So: the tag message, falling back to the commit subjects since the
previous `mobile-` tag when the tag is lightweight or its message is
empty. The fallback is three lines of shell and it exists because the
failure it prevents — a forgotten `-a`, and a build that appears silently
again — is exactly the one being fixed. It is a floor, not a feature.

`asc` has a `release-notes generate` subcommand that reads git history,
but it produces App Store "What's New" text rather than TestFlight notes,
and the fallback it would replace is one `git log --format`. It is not
used.

### Which group, and why an external one

One external group, `iggybilly testers`, created once in App Store
Connect alongside the app record.

External is the right kind because an *internal* tester has to be a user
on the App Store Connect account with an Apple ID on the team, and
bandmates are not that. An external group is a link and an email address.
The cost is that the first build of each version goes through beta
review — a fraction of App Store review, usually same-day — and that is
why the release submits for it rather than leaving the build sitting in a
state nobody is watching. Later builds of the same version skip it.

An internal group would make the group step almost meaningless anyway:
internal groups can be configured for all builds, and a build reaches
them whether or not anything adds it.

### The states nobody wants

- **Processing fails, or never finishes.** The step reports which stage
  it got to and exits nonzero. The ipa is already at Apple, so re-running
  the job would upload a second copy under a new build number; the right
  recovery is to distribute the build that exists, which is a one-line
  variant of the same command.
- **The upload succeeds and the notes do not.** Same shape: the build is
  live, the step fails, and the notes can be set afterwards without
  touching the binary.
- **No tag message and no commits since the last tag.** Nothing sensible
  to say, so nothing is said: the notes are omitted and the rest of the
  release proceeds. An empty "What to Test" is what TestFlight does
  today, and failing the release over prose would be the wrong order of
  priorities.
- **The group does not exist.** The step fails before uploading anything,
  naming the group. This is setup that was skipped, not a release that
  went wrong, and it should be loud on the first release rather than
  quiet on every one.

## Why asc

### Notes and the group are one call, not three

This is the entire reason to consider the change, and it is the answer
the issue most needs.

`asc builds upload` does not take notes or a group; on its own it is
altool with a different spelling. `asc publish testflight` does. Its
flags include `--test-notes` with a required `--locale`, `--group`,
`--notify`, `--wait` and `--submit --confirm`, and its documented order
of work is upload, wait for processing, write the beta build
localization, add the beta groups, notify, then submit for beta review if
a group needs it. Setting `--test-notes` implies the wait whether or not
`--wait` is passed, for the reason given above.

So the answer to "one call or two" is one, and the one is `publish
testflight` rather than `builds upload`. That distinction matters: the
issue proposes `builds upload`, which would not have closed the gap.

### Authentication, and what `ASC_BYPASS_KEYCHAIN` is actually for

The three secrets already in the repository are enough, and no login step
is needed. `asc` reads `ASC_KEY_ID`, `ASC_ISSUER_ID` and `ASC_PRIVATE_KEY`
(PEM contents, which is exactly what `APP_STORE_CONNECT_PRIVATE_KEY`
holds) straight from the environment. Nothing is written to disk, which
is one thing better than altool, whose whole reason for the
`~/.appstoreconnect/private_keys` dance in the current script is that it
takes a key *name* and not a path.

The issue expects `ASC_BYPASS_KEYCHAIN` to be the CI path. It is not, and
this is worth stating plainly because setting it would make things
slightly worse. `asc`'s credential resolution is a matrix, not an order:
with no profile selected and the keychain *enabled*, a complete
environment credential set is consulted first and skips stored lookup
entirely. With `ASC_BYPASS_KEYCHAIN` set, the first source becomes the
config *file*, and the environment is only a fallback for a missing one.
The documentation says so directly — for environment-only
authentication, clear `ASC_PROFILE`, `ASC_BYPASS_KEYCHAIN` and
`ASC_CONFIG_PATH` and provide a complete set. `ASC_BYPASS_KEYCHAIN` is
for the other CI shape, where credentials arrive as a committed or
restored `.asc/config.json`.

The premise underneath the issue's worry does not hold either. A
GitHub-hosted macOS runner does have a keychain, and
`ci/setup-ios-signing` creates a second one three steps earlier. The
keychain was never the obstacle.

### The app id is a non-problem

`--app` takes an App Store Connect app id, an exact bundle id, or an
exact app name. A non-numeric value is resolved by
`filter[bundleId]` against `/v1/apps` before anything else happens, and
an ambiguous match is an error rather than a guess.

So it is neither a tenth secret nor a lookup call in the script: it is
`--app tech.skagedal.iggybilly`, the same constant `ci/setup-ios-signing`
already hardcodes. The issue's preferred answer, with the lookup done for
us.

### Upload reliability

`asc` does not reimplement Apple's delivery protocol and does not shell
out to Transporter. It uploads through Apple's own REST endpoints —
`POST /v1/buildUploads`, then `/v1/buildUploadFiles`, then the chunked
upload operations Apple hands back, committed with MD5 and SHA-256
checksums. Chunks carry explicit byte offsets, so it runs four of them in
parallel and retries transient transport failures at the chunk level.

That is a narrower claim than "as reliable as altool" and it is the one
worth making: this is a client of a documented Apple API, not a
third-party guess at a private one. Whether Apple's newer upload API is
as forgiving in practice as the path altool has taken for a decade is not
something the documentation can settle, and it is in the open questions.

Failure reporting is the clearer win. altool prints a wall of text and an
exit status. `asc` maps HTTP status onto exit codes (3 for every flavour
of authentication failure, 60–99 for 5xx) and, when a multi-stage publish
fails partway, prints structured output naming `failureStage`,
`completedStages`, and the `buildId` it reached — which is what makes
"the upload worked, the notes did not" recoverable without uploading a
second binary. A failed notes write goes further and prints the exact
retry command with the original text.

### Install cost

One download. The macOS arm64 binary for 5.2.1 is about 53
MB, published as a release asset next to a checksums file, with no
runtime to install underneath it. Against a job that already spends
several minutes in `flutter build ipa`, it does not register.

It is installed with the author's action,
[`rudrankriyam/setup-asc`](https://github.com/rudrankriyam/setup-asc).
Given an exact `version`, it downloads that release asset, verifies it
against the release's SHA-256 checksums file, caches it, and puts `asc`
on the `PATH`. That is exactly the half-dozen lines of `curl`, `grep`
and `shasum` the script would otherwise carry, and it keeps them out of
the script. The action is a second third-party dependency in the release
path, but it is by the tool's author, it is a composite action whose whole
source is one readable `action.yml`, and it is pinned to a commit SHA
like every other action here.

`brew install asc` is the way that is wrong: it pulls a Homebrew update
first, which is minutes rather than seconds.

### What it costs

A third-party binary in the release path, which altool is not. Three
things follow, and only the first needs a decision.

The release cadence is fast — 5.0.0, 5.1.0, 5.2.0 and 5.2.1 all inside a
week. Tracking `latest` in CI would mean a release path that changes
under you between tags, so the version is pinned in the `setup-asc` step
and the action checks the download against the published SHA-256. The
action itself is pinned to a commit. This matches how the repository
already treats Actions (`.pinact.yaml`) and pnpm packages
(`minimumReleaseAge`): pin, verify, and upgrade deliberately.

Telemetry is on by default and stays on in CI. The payload excludes flag
*values*, key ids, issuer ids, bundle ids and paths, so nothing secret
leaks, but a release script should not phone anywhere. `ASC_TELEMETRY_DISABLED=1`
turns it off.

And it is one more thing to understand when the release breaks in two
years. That is real, and it is the price of the feature.

### The alternatives, and why not

**Keep altool and add a second call.** The second call is the App Store
Connect REST API by hand: mint an ES256 JWT, poll `/v1/builds` filtered
by version and build number until the record appears and processing
finishes, `POST`/`PATCH` `/v1/betaBuildLocalizations`, then `POST` the
`betaGroups` relationship, then create a beta review submission when the
version is new. `local/make-ios-signing` already contains a working JWT
minter and a decent error-reporting `api` helper, so this is not
hypothetical — it is roughly a hundred and twenty lines of bash moved and
extended, including a polling loop. It would be the right answer if the
gap were one field. It is four API resources and a wait, and hand-rolling
that to avoid a dependency is the more expensive kind of frugality.

**Upload with altool, then use `asc` only for the notes and the group.**
Keeps the binary on Apple's own path and uses `asc` for the metadata.
Tempting, and it stays available as a fallback if uploads turn out
flaky — it is `asc builds wait --app … --latest` followed by
`asc publish testflight --build-id …`, which is one line's difference in
the script. But it installs `asc` anyway, waits for processing anyway,
and adds a build lookup that the single command does for free. If the
tool is on the runner there is no reason to keep two uploaders.

## Implementation

One script and one workflow step, as the issue says, and a second script
for cutting a release.

### `ci/upload-to-testflight`

Rewritten around `asc`. It keeps its shape: the same three `: "${…:?}"`
guards, the same "no ipa" check, the same comment explaining what the
script is for.

The private key stops being written to `~/.appstoreconnect/private_keys`
— that existed only because altool looks keys up by name — and becomes
`ASC_PRIVATE_KEY` in the environment of the one command that needs it.

Constants at the top, next to each other, in the style
`ci/setup-ios-signing` already uses for `bundle_id`:

    bundle_id="tech.skagedal.iggybilly"
    group="iggybilly testers"

`asc` is on the `PATH` by the time the script runs, put there by the
workflow; the script checks for it and says which step should have
installed it if it is missing, rather than failing on a bare
"command not found".

The notes, from the tag message with the commit subjects as the floor.
`git tag -l --format='%(contents)'` is empty for a lightweight tag and
for an annotated one with no message, which is what makes one test cover
both:

    notes="$(git tag -l --format='%(contents)' "$GITHUB_REF_NAME")"
    if [ -z "${notes//[[:space:]]/}" ]; then
        previous="$(git describe --tags --abbrev=0 --match 'mobile-*' "$GITHUB_REF_NAME^" 2>/dev/null || true)"
        range="${previous:+$previous..}$GITHUB_REF_NAME"
        notes="$(git log --no-merges --format='- %s' "$range")"
    fi
    notes="$(printf '%s' "$notes" | head -c 4000)"

The truncation is deliberate. Apple caps "What to Test" at 4000
characters and `asc` checks that limit in
`asc build-localizations create`, but *not* in
`publish testflight --test-notes` — that path validates only the locale.
So an over-long note would be rejected by Apple after the upload and the
wait, which is the worst possible moment. Truncating here costs one line.

Then the upload, with the notes flags omitted entirely when there are
none, since `--test-notes ""` is not the same as not passing it:

    arguments=(
        publish testflight
        --app "$bundle_id"
        --ipa "$ipa"
        --group "$group"
        --wait --notify --submit --confirm
        --output json
    )
    [ -n "$notes" ] && arguments+=(--test-notes "$notes" --locale en-US)

    ASC_KEY_ID="$APP_STORE_CONNECT_KEY_ID" \
    ASC_ISSUER_ID="$APP_STORE_CONNECT_ISSUER_ID" \
    ASC_PRIVATE_KEY="$APP_STORE_CONNECT_PRIVATE_KEY" \
    ASC_TELEMETRY_DISABLED=1 \
        asc "${arguments[@]}"

`ASC_PROFILE`, `ASC_BYPASS_KEYCHAIN` and `ASC_CONFIG_PATH` are left
unset, which is what selects the environment-only path. `--submit
--confirm` is conditional inside `asc` — it submits for beta review only
when a group that was added needs one — so it is safe on every release
and necessary on the first of each version.

### The workflow

`.github/workflows/release.yml`, the iOS job. Three changes.

The checkout gains `fetch-depth: 0`. The default shallow fetch has no
tags and no history, so neither the tag message nor the previous tag is
readable without it. The repository is small enough that this is not
worth optimising.

A step before the upload installs `asc`:

    - name: Install asc
      uses: rudrankriyam/setup-asc@5358c70a27a3f0d1517604b0f1fdc43e70c1cc4d # v1.0.1
      with:
        version: 5.2.1

The upload step keeps its three secrets and its name. It gains nothing
else; everything else lives in the script.

### `local/release`

A script for cutting a release, next to `local/build-to-phone` and in its
style:

    ./local/release [major|minor|patch] <message>

The bump defaults to `patch`. The message is required: it is what testers
read, and a release script that let you skip it would be undoing the
point of the tag message. The fallback in `ci/upload-to-testflight`
remains for tags made by hand.

It refuses, before doing anything, unless:

- the current branch is `main`,
- the working tree is clean, and
- `main` is the same commit as `origin/main` after a fetch, so the tag
  names a commit that CI has seen and everyone else has.

It then lists the `mobile-*` tags, keeps those that parse as one to three
integers — the same rule the workflow enforces — treats missing parts as
zero, and takes the highest by semver. No tags at all is `0.0.0`. The
bump is applied the usual way (minor resets patch, major resets both),
and the new tag is always written with three parts. The existing
`mobile-0.1.2` makes the next patch `mobile-0.1.3`.

It prints the version it is about to tag and the message, creates the
annotated tag on `HEAD`, and pushes that one tag. Pushing is what starts
the release, so it asks for confirmation first, and `--yes` skips the
question.

### One-time setup

`mobile/README.md`'s "Setting up the Apple side" lists two things that
must be done by hand because App Store Connect has no API for them. This
adds a third, of the same kind: create the external group
`iggybilly testers` and put the band in it. It is one screen, once.

The same file's "Releasing" section, and the comment at the top of
`release.yml`, show `./local/release` instead of the bare `git tag`
command, with a sentence saying the message becomes what testers read.

### What does not change

`local/make-ios-signing` stays, and so does `ci/setup-ios-signing`.
Nothing about signing, the certificate, the profile or the keychain is
touched.

The issue's stated reason for that, though, is no longer true and should
not be repeated. It says `asc`'s signing commands only fetch and sync
existing material. As of 5.2.1 they do more:
`asc certificates create --certificate-type IOS_DISTRIBUTION --generate-csr`
generates the key and the signing request locally and has the certificate
issued against it; `asc signing fetch --create-missing` creates a
profile that does not exist; and `asc bundle-ids create` registers a
bundle id. That is, near enough, the whole of `make-ios-signing`.

So the reason to keep `make-ios-signing` is not that nothing else can do
the job. It is that it works, it runs once a year, and replacing a
working yearly script is not what this issue is about. Worth its own
issue; not worth smuggling into this one.

## Open questions

- **Whether Apple's `buildUploads` API is as forgiving as altool's path
  in practice.** `asc` is a correct client of a documented endpoint, but
  altool has a decade of accumulated behaviour around flaky networks and
  Apple-side hiccups, and no amount of reading settles which fails less
  often. The mitigation is that failure is cheap: the build number folds
  in the run attempt, so a re-run is clean, and a build that uploaded but
  failed later is recoverable with `--build-id`. If uploads do prove
  flaky, the hybrid described above is one line.
- **Whether a build must be fully processed before notes can be
  attached.** `asc` waits, so this spec waits. Apple may well accept a
  beta build localization against a build still in `PROCESSING`, which
  would let the job end minutes earlier. Not confirmed either way from
  the documentation.
- **`--locale en-US`.** Picked because it is the only locale the app
  record is likely to have and the only one worth maintaining for five
  people. If App Store Connect rejects it for a differently-configured
  app, the fix is to match the app's primary locale, which is one
  constant.
- **Truncating at 4000 bytes rather than characters.** `head -c` counts
  bytes and Apple counts characters, so a note full of non-ASCII would be
  cut shorter than it needs to be. Nobody will write 4000 characters of
  release notes for a band's app, and the alternative is arithmetic in
  bash.
- **The action downloads from the CLI's old repository name.** The CLI
  moved from `rudrankriyam/App-Store-Connect-CLI` to
  `rorkai/App-Store-Connect-CLI`, and `setup-asc` v1.0.1 still builds its
  download URLs against the old name, relying on GitHub's redirect. That
  works today and the checksum still guards what arrives, but a redirect
  is not a promise. If it breaks, the fallback is the few lines of `curl`
  the action replaced.
- **Whether the group name belongs in a repository variable.** It is
  hardcoded here, matching `bundle_id` in `ci/setup-ios-signing` and
  `profile_name` in `local/make-ios-signing`. A variable would be one
  more thing that must be set before a release does anything, and
  `IOS_RELEASE` is already that.
- **Whether pinning 5.2.1 will age badly.** The action's SHA is kept
  current by the tooling that already watches actions, but the `version`
  input is a string it does not understand, and is upgraded by a human
  noticing. Nobody is watching a release workflow between releases. The likely outcome is that it stays on 5.2.1 until something
  breaks, which is acceptable and should be admitted rather than
  designed around.
