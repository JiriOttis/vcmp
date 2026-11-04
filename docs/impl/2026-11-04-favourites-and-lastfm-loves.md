# Favourites and Last.fm loves

Spec: docs/specs/2026-11-04-favourites-and-lastfm-loves.md
Date: 2026-11-04
Status: partially implemented
Area: vcmp-library (schema, favourites, M3U export), vcmp-app (rows, hearts, menus,
playlists, player bar, queue, navigation)

## What was done

PR 1 of the spec, local favourites, on `feature/favourites`. PR 2 (Last.fm love,
unlove and import) is not started.

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

## What is missing

- PR 2 of the spec, which waits for this PR to merge:
  - the "Love on Last.fm" switch and the love/unlove queue;
  - the Last.fm client calls (`track.love`, `track.unlove`, `user.getLovedTracks`);
  - the "Import loved tracks from Last.fm" button.

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

## Recommended follow-ups

- PR 2 (Last.fm loves and import), as the spec plans, once this is merged.
- Name the `favourites` context to agents too, so the MCP queue answer says what
  plays.
- Look into the engine test that failed once under load, so the pre-merge run is
  reliable.

## Changelog

- Like a track with the heart in any track list or the player bar, or from any
  track or album menu
- Find your liked tracks in a Favourites playlist, first among your playlists,
  newest first, ready to play, shuffle or export
