# Play history

Status: agreed
Date: 2026-11-07
Area: vcmp-library (schema 14, plays log, added-date backfill, queries),
vcmp-playback (whether the current track came from Up next), vcmp-app (play
counting, Songs table columns, Settings); all three platforms

## Context of the change

This is the "Play history" feature of the agreed
[listening and library features spec](2026-10-22-listening-and-library-features.md)
(its "High-level plan" and "Architecture decisions", "Play history"), tracked as
issue [dalibor-osu/vcmp#77](https://github.com/dalibor-osu/vcmp/issues/77). That
spec already decided:

- a play log (every play with its time), not counters on `tracks`;
- the Last.fm rule decides what counts as a play, for every library track, with or
  without an artist tag, whether Last.fm is connected, disconnected or not built in;
- `tracks.added_ms` keeps "first seen" for new rows, and existing rows are
  backfilled once from file dates, never moving a date later;
- Plays, Last played and Added as sortable Songs columns, with a way to hide columns;
- "Clear play history" in Settings, behind a confirmation, deleting the log only;
- out of scope: a statistics page (planned), pausing recording, MCP tools.

This document settles what that spec left open for this one PR. What exists on
`main` today:

- **Schema.** 13 migrations in `crates/vcmp-library/src/schema.rs`, the last one
  `favourites`. The issue says 13 for this migration; favourites took it, so play
  history is version **14**. Schema PRs still land in order and renumber before
  merging if another gets in first.
- **Added dates.** `tracks.added_ms` is set when the scanner inserts a row
  (`scan.rs`), so every track of a library's first scan shares that scan's time. It
  is not on `Track`. The scan rescans at startup and watches sources by default
  ([library.md](../library.md#scanning)).
- **Plays.** `player_service.rs` follows `lastfm::Play` on every Position event
  (`count_play`), but `start_play` builds a play only for tracks with an artist tag,
  and the play only goes to Last.fm's sink. Nothing records it in the library.
- **Origins.** Every `Command::PlayContext` carries an origin string:
  `album:<id>`, `artist:<id>`, `playlist:<id>`, `favourites`, `label:<id>`,
  `folder:<path>`, `search` and `songs`. The queue keeps one origin for its context;
  Up next entries carry none, and `QueueView` does not say whether the current
  track came from Up next (the snapshot's `QueueSlot` does).
- **Songs table.** `ui/songs-page.slint`: Title, Artist, Album, Labels (only while
  a label exists), Year, Duration; headers sort and the three text columns resize,
  with their shares saved in `ui.song_columns` (`src/preferences.rs`) and the sort
  in `ui.songs_sort`. Track menus already open with right click and the Menu key.
- **Settings.** Sections Music folders, Playback, Artist photos, Appearance, Last.fm
  and VCMP. The confirmation dialog exists (`Modal`).

## User request

Brainstorm and implement issue #77, "Play history: play counts, last played and
date added", working on a new feature branch in its own worktree. The issue asks for
a `plays` table with track id, time and play-context origin; plays counted by the
Last.fm rule once per play, for tracks without an artist too and while Last.fm is
disconnected or not built in; `tracks.added_ms` as "first seen" for new rows and a
one-time backfill of existing rows from the earlier of creation and modification
time, never moving a date later, exposed on `Track`; Plays, Last played and Added
as sortable Songs columns and a way to hide Songs columns; "Clear play history" in
Settings behind the confirmation dialog, deleting only the play log. Out of scope: a
statistics page, pausing recording, MCP tools for play history. It also asks for
library tests (log, counts in a time range, backfill never later and run once,
clearing), app tests (counting rule, Songs columns), updates to `docs/library.md`,
`docs/ui.md` and `docs/manual-testing.md`, and every new string through `@tr()`.

## Decisions

### What should each row of the new plays log store as its "origin" (where the play was started from)?

Today the player already tags contexts as `album:<id>`, `artist:<id>`,
`playlist:<id>`, `favourites`, `label:<id>`, `folder:<path>`, `search` and `songs`,
while the October spec listed only album, artist, playlist, Favourites or none.

| Option | What it means |
|---|---|
| Engine string as is (Recommended) | Store the existing origin text verbatim (nullable); nothing is thrown away and Home/statistics decide later what to show, but ids inside text can't be foreign keys (a deleted playlist leaves a dangling id). |
| Kind + id columns | Normalise to a kind (album, artist, playlist, label, favourites, folder, search, songs) and an optional numeric id; cleaner queries and indexable per kind, but folder paths need their own column and every new origin needs a schema change. |
| Spec's five kinds only | Album, artist, playlist, Favourites, else none; matches the October spec literally, but plays from labels, folders, search and Songs lose where they came from forever. |

**Answer: Engine string as is**

### How should columns of the Songs table be shown and hidden? (Title always stays.)

| Option | What it means |
|---|---|
| Header menu (Recommended) | Right-click or the Menu key on any column header opens a checklist of columns; the familiar desktop pattern and reuses the existing menu component, but nothing visible hints that it exists. |
| Columns button | A small "Columns" button at the end of the header row opens the same checklist; discoverable at a glance, but costs header width that the columns need at 720 px. |
| Both | Header menu plus the button; discoverable and fast, at the cost of the button's width and two paths to test. |
| In Settings only | A Songs columns section in Settings; no new UI on the page, but far from where you notice a column is missing. |

**Answer: Header menu**

### Which Songs columns are visible by default (fresh install, and existing installs after the update)?

| Option | What it means |
|---|---|
| New ones hidden (Recommended) | Today's columns stay as they are; Plays, Last played and Added start hidden and are turned on from the column list; nothing changes for anyone until they ask, but the feature is invisible at first. |
| Plays shown, dates hidden | Plays appears next to Duration, the two dates start hidden; the most useful number is visible without crowding 720 px with dates. |
| All shown | Every column visible; the new data is obvious, but at 720 px the text columns get narrow until you hide some. |

**Answer: New ones hidden**

### How should the Last played and Added cells in the Songs table read?

| Option | What it means |
|---|---|
| Short absolute date (Recommended) | e.g. "7 Nov 2026" in the system's locale order; stable, narrow and sorts the way it reads, but "was that yesterday?" needs mental maths. |
| Relative, absolute later | "Today", "Yesterday", "3 days ago" up to a week, then the short date; friendlier for recent plays, but the text changes over time and needs refreshing at midnight. |
| Date and time | e.g. "7 Nov 2026 14:32"; most exact, but about twice as wide, which hurts at 720 px. |

**Answer: Relative, absolute later**

### Where in Settings does "Clear play history" sit?

| Option | What it means |
|---|---|
| Playback section (Recommended) | Next to the other playback settings, where plays are made; but Playback is about how music sounds, not stored data. |
| Music folders section | With the library's own data (folders, rescan); groups it with what's stored, but that section is about where files are. |
| New "Play history" section | Its own heading with one row now; clearest, and room for statistics or pausing later, but a whole section for one button. |

**Answer: Playback section**

### What origin does a play get when the track came from Up next ("Play next" / "Add to queue") rather than from the album, playlist, etc. that was started?

The engine today keeps one origin per context, and Up next entries carry none.

| Option | What it means |
|---|---|
| No origin (Recommended) | Up next plays are logged with an empty origin; honest and small (the engine only has to say whether the current track came from Up next), but Home's "Recently played" won't show where those came from. |
| Origin per Up next entry | Each queued entry remembers where it was queued from (album:12, search, ...); the most accurate log, but every queue action and the saved queue have to carry an origin, a wider change to vcmp-playback. |
| Context's origin anyway | Every play gets the running context's origin; no engine change, but a track queued from search while a playlist runs is logged as played from that playlist. |

**Answer: No origin**

## High-level plan

One PR on `feature/play-history`.

**Library: the log and the dates.** Migration 14 with the `plays` table and its
indexes; recording a play, counts and last played per track, plays in a time range,
clearing the log; `Track` gains the added date; the one-time backfill in the
scanner. Depends on nothing else here.

**Playback: where the current track came from.** The engine says whether the
current track came from Up next. Small, and independent of the library work.

**App: counting.** The player service follows every library track's play by the
Last.fm rule and records it with its origin once it counts; Last.fm still gets only
the plays it can take. Comes after the library and playback work.

**App: Songs columns.** Plays, Last played and Added columns, sortable; the header
menu that shows and hides columns; visibility saved; relative dates. Comes after
the library work, and after counting for its live updates.

**App: Settings.** "Clear play history" in Playback with the confirmation. Comes
after the library work.

**Docs and tests.** The library, playback and UI documentation, the manual testing
steps and the implementation notes, written alongside each workstream.

## Architecture decisions

**The plays table.** `plays (id INTEGER PRIMARY KEY, track_id INTEGER NOT NULL
REFERENCES tracks(id), played_ms INTEGER NOT NULL, origin TEXT)` with an index on
`(track_id, played_ms)`, plus one on `played_ms` for range queries over all tracks
(Home and the statistics page). Tracks are never deleted, so the reference always
holds, and plays follow a re-linked track like playlist entries and favourites do.
Counts and last played are `COUNT` and `MAX` over the index; nothing is cached on
`tracks`. Beat counters because Home and statistics need time ranges.

**Origin is the engine's string, verbatim** (decided). It is whatever the app passed
in `PlayContext`, or NULL. The text is not parsed when stored, so new origin kinds
need no schema change. Readers parse it when they need it and treat an id that no
longer resolves (a deleted playlist or label) as no origin. A `folder:<path>`
origin stores the path as text, as the queue does today.

**Up next plays have no origin** (decided). The engine reports, with the current
track, whether it came from Up next; the queue already knows this from
`QueueSlot::UpNext`. The app stores NULL for those plays. A play with no context at
all (`origin: None`) is also NULL.

**The time stored is when the play started** (`Play::started`), the same moment
Last.fm's scrobble gets, not the moment the rule was met. Last played and ranges use
it.

**Counting.** The player service keeps one play per started track: the track id,
the origin captured at start, and a `lastfm::Play`. It exists for every track that
has a library id and a known duration over 30 s, artist tag or not. When the rule is
met, the play is written to the library once, then handed to Last.fm only if the
track has an artist (today's `lastfm::Track` gate moves to that point). The
"Scrobbling/Scrobbled" mark in the player bar keeps its meaning: it is about
Last.fm only. Files outside the library (no track id) are never logged.
A new play starts whenever the engine announces a track (`TrackChanged`), as
scrobbling already does: each pass of Repeat One is its own play, while Previous
pressed past the engine's restart threshold seeks to the start and stays the same
play. Writing the
play is one small insert on the UI thread, only once per counted play, never on
Position events. A failed write is logged and dropped; it never stops playback.

**Added dates.** `Track` gains `added: SystemTime` (from `added_ms`). New rows keep
"first seen". The backfill:

- runs in the scanner, once, on the first scan after migration 14, and a setting
  (`library.added_backfilled`) records that it ran, so it never runs again;
- touches only rows that existed before that scan (rows whose `added_ms` is older
  than the scan's start), so tracks first seen in the same scan keep "now";
- for each such row whose file the scan finds, sets `added_ms` to the smallest of
  its current value, the file's modification time and, where the file system
  reports one, its creation time; the result can only move earlier;
- leaves rows whose file is missing at that scan as they are, for good.

SQL can't read file dates, which is why this is in the scanner and not in the
migration.

**Songs columns.** All columns except Title can be hidden: Artist, Album, Labels,
Year, Duration, Plays, Last played and Added. Right click or the Menu key on any
header opens a checklist menu of them (decided), reusing the existing menu
component, so it is keyboard-operable. Plays, Last played and Added start hidden on
every install (decided); today's columns start shown. Labels still appears only
while a label exists, and only if it is not hidden. Visibility is saved in its own
setting next to `ui.song_columns`, which keeps the three width shares unchanged; a
hidden text column's share is kept for when it comes back. The three new columns
are fixed-width like Year and Duration. Sorting by a column that gets hidden falls
back to Title. Never-played tracks sort after played ones in both directions of
Plays and Last played order, as a group, ordered by title.

**Date texts** (decided: relative, absolute later). Last played and Added read
"Today", "Yesterday", "2 days ago" to "6 days ago", then a short date such as
"7 Nov 2026"; a never-played track shows an empty Last played cell and 0 plays.
Days are calendar days in local time. Following the pattern of `minutes_ago` in
`agents.rs`, the rows carry numbers (days ago, and the date's day, month and year)
and the text is built in Slint through `@tr()`, so it is translatable, month names
included. The texts are recomputed when the table is built, at local midnight
while the window runs, and when the window regains focus (covering a system that
slept through midnight). Sorting uses the stored times, never the text.

**Live updates.** When a play is recorded, the Songs rows of that track get their new
count and last played in place. The table is not re-sorted under the pointer; the
order catches up the next time the list is built (page opened, sort or filter
changed).

**Clear play history** sits in Settings, Playback (decided), as a button row. The
confirmation names what is lost: every play count and last-played date. It deletes
every row of `plays` in one statement and nothing else: `scrobble_queue`, added
dates and favourites stay. The Songs table refreshes afterwards.

**What tests enforce.** Library: recording and reading counts, last played and a
time range; plays survive a re-link; clearing empties the log and leaves scrobbles
and added dates; the backfill only moves dates earlier, ignores rows first seen in
the same scan, skips missing files and runs once. Playback: the Up next flag for
context and Up next tracks. App: a track without an artist is logged but not
scrobbled; a track under 30 s, a quick skip and a seek past the half are not
logged; one play is logged once even when Position events continue; Up next plays
have no origin; the header menu hides and restores columns and the choice is saved;
the existing UI invariants (no colour literals, `@tr()`, keyboard) still pass.

## Weaknesses and risks

| Risk | Likelihood | Cost | Mitigation |
|---|---|---|---|
| Backfill dates are wrong for copied libraries and for files VCMP already edited (a tag write replaces the file, giving it the edit's dates) | Medium | Odd Added dates on old tracks | One-time only; documented as a known limit in `library.md`; new music is exact |
| Creation time is missing or unreliable on some Linux file systems and network shares | Medium | Backfill falls back to the modification time | That fallback is the decided rule; documented |
| A track missing at the backfill scan (unplugged drive) keeps its first-scan date forever | Low | One old date per such track | Documented; could be retried later if it matters |
| Startup rescan turned off: the backfill waits for the next manual rescan | Low | Added dates stay at first-scan time until then | Documented; the setting keeps the backfill pending until a scan runs |
| Migration 14 collides with another schema PR | Medium | Renumber, broken dev databases | Merge schema PRs in order, renumber before merging, test with a throwaway `VCMP_HOME` |
| Origin text with ids can point at deleted playlists or labels | High over time | Readers see unknown origins | Readers treat unresolved origins as no origin; no foreign key is promised |
| Relative texts go stale when the system sleeps through midnight with the window unfocused | Low | "Today" shown for yesterday until focus returns | The recompute rules above |
| A play written on the UI thread while a scan holds the write lock waits for it, up to the 5 s busy timeout | Low | The window stalls briefly when a play counts during a scan of a slow disk; past the timeout the play is lost | Accepted for now: the scan holds the lock only while writing batches of files already read; moving the write off the UI thread is the fix if it shows up in use |
| Clearing can't be undone | Low | Lost history | Confirmation naming what is lost |
| New columns hidden by default: the feature is invisible until someone opens the header menu | High | Feature goes unnoticed | Mentioned in the release notes; Home (next feature) will surface the data |

Left open:

- Whether Home's "Recently played" uses NULL-origin plays (Up next) at all. Settled
  with the Home page feature.

## Out of scope

- A statistics page (planned for a later change; the log and its `played_ms` index
  are shaped for it), Home and its sections.
- Pausing play history recording.
- MCP tools for reading play history.
- Play counts anywhere other than the Songs table (tag editor, album header, track
  lists).
- Per-entry origins for Up next tracks.
- Importing play counts from Last.fm or other players, or writing them to file tags.
- Locale-dependent date order; v1 is English only, and the date texts go through
  `@tr()` for later.
- Retrying the backfill for tracks missing at its scan.
