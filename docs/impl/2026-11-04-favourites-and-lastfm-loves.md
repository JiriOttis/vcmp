# Favourites and Last.fm loves

Spec: docs/specs/2026-11-04-favourites-and-lastfm-loves.md
Date: 2026-11-04
Status: implemented (PR 2 awaiting the full test run and a real Last.fm account)
Area: vcmp-library (schema, favourites, M3U export, love queue), vcmp-integrations
(Last.fm love, unlove, loved tracks), vcmp-app (rows, hearts, menus, playlists,
player bar, queue, navigation, Settings)

## What was done

PR 1 of the spec, local favourites, on `feature/favourites` (merged). PR 2, Last.fm
loves and the import, on `feature/lastfm-loves` (2026-11-07), is described in
[PR 2: Last.fm loves and import](#pr-2-lastfm-loves-and-import).

- **Library** (`crates/vcmp-library/src/favourites.rs`, `schema.rs`, `m3u.rs`):
  - Schema version 13 adds `favourites` (track id, time liked).
  - `like`, `unlike`, `liked_tracks`, `favourites` (newest first, an album liked at
    once in its order) and `favourites_summary`.
  - `export_favourites` uses the same M3U writer as `export_playlist`.
- **App** (`crates/vcmp-app/src/favourites.rs`, `ui/favourites.slint`):
  - `Hearts` is the one like/unlike path. It writes the database, keeps the liked
    set for row builders, and updates the liked flag in place on every row model,
    the player bar and the Favourites playlist.
  - `TrackRow`, `SongRow` and `QueueRow` carry `track-id` and `liked`.
- **Hearts:** in every `TrackList` row between the title and the time, shown under
  the pointer and always while liked, no tab stop (`ui/album-page.slint`). The
  player bar has a heart button beside the title (`ui/player-bar.slint`).
- **Menus:** "Add to Favourites" / "Remove from Favourites" in the track lists, the
  Songs table, the queue, the now playing menu, both album card grids and the album
  page's More menu. Remove shows only when every chosen track is liked.
- **Favourites playlist** (`src/playlists.rs`, `ui/playlist-page.slint`):
  - Pinned first in the grid and the sidebar, always shown, with a drawn heart.
  - A `PlaylistKey` enum separates it from library playlists; the `Playlists.names`
    menus don't list it.
  - Its page is read-only: Export… only, no rename, picture, delete or reorder.
  - It plays as the `favourites` context ("Next from Favourites" in the queue).
- **Tests:**
  - `vcmp-library/tests/favourites.rs` (4): like and unlike, order, moved and
    missing files, export.
  - `vcmp-app/tests/favourites.rs` (5): heart click, menus, album menus, the
    Favourites page, player bar and queue.
  - `ui_invariants.rs`: every track or album menu has the entry.
  - Existing tests were updated for Favourites taking grid place 0 and adding one
    sidebar tab stop.
- **Docs:** `docs/library.md`, `docs/ui.md`, `docs/DESIGN.md` (your decisions and
  the agent's choices awaiting review), `docs/manual-testing.md`, `README.md`,
  `CLAUDE.md` (crate table). New screenshot shots: `favourites-light`,
  `favourites-dark`.
- **Checks, on Windows:**
  - `cargo fmt --all --check` is clean.
  - `cargo clippy --workspace --all-targets -- -D warnings` passes.
  - `cargo test --workspace`: every test passed except one engine test,
    `stop_and_the_end_of_the_queue_clear_stop_after_the_current_track`. It is in
    `vcmp-playback`, which this change doesn't touch, and it passed five runs alone
    and the whole engine suite right after (see Known problems).
  - The new heart-click test was seen to fail with the heart disconnected.

## How to test

**Automated coverage.** `cargo test -p vcmp-app --test favourites` and `cargo test
-p vcmp-library --test favourites` drive the heart by a pointer click in the
headless window and the menus through their callbacks. Slint's menus aren't in
the accessibility tree, so no test opens a menu by right click.

**Setup.** A `VCMP_HOME` library with at least two albums and one album of two or
more tracks: `VCMP_HOME=target/fav cargo run -p vcmp-app`, then add a music folder.

**Steps.**
1. The sidebar lists "Favourites" first under Playlists. Open it: the heart
   picture, "0 tracks", and "Nothing is in your favourites yet. ..." show.
2. Open an album and hover its first track. An outline heart appears left of the
   time. Click it: it fills in the accent, nothing plays, and the row stays
   unselected.
3. Open Favourites from the sidebar. The track is listed, and the Playlists grid
   card says "1 track".
4. Right-click an album card → "Add to Favourites". All its tracks are liked.
   Right-click it again: the entry now reads "Remove from Favourites".
5. Play a liked track. The player bar's heart is filled; click it, and the hearts
   on the album page and the Favourites page empty at once.
6. In Favourites, press Shuffle. The queue panel says "Next from Favourites".
   More → Export… suggests "Favourites.m3u8", and the file lists the tracks newest
   first.

**Also worth trying.**
- Select two tracks, one liked: the menu says Add; afterwards it says Remove and
  takes both out.
- Songs table, queue panel and the now playing "…" menu: the entry is there and
  matches the track.
- Keyboard: the Menu key on a track list opens the menu with the entry. Tab
  reaches the bar's heart, and Space toggles it.
- A liked file moved to another folder stays liked after a rescan. A liked file
  whose folder is gone stays in Favourites, dimmed, as "File not found".
- A 720 px window with a long title playing: everything in the bar stays inside.

## PR 2: Last.fm loves and import

### What was done

- **Your decisions on the points the spec left open (2026-11-07):**
  - switching "Love on Last.fm" off keeps the loves waiting, which still go out;
  - the import shows "page X of Y" while it runs;
  - waiting loves get a line of their own under the Last.fm row;
  - matching folds accents with `unicode-normalization` (already used by the
    MusicBrainz lookup in the same crate).
- **Client** (`crates/vcmp-integrations/src/lastfm/loves.rs`, `lastfm.rs`):
  - `Client::love` and `Client::unlove` are signed POSTs like the scrobbles. An
    empty 200 answer counts as success.
  - `Client::loved_tracks(user, page)` reads 200 loved tracks a page. It is a
    plain GET without a session key or signature, so the spec's open question
    (whether a POST works) doesn't arise. `Transport` gained `get`.
  - The parser takes numbers given as strings, a lone track sent as an object
    instead of a list, a missing date, and an empty list (`totalPages` 0).
  - `match_key` and `match_loved` do the matching: NFKD, combining marks dropped,
    lowercase, runs of spaces made one. Every library track that matches is liked.
- **Scrobbler thread** (`lastfm/scrobbler.rs`):
  - A `LoveStore` trait sits beside `ScrobbleStore`.
  - Loves go out one by one once the plays are out, with the same backoff. One
    that Last.fm refuses is dropped; a revoked key keeps them; disconnecting
    clears them and stops an import.
  - The import reads one page each time round the loop, so plays, loves and the
    other jobs keep going. A busy or unreachable page is tried three times.
    After the last page the store likes the matches.
  - New reports: `Loved`, `ImportProgress`, `Imported`, `ImportFailed`.
- **Library** (`crates/vcmp-library/src/loves.rs`, `favourites.rs`, `schema.rs`):
  - Schema version 14 adds `love_queue`, unique by artist and title.
    `INSERT OR REPLACE` gives the newer entry a new id, so a love being sent as it
    is replaced isn't mistaken for the unlove that replaced it.
  - `like_at` likes tracks as of Last.fm's love dates and keeps earlier hearts.
  - `track_names` lists id, artist and title, unavailable tracks included.
- **App:**
  - `Hearts::set` tells a `LoveSink` which tracks' hearts actually changed, and
    `Hearts::refresh` shows likes made elsewhere.
  - `LastfmService` is that sink, wired in `main.rs` with `attach`. It queues loves
    only while connected with the switch on (`lastfm.love`, off by default).
  - `QueueStore` implements `LoveStore`, the import's matching included.
  - Settings: `LastfmLoveRow` (the switch) and `LastfmImportRow` (the button,
    progress and result), both shown only while an account is connected. Every
    string goes through `@tr()`.
- **Tests:**
  - `vcmp-integrations` (17 new): the love and unlove forms, the GET and its
    parsing quirks, matching, the love queue's order, drop and backoff, a revoked
    key, disconnecting, the import's paging, retries, giving up, and loves sent
    while an import waits.
  - `vcmp-library` (4 new): queue replacement and reopening, `like_at` keeping an
    earlier heart, `track_names` with a missing file.
  - `vcmp-app/tests/lastfm.rs` (7 new): the rows only while connected, the switch
    gating loves (and remembered), like-then-unlike leaving only the unlove,
    disconnecting, the import's progress, result and failure. One test runs the
    real scrobbler against a scripted Last.fm and a real library, and checks that
    case- and accent-different loves like both copies of a song with the love date.
- **Docs:**
  - `docs/ui.md` (Last.fm section, file table, known limits) and `docs/library.md`
    (`love_queue`, the new reads).
  - `docs/manual-testing.md` (love, offline loves, import).
  - `README.md` (features) and `CLAUDE.md` (crate and stack tables).
- **Checks, on Windows:** `cargo fmt --all --check` and `cargo clippy --workspace
  --all-targets -- -D warnings` are clean. The Last.fm and favourites suites pass:
  integrations `lastfm`, library `loves`, the library `favourites` test file and
  the app's `lastfm` test file. The full `cargo test --workspace` was not run, at
  your request: you will run it once the open sessions are merged.

### How to test

Use a build with the keys (`VCMP_LASTFM_API_KEY`, `VCMP_LASTFM_SHARED_SECRET`) and a
throwaway `VCMP_HOME`. The steps are in `docs/manual-testing.md` (Online). In short:

1. Connect an account. "Love on Last.fm" and "Import loved tracks from Last.fm"
   appear under the Last.fm row.
2. With the switch off, heart a track: nothing on last.fm. Switch it on and heart
   another: it is loved on your profile. Take the heart away: it is unloved.
3. Press Import: the row counts pages, then says how many tracks are in Favourites
   and how many matched nothing. The hearts show at once.

### What is missing

- Nothing from the spec. Not yet seen against the real API: the love calls, and
  whether `user.getLovedTracks` accepts 200 a page.

## Known problems

- **Not tested by hand:**
  - nobody has tried this in the real window yet;
  - the heart's look on a real screen and the 720 px player bar were only checked
    by the headless tests.
- **Repaint cost:** a heart change scans every row model on screen, the loaded
  Songs table included. That is cheap per row, but not measured on a large library.
- **Sidebar tab stop:** Favourites is always in the sidebar, so it adds one tab
  stop after About.
- **Agents (MCP):**
  - the `favourites` play context isn't one of the MCP playback contexts, so an
    agent reading the queue while Favourites plays sees no named context;
  - MCP tools for favourites are out of scope, as the spec says.
- **The row's "hit area"** for the heart is the heart's column plus half the
  spacing on either side, worked out from the pointer's position, like the play
  button. A very narrow window shrinks the title, not the heart.
- **A flaky engine test:** `stop_and_the_end_of_the_queue_clear_stop_after_the_current_track`
  failed once under the full workspace run (`engine/tests.rs:686`). It passed alone
  and in its own crate's run, so it looks timing-dependent under load.

- **Last.fm (PR 2):**
  - Matching is exact after folding: "feat." credits, punctuation and
    "(Remastered)" make a loved track count as unmatched. The same song on two
    albums is liked on both, as you decided.
  - A love or import report that arrives just after disconnecting can still change
    the row's numbers. The rows are hidden by then, and the next import resets
    them.
  - Imported likes count as old for "Forgotten favourites" at once, because they
    keep Last.fm's dates (intended by the spec).

## Recommended follow-ups

- Name the `favourites` context to agents too, so the MCP queue answer says what
  plays.
- Look into the engine test that failed once under load, so the pre-merge run is
  reliable.

## Changelog

- Like a track with the heart in any track list or the player bar, or from any
  track or album menu
- Find your liked tracks in a Favourites playlist, first among your playlists,
  newest first, ready to play, shuffle or export
- Turn on "Love on Last.fm" to love the tracks you heart on your Last.fm profile
  too
- Bring the tracks you loved on Last.fm into Favourites with "Import loved tracks
  from Last.fm"
