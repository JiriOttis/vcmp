# Favourites and Last.fm loves

Status: draft
Date: 2026-11-04
Area: vcmp-library (schema, favourites queries, M3U export), vcmp-integrations (Last.fm
love, unlove, loved tracks), vcmp-app (player bar, track lists, track menus,
playlists, Settings)

## Context of the change

VCMP has no way to mark a track as a favourite. The 2026-10-22 spec
(`docs/specs/2026-10-22-listening-and-library-features.md`, "Favourites" in the
High-level plan and Architecture decisions) planned favourites as one of its
features, and issue #78 restates that plan. Two other planned changes wait for it:
the Home page's "Forgotten favourites" (#79) and exporting Favourites as M3U.

Already decided by the 2026-10-22 spec and issue #78, and not asked again here:

- Storage is a `favourites` table (track id, time liked), not a `playlists` row.
  It follows the track id, so a heart survives a re-link; an unavailable track
  stays liked and shows "File not found" like in a playlist.
- Hearts appear in the player bar and in track lists (album, artist, playlist,
  search and folder rows). Row hearts are `tab-stop: false` like the queue's
  remove button; the keyboard path is the menu. No hearts in the Songs table, the
  queue or the now playing view.
- "Add to Favourites" / "Remove from Favourites" is in every track menu (track
  lists, Songs table, queue, now playing), and a test checks that every track menu
  has it.
- Favourites is drawn as a playlist: pinned first in the sidebar list and the
  playlists grid, fixed name, newest first. Its picture was the one point re-opened
  here: it is a drawn heart, not the newest four albums (see Decisions). Its page
  plays, shuffles and exports as M3U, but can't be renamed, deleted or
  reordered; removing a track there unlikes it.
- `TrackRow`, `SongRow` and `QueueRow` carry the track id and a liked flag, and one
  source updates every visible heart.
- Settings gets a "Love on Last.fm" switch (off by default) that sends love and
  unlove when an account is connected, and an "Import loved tracks from Last.fm"
  button that likes the library tracks matching the account's loved tracks by
  artist and title (ignoring case and accents), reports how many matched and how
  many didn't, only adds, can run again, and runs in the background.
- Out of scope: ratings, favourites in file tags, two-way Last.fm sync, MCP tools
  for favourites.

What exists today:

