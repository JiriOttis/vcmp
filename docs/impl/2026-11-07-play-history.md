# Play history

Spec: docs/specs/2026-11-07-play-history.md
Date: 2026-11-07
Status: implemented
Area: vcmp-library (schema 14, `plays.rs`, scan backfill, `Track::added`),
vcmp-playback (`NowPlaying.origin`), vcmp-app (play counting, Songs columns and
header menu, Settings), docs

## What was done

- **Library:** migration 14 adds `plays` (track, start time, origin text) with
  indexes on (track, time) and time. `src/plays.rs` records a play, reads counts and
  last played over all time or since a moment, one track's stats, and clears the
  log. `Track` gains `added`; the scanner backfills existing rows once from file
  dates (`library.added_backfilled`), only moving dates earlier.
- **Playback:** `NowPlaying` carries `origin`, the context's key while its own track
  plays and `None` for a track from Up next (`Queue::current_origin`). This is how
  "the engine reports whether the current track came from Up next" was built: the
  origin travels with the track change, since `QueueChanged` for a new context
  arrives after it.
- **App:** `PlayerService` keeps a play for every library track and records it once
  the Last.fm rule is met; Last.fm only gets tracks with an artist. The Songs table
  has Plays, Last played and Added columns (sortable, hidden by default), a
  checkable header menu for every column but Title (`SongColumns`, saved as
  `ui.song_columns_shown`), relative date texts in Slint (`SongDates`, `src/days.rs`
  with chrono), and live row updates. Settings, Playback has "Clear play history..."
  behind the confirmation dialog.
- **Docs:** `library.md`, `ui.md`, `playback.md`, `manual-testing.md`, `README.md`,
  `CLAUDE.md` (chrono in the stack, play history in the library crate).
- **Checks:** `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` pass on Windows, and
  `cargo check --workspace --all-targets` compiles every new test. **The tests were
  not run**, at the owner's request: they run them once the open sessions are
  merged.

## How to test

**Automated coverage.** No end-to-end suite exists. New tests, not yet run:
`cargo test -p vcmp-library --test plays`, `cargo test -p vcmp-app --test
play_history`, `cargo test -p vcmp-playback a_track_names_its_context` and
`only_the_contexts_own_tracks_carry_its_origin`, and unit tests in
`library_view.rs`, `preferences.rs` and `days.rs`. Run `cargo test --workspace`.

**Setup.** A copy of a few albums in `target/manual-test/music`, then
`VCMP_HOME=target/manual-test cargo run -p vcmp-app`, and add that folder in
Settings. For the backfill, copy an existing `VCMP_HOME` from before this branch.

**Steps.**

1. Open Songs. The headings are Title, Artist, Album, Year, Time.
2. Right-click any heading. A menu lists Artist, Album, (Labels while labels exist),
   Year, Time, Plays, Last played, Added, with checks on the ones shown.
3. Check Plays, Last played and Added. They appear after Time; Added reads "Today"
   on a fresh library, Plays reads 0 and Last played is empty.
4. Play a 3-minute track and let it pass 1:30. Its row turns to Plays 1, Last
   played "Today", without leaving the page.
5. Click the Plays heading twice. The played track is first; never-played tracks
   follow, by title.
6. Uncheck Plays in the header menu. The table goes back to Title order.
7. Restart the app. The same columns show.
8. Settings, Playback, "Clear play history...". Press "Keep the play history":
   nothing changes. Press it again and "Clear play history": every Plays is 0.

**Also worth trying.**

- A track with no artist tag: counts in Plays, is not scrobbled.
- Skipping after a few seconds, a 25-second track, or seeking past the half: no play.
- Repeat One over two passes: Plays goes up by two.
- A track queued with "Play next": its play is stored with no origin (check
  `SELECT origin FROM plays` in `library.sqlite3`).
- An old `VCMP_HOME`: after the first scan, Added shows file dates; a later rescan
  never changes them again; a track whose file was away during that scan keeps the
  first-scan date.
- Hide Artist: the edge after Title resizes Title against Album.
- Tab to a heading and press the Menu key: the column menu opens.

## What is missing

Nothing - the spec is implemented in full.

## Known problems

- None of the new or changed tests have been run yet; the first run may need fixes,
  especially the window tests in `tests/play_history.rs` (Settings button lookup,
  menu-free column toggling) and the backfill test's file dates on macOS and Linux.
- The header menu was never opened in a real window; the tests switch columns
  through `SongColumns`, as other tests drive menus through their globals.
- The relative dates refresh on a one-minute timer instead of on window focus as the
  spec says: it covers midnight and a computer that slept, without a focus hook
  Slint 1.18 lacks. Up to a minute late.
- With every column shown, the Songs table's fixed columns need about 540 px plus
  gaps, so at 720 px the text columns get very narrow; hiding columns is the answer
  the spec chose. Not checked against the window's minimum width.
- Plays ascending puts 0-play tracks after 1-play ones, as the spec says (never
  played follow the played ones both ways); it may read oddly for Plays.
- A play that counts during a scan waits for the scan's write lock, up to the 5 s
  busy timeout, on the UI thread (accepted in the spec).
- New code follows the project's commenting style, not the global "remove all
  comments" preference, to stay consistent with the rest of the repository.

## Recommended follow-ups

- Run `cargo test --workspace` and the manual Play history steps, then fix what
  they find before merging.
- Home page (the next feature of the listening spec): Recently played and Most
  played read `plays` with `play_stats(Some(since))`; it still has to decide what to
  do with NULL-origin plays.
- Move the play write off the UI thread if the scan-lock stall shows up in use.
- `docs/impl/2026-10-22-listening-and-library-features.md` still lists play history
  and favourites under "What is missing"; refresh it when the listening features are
  next touched.

## Changelog

- Remember every track you play: how often and when it was last played
- Show Plays, Last played and Added columns in Songs, and sort by them
- Show or hide any Songs column from a right click on the column headings
- Date tracks already in your library by their files, so "Added" means something
  for your existing music
- Clear the play history from Settings, Playback
- Count plays of tracks without an artist tag, and while Last.fm is disconnected
