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

A spec starts in `drafts/`. When the feature is on `main` and works, the
file moves to `implemented/` in the same commit, edited to match what
was actually built where that differs from what was planned. Nothing is
deleted: the argument for why something is the way it is stays useful
long after the code exists, and `git log --follow` keeps the whole of it.

A spec in `implemented/` is not a maintained description of the system —
the READMEs are that. It is what was decided, when.

## How to write one

Name the file after the feature, not the issue: `recording-in-app.md`,
not `issue-4.md`. Reference the issue number at the top.

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
