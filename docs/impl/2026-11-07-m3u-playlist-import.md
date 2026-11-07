# M3U playlist import

Spec: docs/specs/2026-11-07-m3u-playlist-import.md
Date: 2026-11-07
Status: implemented
Area: vcmp-library (M3U reading and matching, playlists), vcmp-app (Playlists page, import report)

## What was done

- `vcmp-library`: `Library::import_playlist` in `src/m3u.rs`, next to the writer.
  It reads the file as UTF-8 (byte order mark dropped), or as Windows-1252 when the
  bytes aren't UTF-8. Entries match by path first (`file:` URLs decoded,
  relative paths resolved against the file's folder, `/` and `\` alike, a second
  try by the file's real path), then by `#EXTINF` artist and title, folded for
  case, diacritics and spaces, with the closest length winning. It returns
  `M3uImport` with the unmatched `M3uEntry`s. `create_filled_playlist` in
  `src/playlists.rs` makes the playlist under a free name ("Mix 2", ...) and fills
  it in one transaction. New dependencies of the crate: `unicode-normalization` and
  `url`, both already in the workspace.
- `vcmp-app`: an Import… button beside New playlist, the file dialog and
  `PlaylistBrowser::import` in `src/playlists.rs`, and the report dialog
  (`ui/playlist-import.slint`: `PlaylistImport`, `PlaylistImportReport`). The
  report shows only when something didn't match, nothing did, or the file couldn't
  be read.
- Tests: unit tests in `src/m3u.rs` (decoding, parsing, paths, folding, the length
  tie-break) and in `tests/m3u.rs` (round trip with the exporter, every path form,
  `file:` URLs, Windows-1252, missing tracks, tag matching, unmatched entries,
  nothing matched, taken names, case on Windows); `crates/vcmp-app/tests/playlists.rs`
  drives the import and its report.
- Docs: `docs/library.md`, `docs/ui.md`, `docs/manual-testing.md`, `README.md`,
  `CLAUDE.md` (crate table), and import taken off "What is missing" in the
  listening and library features notes.
- Checks: `cargo fmt --all --check` passes, and
  `cargo clippy -p vcmp-library -p vcmp-app --all-targets -- -D warnings` passes
  (so the tests compile too). The tests were not run, at the owner's request: they
  run once the open sessions are merged.

## How to test

**Automated coverage.** `cargo test -p vcmp-library --test m3u`, the unit tests in
`crates/vcmp-library/src/m3u.rs` (`cargo test -p vcmp-library --lib m3u`), and
`cargo test -p vcmp-app --test playlists an_import`. Not run yet (see above).

**Setup.** A library of copied files, never the real collection:
`VCMP_HOME=target/import-check cargo run -p vcmp-app`, with a music folder holding a
few albums, at least one with accented names (Dvořák). Export one playlist with a
track in it twice into the music folder first (playlist page, More, Export…).

**Steps.**

1. Open Playlists. Beside the title are Import… and New playlist.
2. Press Import…: the system's file dialog opens, filtered to `.m3u` and `.m3u8`.
3. Pick the file exported in Setup. A playlist named after the file, with " 2"
   added (its name is taken), opens with the same entries in the same order, the
   repeated track twice. No dialog shows.
4. Write a file `Mix.m3u` next to the music folder:
   `#EXTINF:1,<artist> - <title of a track in the library>` then `elsewhere/x.mp3`,
   then `#EXTINF:5,Nobody - Nothing` then `gone.flac`, then
   `http://example.com/x.mp3`. Import it.
5. Playlist "Mix" opens with the one track, under a dialog: "Imported as “Mix”",
   "2 entries aren't tracks of the library, so they were left out:", then
   "Nobody - Nothing" over a smaller `gone.flac`, then `http://example.com/x.mp3`.
6. Press Escape: the dialog closes and the playlist is there; Tab reaches its back
   button.
7. Import a file whose entries are all unknown: no playlist is made, the dialog
   says "No playlist was imported" and lists them; Close puts the focus back on
   Import….

**Also worth trying.**

- An empty `.m3u8`: "No playlist was imported", "“Empty.m3u8” has no entries."
- A `.m3u` saved in Windows-1252 with "Café" in a path: the entry matches.
- A VLC playlist (`file:///` URLs) and a foobar2000 one: every library entry comes
  in.
- A phone's export (paths from the phone): entries come in by artist and title.
- A long unmatched list: arrows, Page Up/Down, Home and End scroll it.
- A 720 px wide window: Import… and New playlist stay inside it.

## What is missing

Nothing - the spec is implemented in full.

## Known problems

- Not verified by tests yet: the owner asked for no test runs in this session. The
  Slint layout of the report (a `FocusScope` around the list and the foot, for the
  scrolling keys) and the focus handling after it closes were not seen in a window.
- The button reads "Import…", not "Import playlist…" as the spec wrote it: title,
  Import playlist… and New playlist side by side would likely not fit the 500 px
  page of a 720 px window. Its spoken label is "Import playlist…".
- The dialog closes with "Close", like the agents' history, where the spec said OK.
- The spec expected the real path to fix letter case on Windows and macOS; only
  Windows does (`canonicalize` there returns the file's own case). On macOS a path
  in other case falls back to the tags.
- An entry with a title alone (no " - ") matches that title by any artist; and
  "Artist - Title" that finds nothing tries the whole text as a title, so a track
  without an artist whose title holds " - " still matches.
- The import runs on the UI thread, like export. A file of thousands of entries
  whose paths miss reads every track's tags once; not measured.

## Recommended follow-ups

- Measure an import of 10 000 entries on a large library, and move it off the UI
  thread if it pauses the window.
- Add the report to `examples/screenshots.rs`, so its look is checked in every
  theme.
- Offer encoding detection (or a code page choice) if CP1250 or CP1251 files turn
  out common.

## Changelog

- Import an M3U or M3U8 playlist from another player, a phone or a backup from the
  Playlists page; its tracks are found by path or by artist and title, and the
  entries that aren't in the library are listed
