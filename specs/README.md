# specs

Written-down designs for the features that are too big to hold in a
GitHub issue. An issue says what is wanted; a spec here says what it
does and how it is built, in enough detail that implementing it needs no
further design decisions.

Small changes don't get one. A spec is worth writing when a feature
touches the database, the server, the web frontend and the phone app at
once, or when the interesting part is a decision rather than the code —
which order wins when two people drag at the same time, what a repeat
button means once there is a playlist to repeat.

## Where a spec lives

    specs/drafts/       not built yet
    specs/implemented/  shipped

A spec starts in `drafts/` under a plain name — `recording-in-app.md` —
and is edited as freely as any other work in progress. It is a proposal,
and arguing with it is the point.

When the feature is on `main` and works, the file moves to
`implemented/` and **takes the next number**: `005-recording-in-app.md`.
The move happens in the commit that ships the feature, and that commit is
the last chance to edit it — the text is brought in line with what was
actually built where that differs from what was planned.

After that it is frozen.

## Numbered specs are decision records

A spec in `implemented/` is not a maintained description of the system.
The READMEs are that. It is a record of what was decided and why, at a
particular time, and its value comes from being exactly what was decided
then.

So it is not rewritten when the world moves on. A design that gets
replaced two years later leaves its spec standing and wrong, which is the
correct outcome: the reasoning that was true at the time is what you want
when you are working out why something is the way it is.

Two exceptions, both narrow:

- **A correction** for something that was never right — a mistyped route,
  a wrong file name, a claim about the code that was false the day it was
  written. Fixing a fact is not revising a decision.
- **A header note** saying that a later spec has superseded or
  complemented this one. That is a pointer, not a change to the argument.

Everything else is a new spec. A decision is undone by a later decision,
and both stay readable.

### Superseding and complementing

A spec that replaces an earlier decision says so at the top, and the
earlier one gains a line pointing forward:

    > Superseded by [007-playlist-order-v2](007-playlist-order-v2.md).

A spec that builds on an earlier one without contradicting it says
**complements** instead. The distinction is whether the old text is still
true. Superseded means it is not, and you are reading history.

The forward pointer on the old spec is the only edit a later change makes
to it. Numbers are never reused, and a spec is never deleted.

A number can be claimed by a spec written from the code afterwards,
rather than from a draft. That is worth doing when a later spec needs to
argue from an earlier decision that was never written down:
`001-label-wiki.md` exists because the comments draft takes its storage
decision from it, and an argument that points at a written reason is
stronger than one that points at a file.

## How to write one

Name the file after the feature, not the issue: `recording-in-app.md`,
not `issue-4.md`. The number is not part of the name until the spec moves
to `implemented/`, so a draft never has one and two drafts can never
collide over it.

Reference the issue number at the top, and any spec this one supersedes
or complements.

Then two parts, in this order.

**The functionality specification** comes first, and is written without
mentioning a table or a route. What does the user see, screen by screen,
on the web and on the phone? What are the states, including the ones
nobody wants — no microphone, no network, a permission denied, two
people editing the same thing? Be concrete and decisive: where the issue
leaves something open, pick an answer and write it as the spec. Collect
the picks you are least sure of in a short "Open questions" section at
the end, so they can be argued with without unpicking the rest.

**The implementation specification** comes second: the migration as
actual SQL, the routes with their request and response shapes, and the
real files that change on each of the three sides. It should read as
instructions to someone who knows the codebase, not as an introduction
to it.

Both halves are prose where an argument is being made and lists only
where the items are genuinely parallel. The point of the document is the
reasoning; the lists are the leftovers.
