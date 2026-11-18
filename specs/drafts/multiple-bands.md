# Multiple bands

Implements [#34](https://github.com/skagedal/iggybilly/issues/34).
Complements [001-label-wiki](../implemented/001-label-wiki.md) and
[002-playlists-from-labels](../implemented/002-playlists-from-labels.md),
whose labels, pages and playlists all become one band's rather than the
instance's.

An iggybilly instance is one band's. Its users are that band's members,
its clips are their recordings, its labels are their songs. None of that
is written down anywhere, because with a single band it never had to be,
and "the band" is simply everything in the database. This spec writes it
down and then allows more than one.

The near reason is that Apple needs an account to review the app with,
and the only account we can give them today is a real one in a real
band's instance, which hands a stranger every rehearsal recording the
band has made. The lasting reason is that the instance is useful to more
than one band and nothing in it is really specific to the first.

The shape here is the smallest one that works. A band owns all content;
a user belongs to zero or more bands; everything administrative is done
by the site owner at the command line, exactly as users already are.
Nothing about the band is self-service, and for one band nothing visibly
changes but the address bar.

## Functionality

### A band owns everything

Every clip belongs to exactly one band, from the moment it is uploaded,
and never moves. So does every label, every label's wiki page, and every
playlist order, which follow from the clips because a label is only
meaningful among the clips it is on.

Two bands can use the same words without meeting. Both can have a label
`verse-1`, both can have a clip called `Take 3`, and the two are
unrelated rows that never appear in the same list. Within one band the
old rules hold unchanged: names are unique, labels are unique, and the
suffixing that resolves a clip-name collision on upload still resolves
it against that band's clips alone.

A user belongs to zero or more bands. Membership is what decides what
they can see: there is nothing an instance-wide view would be good for,
so there is no such view, not even for the site owner. The site owner
reads another band's clips by being a member of it.

### The band is in the URL

Every screen that shows a band's content sits under that band's slug:
the clip list, a clip, a label's wiki history, and the audio behind a
clip. Signing in, the account screen and the health check are not under
a band, because they are about the person or the server rather than
about a band.

Putting the band in the URL rather than only in the session is what
makes a link worth sending. A band's members pass links to clips around
— that is most of what the Discord posts are — and a URL that means
"whatever band you last looked at" is a link that shows the wrong thing
to half the people who click it.

A slug is lower-kebab-case, by the same rule and the same validator as a
label: Unicode lowercase letters and digits, hyphens between groups. It
is the whole of the band's name — there is no separate display name to
keep in step with it, and the slug is what every screen shows.

### Signing in, and where you land

Signing in is unchanged: username, password, no band. Which band you
land in is the server's business, and it answers with the band you were
last in.

That choice is remembered for you rather than by your browser, so it is
the same on a new laptop and on the phone, and a link someone sends you
takes precedence over it: following a link into a band you are a member
of puts you in that band and makes it the one you were last in. Nobody
has to switch bands before a link works.

### Belonging to one band

The common case, and the only case today. Everything looks as it does
now: the same list, the same clip pages, the same labels and wikis. The
address bar has the band's slug in it, and the account screen names the
band you are in. There is no selector, because there is nothing to
select.

### Belonging to several

The account screen lists the bands you are in and marks the one you are
looking at; picking another takes you to its clip list. That is the
whole of the switch — there is no in-page band menu and no band shown in
the header, because for all but one or two people there would never be
anything in it.

Nothing is shared across the bands you are in. The player stops when you
switch, filters do not carry over, and a label with the same name in
both is two labels.

### Belonging to none

An account can exist with no band — a new user before they are assigned
one, Apple's reviewer before the test band is ready, someone whose band
has been wound up. Signing in works, because the password is right, and
then there is nowhere to go.

So the web answers every band screen with a plain page saying you are
not a member of any band and to ask the site owner, with a sign-out
button on it. The app says the same thing in place of the clip list. The
API answers those requests 403 rather than 401, which matters more than
it looks: 401 is what the app treats as "this credential is finished",
and it would respond by throwing the token away and asking for the
password again, which is right for a revoked token and an infinite loop
for a valid account with no band.

### Asking for a band that isn't yours

A URL naming a band you are not a member of answers exactly as a URL
naming a band that does not exist: not found. The two are deliberately
indistinguishable, so that a signed-in member of one band cannot
discover who else is on the instance by trying slugs.

### Administering bands

New commands, alongside the user ones, and site-owner-only in the same
way — by being on the command line on the server:

    iggybilly create-band <slug>
    iggybilly rename-band <old-slug> <new-slug>
    iggybilly list-bands
    iggybilly assign-to-band <username> <slug>
    iggybilly remove-from-band <username> <slug>

`create-band` refuses a slug that is not lower-kebab-case, one that is
taken, and one that would shadow a path the server already uses.
`rename-band` is how a band is renamed, and it says as it runs that
every link to the old slug stops working — there is no redirect from
it, because the rename is rare, deliberate, and done by the person who
can warn the band.

`list-bands` prints each band with its member count, and `list-users`
gains the bands each user is in, so the two together answer "who can see
what" without opening the database.

`remove-from-band` leaves the user's clips where they are. They belong
to the band, not to the person who uploaded them, and a bass player
leaving does not take the recordings of the songs with them.

Membership carries no role. The one privileged actor is the site owner
at the command line, nothing in the app would read a role, and a column
that nothing reads is a guess about a decision we have not made yet.
`is_admin` on a user stays what it is: site-wide, and still gating only
the things that are CLI-only anyway. When band administration moves into
the app — inviting, removing, renaming — that spec is where a band role
belongs, and it will have the use cases to shape it.

### Discord

The webhook is per band, not per instance, set by the site owner. This
is not a nicety: one instance-wide webhook would post one band's uploads
and wiki edits into another band's channel, which is the same leak the
whole spec exists to prevent, arriving by a side door.

A band without a webhook posts nothing, which is the existing behaviour
of an instance without one. The links in a post carry the band's slug,
like every other link.

### The phone app

The app has no address bar, so the band lives in its account screen,
exactly as on the web: the bands you are in, the one you are looking at
marked, and picking another reloads the lists. A user in one band sees
the band named and nothing to pick.

The app remembers the selected band on the device as well as asking the
server for it, so a cold start with no network opens the band you were
last in rather than refusing to open at all. Cached and kept-downloaded
clips stay valid across a switch — a clip is a clip whichever band it is
in — and are evicted by the existing size rule without regard to band.

Switching band while something is playing stops it.

### Open questions

- The migrated band is called `default`, which is nobody's band name.
  Renaming it to the band's real slug is a one-line command, and the
  day to run it is before anyone else is on the instance. Should the
  migration refuse to leave it at `default` instead — or is the rename
  simply the first thing on the deploy checklist?
- Apple's reviewer gets a band with a handful of clips in it. Who
  uploads them, and do they have to be real recordings? A band with
  three clips of a metronome would review fine and gives nothing away.
- Does a band that has lost its last member keep its clips indefinitely,
  or is that worth a command?
- Search (#26), when it arrives, searches within a band. Is there ever
  a reason for a search across the bands you are in?

## Implementation

### Storing

A band is a row with a slug, a creation time, and its Discord webhook.
Membership is a user-band pair, and carries nothing else.

Clips and labels each gain the band they belong to. Everything else
reaches a band through them: a wiki revision through its label, a
playlist position through the clip-label pair, a comment — when there
are comments — through its clip.

The two uniqueness rules that are instance-wide today become per band:
clip names, which are case-insensitively unique so that a name means one
recording, and label names, which are unique so that a label means one
song. Both keep their case-insensitivity and gain the band as the first
column, which is also the order every list reads them in.

A user gains the band they were last in. It is a preference, not a
permission: it decides where to send someone who has not said where they
want to go, and it is never what a request is scoped by. Scoping by it
would make two requests from the same account race each other — the app
refreshing the clip list while the web switches band would answer the
app with the other band's clips.

### The migration

A band called `default` is created. Every existing clip, label and user
becomes its, and the instance's Discord webhook becomes that band's.
Everyone's last-band is set to it.

So the day it ships nothing moves: the same people see the same clips
under the same labels with the same playlist order, the same posts go to
the same channel, and the only visible change is `/default/` appearing
in the address bar. Renaming the band to something the band would
recognise is a command, run whenever.

### Routing

The band's slug is the first path segment of every page route and of the
audio route. The pages the band owns move under it; login, the account
screen, the health check, the static files and the two API prefixes stay
where they are.

Those existing first segments are exactly the reserved slugs, and
`create-band` refuses them. The check belongs in one place with the
routes it protects, so that a route added later cannot silently become
unreachable for a band that already has its name.

The router in the browser holds no route table and asks the server what
every URL is, so it needs no change to follow links into a band; the
server stays the only place URLs are defined.

### What the server checks

Every request for band content resolves a band from the path, and checks
that the caller is a member of it. A slug that does not exist and a
slug the caller is not in both answer not found, from the same branch,
so the two cannot be told apart by timing either.

Clip and label ids stay globally unique, which keeps every existing id
in every existing link working. The id in the path is not trusted to
imply its band: a clip whose band is not the band in the path is not
found, which is the same answer as a clip that does not exist, and the
same answer the audio route gives for it.

The queries gain the band as a parameter, which is a parameter the
shared query layer takes once for both clients rather than a filter each
handler remembers to apply. There is no query for "all clips" left to
call by accident.

### The two clients

The app's surface mirrors the web's: the band is a path segment on every
route that reads or writes a band's content, and `/api/v1/me` grows the
list of bands the caller is in and which one they were last in. The app
reads that list to draw its account screen, and names the band in each
request rather than relying on what the server remembers, for the same
reason the web puts it in the URL — a request that says what it wants
cannot be answered about the wrong band.

Setting the band you were last in is its own small endpoint, called by
both clients when a person switches, and by neither on an ordinary read.

The page props gain the current band and, for the account screen, the
caller's bands. The props for a clip row are otherwise unchanged, since
a row never shows its band: everything on the page is already from one.

### Installed apps that know nothing about bands

Every app already installed calls the routes without a band in them, and
will go on doing so until each phone updates. Those routes keep working
and resolve to the band the caller was last in. For a user in one band —
every user on the day this ships — that is their band, and the app
carries on working untouched.

This is the one place the last-band preference scopes a request rather
than directing a person, and it is the reason it must be a column rather
than something the session holds: a bearer token from an old app build
has no session to put it in.

The unversioned routes come out when the app's own minimum version has
moved past this, and not before.

### Documentation

The README's "What it does" gains the band as the thing everything
belongs to, "Admin" gains the five commands, and the deploy notes gain
the rename as a step. The app's README gains the account screen's band
list and what a switch does to the player and the cache.

## Alternatives considered

**An instance per band.** The deployment already exists and is a single
container with a hostPath for its data; a second one is a second
namespace, a second host name, a second certificate and a second thing
to update. It would also not solve the problem that prompted this —
Apple's reviewer would need an instance of their own, kept alive and
kept current, to review an app that is otherwise identical.

**A band as a label.** Labels already partition clips, and "the band is
a label every clip carries" needs no new table. It fails on everything
that is not a clip: a label would have to belong to a label, wiki pages
and playlists with it, and the filter UI would have to hide one label
from every list it draws. It also makes a leak the default — forget the
filter in one query and you have published another band's recordings.

**A subdomain per band.** Prettier, and wrong here. The session cookie
is `SameSite=Strict` on one origin; per-band origins mean either a
cookie per band or a cookie across subdomains, and a wildcard
certificate, and a local development story that no longer works by
visiting a port on localhost. The path segment costs none of that.

**The band only in the session, not in the URL.** Fewer routes to
change, and it breaks links between members of the same band, which is
most of what links here are for.
