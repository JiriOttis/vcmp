# Library check

Spec: docs/specs/2026-11-07-library-check.md
Date: 2026-11-07
Status: implemented
Area: vcmp-library, vcmp-integrations, vcmp-app

## What was done

- `vcmp-library`: `src/check.rs` with `duplicate_groups`, `mark_not_duplicates`,
  `unmark_not_duplicates`, `missing_tags` and `albums_without_cover`; migration 14
  adds the `not_duplicates` table. Accents are folded with `unicode-normalization`
  (now a direct dependency).
- `vcmp-integrations`: `file_manager` with `open_folder` (moved from `themes.rs`)
  and `reveal` (Explorer `/select`, `open -R`, D-Bus `FileManager1.ShowItems` with a
  fallback to the folder).
- `vcmp-app`: the page (`ui/library-check.slint`, `src/library_check.rs`, owned by
  `LibraryView`), the "Check library..." row in Settings, `Place::LibraryCheck` in
  the history, `CoverSearch.find-album`, `TagEditorState.edit-files` and
  `PlayerService::play_tracks`. Six Library check shots in the screenshots example.
- Docs: `library.md`, `ui.md`, `manual-testing.md`, `README.md`, `CLAUDE.md`.
- Checks: `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` pass, so every test
  compiles. `cargo test`, the screenshots example and the manual steps were **not
  run**, at the owner's request (the suite runs once the open sessions are merged).

## How to test

**Automated coverage.** `crates/vcmp-library/tests/library_check.rs` (the rule,
"Not duplicates", the file-name title, covers, the no-write invariant), the unit
tests in `crates/vcmp-library/src/check.rs` (folding, the 2 s chain, the subset
rule), `crates/vcmp-app/tests/library_check.rs` (the page, Back, actions, files
untouched) and `the_library_check_changes_no_file` in
`crates/vcmp-app/tests/ui_invariants.rs`. Run with `cargo test --workspace`. None
of them has been run yet.

**Setup.** A library of copied files: `VCMP_HOME=target/check-home cargo run -p
vcmp-app`, with one music folder holding a few albums. Put a copy of one track
(same title and artist tags) into a second folder, and keep one file without a
title tag and one album without any cover image.

**Steps.**

1. Open Settings. Under Music folders, a "Library check" row has "Check library...".
2. Click it. The Library check page replaces Settings, with a back button
   "Settings" and three tabs: Duplicates (1), Missing tags (n), Missing covers (n).
3. Duplicates lists the copied track once, with two copies: each shows its album,
   "FLAC" (or its format) and length, and its path.
4. Click a copy's play button: both copies play, one after the other, from that one.
5. Click "Not duplicates": the group goes, the tab says Duplicates (0), and a switch
   "Show the 1 group marked Not duplicates" appears. Turn it on: the group shows
   with "Show again". Click it: the group is back.
6. Missing tags: the file without a title tag shows its file name as the title and
   a "No title" marker. Its album block's "Edit tags..." opens the tag editor for
   the listed tracks. Save a year: the tracks lose their "No year" marker or leave
   the list.
7. Missing covers: "Find cover online" opens the cover picker over the page; pick
   a cover, and the album leaves the list.
8. "Show ... in the file manager" (the folder icon) opens the file manager with
   the file selected.
9. Press Escape, or the mouse's back button: Settings shows again. The forward
   button opens the check again.

**Also worth trying.** A third copy of the duplicate after step 5 shows the group
again with three. A library with no duplicates says "No duplicates found.". The
sidebar's Settings entry, while the check is open, shows Settings. More than 100
albums missing tags show "Show more".

## What is missing

Nothing - the spec is implemented in full; its tests are written but not run.

## Known problems

- Not verified: no test, screenshot or manual step has been run yet.
- The queries run on the UI thread when the page opens and after each library
  change while it is open; the spec's risk table suggested a worker thread. A very
  large library may pause the window for a moment.
- Each tab lists 100 entries and "Show more" adds 100, instead of creating only the
  rows in view as the songs table does.
- Accent folding covers the diacritical mark blocks only; letters that don't
  decompose (o with a stroke, sharp s) don't match their plain forms.
- Migration 14 collides with any other branch adding a migration that merges
  first; renumber before merging.
- "Show in the file manager" on Windows and macOS has never been run, and on Linux
  only file managers that answer `FileManager1` select the file.

## Recommended follow-ups

- Run the checks on a second database connection on a worker thread, with a busy
  state, if the owner's library pauses the window on opening the page.
- Create only the rows in view, as the songs table does, if "Show more" proves
  clumsy on a large library.
- An MCP tool for agents that reuses the same queries (deferred by the spec).

## Changelog

- Find duplicate tracks, tracks with missing tags and albums without a cover in Settings, Library check
- Mark a group of look-alike tracks as "Not duplicates" so it stays hidden until another copy appears
- Show a track's file selected in the file manager from the Library check
- Find a cover online for any album the Library check lists, without opening its page