- The schema is at version 12 (labels, `crates/vcmp-library/src/schema.rs`). Play
  history (#77) is open and not started, so favourites takes version 13 if it
  lands first; the 2026-10-22 spec's rule (merge in order, renumber before merging)
  covers a collision.
- Playlists (`crates/vcmp-library/src/playlists.rs`) are rows with ordered
  entries; the app indexes them by position in one list shared by the sidebar and
  the Playlists grid (`ui/app-window.slint`, `open-playlist-index`). The playlist
  page (`ui/playlist-page.slint`) has a More menu with Rename, Choose picture, Use
  album covers, Export and Delete.
- `Library::export_playlist` (`crates/vcmp-library/src/m3u.rs`) checks that the
  playlist row exists before writing, so Favourites can't call it as it is; the
  `write(file, tracks)` helper behind it is the shared seam.
- Track rows identify tracks by file path, not id: `TrackRow`
  (`ui/album-page.slint`), `SongRow` (`ui/songs-page.slint`), `QueueRow`
  (`ui/player-state.slint`).
- Track menus are in `ui/album-page.slint` (track lists), `ui/songs-page.slint`,
  `ui/queue-panel.slint` and `ui/now-playing.slint`.
- Last.fm (`crates/vcmp-integrations/src/lastfm.rs`, `crates/vcmp-app/src/lastfm_service.rs`)
  connects an account, sends "now playing" (not retried) and scrobbles. Scrobbles
  wait in the library's `scrobble_queue` table and a `vcmp-lastfm` thread sends
  them with backoff. The client has signed POST calls only; there is no
  `track.love`, `track.unlove` or `user.getLovedTracks` yet. `docs/ui.md` says
  "Tracks are not loved."
- The player bar's track section is already down to 136 px minimum after the sleep
  timer, kept inside a 720 px window by `smallest_window_keeps_everything_inside`.

## User request

The user wants to implement issue #78, "Favourites: hearts, a Favourites playlist
and Last.fm loves", and to brainstorm its implementation first. Besides the track
menus the issue names, album menus get the entry as well: pressing it applies the
favourite to all the tracks of the album, and nothing is stored on the album entry
itself.

## Decisions

### Favourites (issue #78): how should the work be delivered - one PR, or split so local favourites land before the Last.fm parts?

| Option | What it means |
|---|---|
| Two PRs (Recommended) | Local favourites first (table, hearts, menus, Favourites playlist), then Last.fm love and import; smaller reviews, Home (#79) unblocked sooner, one more merge. |
| One PR | Everything in issue #78 together, as the old spec planned one PR per feature; one review, but a large one that waits on the Last.fm part. |

**Answer: Two PRs**

### Besides every track menu, should album menus (album card and album page three-dot menu, which already have Add to playlist and Labels for all its tracks) get Add to Favourites for the whole album?

| Option | What it means |
|---|---|
| Track menus only (Recommended) | Matches the issue; liking a whole album is a select-all on its page plus the track menu; keeps album menus short. |
| Also album menus | One click to like a whole album, consistent with Add to playlist and Labels there; one more entry, and a mixed album needs an Add/Remove rule. |

**Answer: "Albums as well. Pressing it will apply favourite to all the tracks in the album. Nothing will live on the album entry itself."**

### Album menus get Add to Favourites for all the album's tracks. What does that entry do when some or all of the album's tracks are already liked?

| Option | What it means |
|---|---|
| Add, or Remove when all (Recommended) | Shows "Add to Favourites" while any track is not liked (likes the rest), "Remove from Favourites" once all are; one entry, undoable from the same place. |
| Always add only | Always "Add to Favourites", likes the missing tracks, never unlikes; simplest, but unliking a whole album means selecting its tracks. |
| Both entries | "Add to Favourites" and "Remove from Favourites" both shown whenever they would change something; explicit, but two entries in a mixed album. |

**Answer: Add, or Remove when all**

### Track menus act on all selected tracks (Ctrl/Shift). When a selection mixes liked and not liked tracks, what does the track menu offer?

| Option | What it means |
|---|---|
| Same rule as albums (Recommended) | Whatever is chosen for the album entry applies to a selection too, so the rule is learned once. |
| Add, or Remove when all | "Add to Favourites" while any selected track is not liked, "Remove" once all are, regardless of the album rule. |
| Both entries | Both shown in a mixed selection; explicit, but two entries. |

**Answer: Same rule as albums**

### The Favourites playlist is pinned first in the sidebar playlist list and the Playlists grid. Is it shown while nothing is liked yet?

| Option | What it means |
|---|---|
| Always shown (Recommended) | Present from the first start, its page explains how to like a track; discoverable, and list positions never shift, at the cost of an empty entry for people who never like anything. |
| Only once liked | Appears with the first heart and goes away when the last is removed; no empty entry, but the feature is harder to find and the list shifts. |

**Answer: Always shown**

### Favourites has a fixed name and its picture is made from the newest four albums. Can the owner set a picture of their own for it, like other playlists?

| Option | What it means |
|---|---|
| Fixed picture (Recommended) | Always made from the newest four albums; nothing to store or guard, but no personal picture. |
| Own picture allowed | Choose picture and Use album covers in its menu, like other playlists; needs storage outside the playlists table for that one picture. |
| A fixed heart picture | A drawn heart instead of covers (like Spotify's Liked Songs); always recognisable, but ignores the newest-four-albums plan. |

**Answer: A fixed heart picture** - this replaces the 2026-10-22 spec's "picture
made from the newest four albums".

### With "Love on Last.fm" on, how does a heart or unheart reach Last.fm?

| Option | What it means |
|---|---|
| Queued, like scrobbles (Recommended) | Waits in the database and the Last.fm thread sends it with the same backoff; survives being offline or a restart, needs a small table and collapses like-then-unlike. |
| Sent once, not retried | Sent at once like "now playing"; no new table, but a heart given offline or during a Last.fm outage never arrives. |

**Answer: Queued, like scrobbles**

### What happens to tracks already liked when "Love on Last.fm" is switched on (or an account is connected while it is on)?

| Option | What it means |
|---|---|
| Only new hearts (Recommended) | Nothing is sent for earlier hearts; predictable and no burst of requests, but Last.fm misses what was liked before. |
| Send all once | Loves every liked track on Last.fm at that moment; Last.fm matches VCMP, but a large list is many requests and loves tracks the owner may not expect. |
| Ask when switching on | A confirmation offers to love the N tracks already liked; explicit, one more dialog. |

**Answer: Only new hearts**

### When the Last.fm import likes a library track, what time is recorded as "liked" (it sets the Favourites order, newest first, and later the 3-month clock of Forgotten favourites)?

| Option | What it means |
|---|---|
| Last.fm's love date (Recommended) | Uses the date the track was loved on Last.fm; Favourites keeps the real history and old loves don't flood the top, but they count as old for Forgotten favourites at once. |
| Time of the import | All imported tracks get the import moment; simple, but hundreds of old loves sit on top of Favourites in arbitrary order. |

**Answer: Last.fm's love date**

### One loved Last.fm track can match several library tracks by artist and title (the same song on an album and a compilation, or in FLAC and MP3). What does the import like?

| Option | What it means |
|---|---|
| All matches (Recommended) | Likes every matching track; nothing is missed and hearts show wherever the song appears, but Favourites can list the same song twice. |
| One match | Likes one (an available file, preferring its original album over a compilation); no duplicates, but the heart is missing on the other copies and the choice can look arbitrary. |

**Answer: All matches**

## High-level plan

Two pull requests into `main`, the second after the first is merged.

### PR 1: local favourites

Storage comes first: the favourites list in the database, in the next free schema
version, with liking, unliking and reading it, and tests showing that a re-link
keeps the heart and that unavailable tracks stay liked.

Once storage exists, the app gets one liked state: track rows of every kind carry
the track id and a liked flag, and a single path likes and unlikes and refreshes
every visible heart and the player bar.

On top of that path come two pieces that can be built side by side. The hearts and
menus: the player bar heart, row hearts in track lists, and the favourites entry in
every track menu and in the album menus, with the add-or-remove rule. The
Favourites playlist: the pinned entry in the sidebar and the Playlists grid, the
heart picture, the read-only page with its empty state, and M3U export.

The documentation closes the PR: the library, UI and manual-testing documentation,
the design document (the pinned Favourites and its heart picture as the owner's
decisions), and screenshots in both themes.

### PR 2: Last.fm loves and import

After PR 1 is merged, the Last.fm client first learns to love, unlove and read the
account's loved tracks page by page, tested without the network.

With those calls in place, two pieces follow independently. The "Love on Last.fm"
switch and the queue of loves and unloves that the Last.fm thread sends, fed by PR
1's single like path. The import button, which matches in the background and
reports what matched.

The documentation closes the PR: the Last.fm part of the UI documentation (which
today says tracks are not loved), the new queue in the library documentation, and
manual steps with a real account.

## Architecture decisions

**Favourites live in `vcmp-library`, apart from playlists.** A `favourites` table
(track id as the key, referring to `tracks`, and the time liked in milliseconds),
indexed by time for the newest-first list. It is not a `playlists` row, so no
playlist operation needs a guard, and "Forgotten favourites" (#79) and the play
history's "Favourites" play origin can query it directly. Liking an already liked
track keeps its earlier time. The library offers liking and unliking a set of track
ids in one transaction, the liked set, the newest-first entries (unavailable tracks
included, like playlist entries) and a summary (count, available count, duration).

**Favourites is not a `PlaylistId`.** The app's playlist list has Favourites as a
fixed first entry, always present, followed by the playlists by name, in both the
sidebar and the grid. What is open is either Favourites or a playlist id, never a
fake id, so no library call can treat Favourites as a playlist by mistake. The
"Add to playlist" submenus don't list Favourites; the favourites entry does that
job.

**M3U export shares the writer, not `export_playlist`.** `export_playlist` checks
for a playlist row; the file writing behind it takes a list of tracks, and a
second entry point exports the favourites list through it, so both write the same
format.

**One source for hearts.** Every like and unlike in the app, from any heart or
menu, goes through one function that writes the database, updates the in-memory
liked set, refreshes the liked flag in every row model on screen and the player
bar, and (in PR 2) queues a love when the switch is on. Rows carry the track id so
that lookup does not go through paths.

**The add-or-remove rule is one function.** For a set of tracks (a selection, or
all the tracks of an album) the menu shows "Remove from Favourites" when every
track is liked, and otherwise "Add to Favourites", which likes only the ones not
yet liked. Album menus act on the album's tracks; nothing is stored on the album.

**The Favourites page is the playlist page in a read-only mode.** No Rename,
Choose picture, Use album covers or Delete in its menu, no reordering; Export
stays. Removing a track unlikes it, at once and without a confirmation, like
clicking its heart. The picture is a drawn heart from `ui/icons/`, coloured with
theme tokens and meeting WCAG AA in both themes, not made from album covers (this
replaces the 2026-10-22 spec). Empty, the page says how to like a track.

**Invariants the tests enforce.** Every track menu and both album menus have the
favourites entry; the Favourites page offers no rename, delete, picture or
reorder; a re-link keeps the heart; liking and unliking from one place updates the
others.

**Loves wait in the database (PR 2).** A small queue table like `scrobble_queue`:
artist and title as they were when queued (Last.fm identifies tracks by them) and
love or unlove. A newer entry for the same track replaces a pending one, so
like-then-unlike sends only the last. The existing `vcmp-lastfm` thread sends them
with the scrobbles' backoff. Tracks without an artist tag are not sent, as they are
not scrobbled. Disconnecting forgets loves not sent, as it forgets plays. Only
hearts given while the switch is on are queued; switching it on sends nothing for
earlier hearts.

**The import (PR 2)** reads every page of the connected account's loved tracks on
a background thread with its own library connection, then likes every available
or unavailable library track whose artist and title match, compared in lowercase
with accents removed. Each like takes Last.fm's love date; a track already liked
keeps its own time. Imported likes are not queued as loves. It reports how many
loved tracks matched at least one library track and how many matched none. It
needs a connected account, and the button is disabled while it runs.

## Weaknesses and risks

| Risk | Likelihood | Cost | Mitigation |
|---|---|---|---|
| A row model is missed when the liked flag spreads to all rows, so a heart shows stale | Medium | Confusing UI | The single like/unlike path, and a test that likes from one place and checks the others |
| The player bar's track section, already 136 px minimum, loses room to the heart at 720 px | Medium | Clipped titles | `smallest_window_keeps_everything_inside`; the heart sits in the title row rather than adding a column |
| Schema version collides with play history (#77) if both are in progress | Medium | Rework | Merge in order and renumber before merging, as the 2026-10-22 spec says; test with a throwaway `VCMP_HOME` |
| "All matches" lists the same song twice in Favourites (album and compilation, FLAC and MP3) | Medium | Noise | Accepted by the owner; unliking one copy from the Favourites page is one click |
| Matching misses names that differ slightly ("feat.", punctuation, "Remastered") | Medium | Partial import | Report the unmatched count; the import can run again after tags are fixed |
| Imported loves count as old for "Forgotten favourites" at once, because they keep Last.fm's date | High | Busy Forgotten favourites after an import | Intended by the owner; #79 decides how many it shows |
| The client only makes signed POST calls, and whether `user.getLovedTracks` answers a POST like a GET is not checked | Low | Rework in the client | Check against the real API first in PR 2; add a GET to the transport if needed |
| Unliking from the Favourites page has no undo | Low | A lost heart and its date | Re-liking is one click; the date resets, which is acceptable |
| The heart picture and the pinned entry change what `docs/DESIGN.md` says about the playlist list | Certain | Doc drift | Record both there as the owner's decisions in PR 1 |

Left open, to settle during PR 2 and record in `docs/ui.md`:

- What switching "Love on Last.fm" off does to loves still waiting to be sent:
  send them (they were given while it was on) or drop them.
- How the import's progress shows in Settings for a long loved list (a count of
  pages read, or a busy state only).

## Out of scope

- Hearts in the Songs table, the queue and the now playing view (menu entries only
  there), as the 2026-10-22 spec decided.
- Favourites entries in artist menus, and anything stored on an album.
- A picture of the owner's own for Favourites, renaming it, or reordering it.
- Favourites as a target in "Add to playlist", and Favourites in search results.
- Sending earlier hearts to Last.fm when the switch is turned on, and two-way
  Last.fm sync: loves made or removed on Last.fm only come in through the import
  button, which never unlikes.
- Ratings, and favourites written to file tags.
- MCP tools for favourites.
- The Home page's Forgotten favourites (#79) and M3U import (#82), which build on
  this change.
