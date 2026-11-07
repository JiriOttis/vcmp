# M3U playlist import

Status: agreed
Date: 2026-11-07
Area: vcmp-library (M3U parsing and matching), vcmp-app (Playlists page); all three platforms

## Context of the change

VCMP exports a playlist, or Favourites, as a UTF-8 `.m3u8` file: "Export..." in a
playlist's More menu, written by `crates/vcmp-library/src/m3u.rs`
(`Library::export_playlist`, `Library::export_favourites`; see
[library.md, "Playlists"](../library.md#playlists)). It cannot read one back, so a
playlist made in another player, or a VCMP export kept as a backup, can't be
brought in.

Import was agreed in
[2026-10-22-listening-and-library-features.md](2026-10-22-listening-and-library-features.md)
("How do M3U playlists get in and out?" and "M3U" under Architecture decisions):
"Import playlist..." on the Playlists page, a new ordinary playlist named after the
file, matching by path and then by `#EXTINF` tags, unmatched entries listed
afterwards. Its implementation notes
([docs/impl/2026-10-22-listening-and-library-features.md](../impl/2026-10-22-listening-and-library-features.md))
list import under "What is missing". GitHub issue #82 asks for it, and this spec
settles what the issue left open.

What exists and constrains the import:

- Playlist names are unique, ignoring case (`playlists.name UNIQUE COLLATE NOCASE`).
  `Library::create_playlist` fails on a taken name.
- A playlist entry is a library track id. A track's `path` is stored as the scanner
  found it, with folder symlinks resolved. A track whose file is gone stays in the
  library, unavailable.
- `rfd::AsyncFileDialog` is already used for export, playlist pictures and artist
  photos.
- Search already ignores case and diacritics (SQLite FTS5), and
  `unicode-normalization` is already in the workspace (`vcmp-integrations`).

## User request

Brainstorm and implement the feature described in GitHub issue #82, "Import M3U
playlists". VCMP can export a playlist as M3U but can't read one, so a playlist
made in another player, or exported from VCMP as a backup, can't come back in.

"Import playlist..." on the Playlists page opens the system file dialog for `.m3u`
and `.m3u8` files. The import makes a new playlist named after the file. It is an
ordinary playlist with no link back to the file. Entries match by path first:
relative paths resolve against the playlist file's folder, and both `/` and `\`
are accepted. Entries that don't match by path match by their `#EXTINF` artist and
title, taking the closest length. Afterwards, the entries that didn't match are
listed. Parsing lives in `vcmp-library`, next to the writer in `src/m3u.rs`.

To check: encodings (`.m3u8` is UTF-8, a plain `.m3u` may be in a legacy code page;
decide what happens to bytes that aren't UTF-8, and record it). A file VCMP
exported must read back to the same playlist, repeats included. Try files from VLC,
foobar2000 and a phone.

Out of scope, per the earlier spec: picking up M3U files from the music folders
automatically, and PLS and XSPF.

Also: tests in `crates/vcmp-library` (relative and absolute paths, tag matching,
unmatched entries, round trip with the exporter) and in
`crates/vcmp-app/tests/playlists.rs`. Update `docs/library.md`, `docs/ui.md` and
`docs/manual-testing.md`, and remove import from "What is missing" in the impl doc.
Every new string goes through `@tr()`.

The work happens on a new feature branch in its own worktree.

## Decisions

### A plain .m3u file may hold bytes that aren't valid UTF-8 (a legacy code page such as Windows-1252 or CP1250). How should import read such a file?

| Option | What it means |
|---|---|
| UTF-8, else Windows-1252 (Recommended) | Whole file read as UTF-8 (BOM stripped); if it isn't valid, re-read as Windows-1252. Covers VLC, foobar2000 and most Western files; Czech/Cyrillic legacy files come out garbled and fall back to tag matching. |
| UTF-8, else system code page | Fallback uses the OS's ANSI code page on Windows (Windows-1250 on a Czech PC). Right for files made on the same machine, but results differ per machine and Linux/macOS have no such page. |
| UTF-8 only, lossy | Invalid bytes become replacement characters; simplest, but those entries never match by path and rarely by tags. |
| Detect the encoding | Guess the code page with a detector crate (e.g. chardetng). Best results on short Czech/Cyrillic files, but a new dependency and guesses can still be wrong. |

**Answer: UTF-8, else Windows-1252**

### An entry's file exists on disk but isn't in the library (outside the music folders, or not scanned yet). What should import do with it?

| Option | What it means |
|---|---|
| Count it as unmatched (Recommended) | Playlists only hold library tracks today; the entry goes to the unmatched list with its path, and the owner can add the folder and import again. |
| Add the file to the library | Scans just that file in so the entry fits; the playlist is complete, but the library gets tracks from outside its folders that no rescan or watcher looks after. |
| Unmatched, with a hint | Same as the first, but the list says these files exist outside the music folders, so the owner knows to add a folder. |

**Answer: Count it as unmatched** - the file is never added to the library. The last
round below refines this: such an entry still tries tag matching first.

### When an entry's path doesn't match, how loosely should its #EXTINF "Artist - Title" be compared with library tracks?

| Option | What it means |
|---|---|
| Case and accents ignored (Recommended) | Trimmed, case-folded, diacritics removed, runs of spaces collapsed; an entry written without accents finds a track whose title has them. Catches files re-tagged or written by other players, small risk of wrong matches on near-identical titles. |
| Case-insensitive only | Trimmed and case-folded, otherwise exact; fewest wrong matches, but a missing accent or extra space leaves the entry unmatched. |
| Fuzzy similarity | Edit-distance score over artist and title, best above a threshold; finds typos and "(Remastered)" variants, but wrong matches become likely and the threshold needs tuning. |

**Answer: Case and accents ignored**

### Several tracks match the tags, or the #EXTINF length is far from every candidate. How is the length used?

| Option | What it means |
|---|---|
| Closest wins, any gap (Recommended) | Length only breaks ties: the candidate closest to the entry's seconds wins, however far; with no length (-1), the first in library order. Never loses a match, but a live version may stand in for a studio one. |
| Closest within a limit | Same, but a candidate more than e.g. 10 s off doesn't count, so the entry stays unmatched; avoids a wrong version, at the cost of misses when lengths are rounded oddly. |
| Unmatched when ambiguous | Tie-breaking by length is used only when exactly one candidate is closest by a clear margin; otherwise the entry is listed as unmatched for the owner to add by hand. |

**Answer: Closest wins, any gap**

### Playlist names are unique (case-insensitive). Importing "Road trip.m3u8" when a "Road trip" playlist exists - what happens?

| Option | What it means |
|---|---|
| New name with a number (Recommended) | Creates "Road trip 2" (then 3, ...), like New playlist does; never touches existing playlists, but re-importing a backup leaves a near-duplicate to delete by hand. |
| Ask: replace or keep both | A dialog offers replacing the existing playlist's entries or keeping both; handles restoring a backup well, but adds a dialog and a destructive path. |
| Open rename first | Imports under a numbered name and opens the playlist with its name field focused, as New playlist does, so the owner names it at once. |

**Answer: New name with a number**

### No entry in the file matches anything in the library (or the file has no entries). What should import do?

| Option | What it means |
|---|---|
| No playlist, say why (Recommended) | Nothing is created; the Playlists page says the file had no tracks from the library (and lists the entries). Avoids empty leftovers. |
| Empty playlist anyway | Creates it and shows the unmatched list on its page, same as a partial import; consistent, but leaves an empty playlist to delete. |

**Answer: No playlist, say why**

### Where does the import report (how many matched, which entries didn't) appear?

| Option | What it means |
|---|---|
| Note on the new playlist (Recommended) | The new playlist opens; a note under its header says "N of M entries imported" with the unmatched entries listed below, staying until another playlist opens, like the export note. Matches existing patterns; long lists push the tracks down. |
| Dialog after import | A modal lists the counts and every unmatched entry, closed with OK, then the playlist shows; easy to read a long list, but one more dialog and gone once closed. |
| Note with a collapsed list | Like the first, but the unmatched entries sit behind "Show N unmatched"; keeps the page tidy, one more control to reach by keyboard. |

**Answer: Dialog after import**

### What does each unmatched entry show in that list?

| Option | What it means |
|---|---|
| Title, then path (Recommended) | The #EXTINF "Artist - Title" when present, with the path as the file wrote it in a dimmer line; enough to find the track and add it by hand. |
| Path only | The entry line as written; simplest, but phone exports with content:// or long relative paths are hard to read. |
| Both, plus a Copy button | As the first, plus a button copying the list to the clipboard for a text file or search; more UI and a clipboard path to test. |

**Answer: Title, then path**

### Every entry matched. Does the dialog still appear?

| Option | What it means |
|---|---|
| No, just open it (Recommended) | The new playlist opens and its track count speaks for itself; the dialog appears only when something didn't match, or nothing did. |
| Always show it | One predictable step every time ("All 24 entries imported"); an extra click on the common case. |

**Answer: No, just open it**

### Which extras belong in this change? Pick none to keep it to the issue's scope.

Several could be chosen.

| Option | What it means |
|---|---|
| None, issue scope only (Recommended) | Import button, matching and the report; anything else becomes its own issue. |
| MCP tool for agents | An import_playlist tool in vcmp-mcp; needs a review step like playlist deletions, since agents would create playlists from arbitrary files. |
| Import into Favourites | An option to like every matched track instead of making a playlist; pairs with exporting Favourites, but touches Last.fm loves too. |
| Drop a file on the window | Dropping an .m3u/.m3u8 onto the Playlists page imports it; handy, but Slint's file drop support varies per platform. |

**Answer: None, issue scope only**

### An entry's path points to an existing file that isn't in the library (e.g. a copy of the music on another drive). Should that entry still try tag matching before being listed as unmatched?

| Option | What it means |
|---|---|
| Yes, try tags too (Recommended) | Any entry whose path doesn't give a library track tries #EXTINF tags; recovers playlists pointing at another copy of the music, at the small risk of a wrong version. |
| No, unmatched right away | A path that names a real file outside the library is trusted as "not ours"; stricter and predictable, but such playlists come in mostly empty. |

**Answer: Yes, try tags too**

## High-level plan

**1. Reading an M3U file (vcmp-library).** Turns a file's bytes into a list of
entries: each has the location as the file wrote it and, when an `#EXTINF` line came
before it, the length and the "Artist - Title" text. Handles the encoding fallback,
a byte order mark, `\r\n` and `\n` line ends, blank lines and other `#` lines.
Depends on nothing; tested on its own with text written in the tests and with the
exporter's output.

**2. Matching entries to library tracks (vcmp-library).** For each entry: by path
first, then by tags. Returns the matched track ids in file order, repeats kept, and
the unmatched entries. Depends on 1.

**3. Creating the playlist (vcmp-library).** One library call that reads, matches
and, when anything matched, creates the playlist under a free name made from the
file name and fills it. It returns what the app needs for the report: the new
playlist (or none), the number of entries, and the unmatched ones. Depends on 2.

**4. The window (vcmp-app).** An "Import playlist..." button on the Playlists page,
the open dialog filtered to `.m3u` and `.m3u8`, the report dialog, and opening the
new playlist. Depends on 3.

**5. Tests and documentation.** Library tests for 1-3, app tests next to the
existing playlist tests for 4, the manual checks with files from VLC, foobar2000 and a
phone, and the documentation updates the request lists. Runs alongside 1-4.

## Architecture decisions

**All of it lives in `vcmp-library`, next to the writer.** The app only picks the
file and shows the result. Reading and writing the format side by side keeps the
round trip testable in one crate, and the MCP server could reuse it later without
touching the app.

**Encoding: UTF-8, else Windows-1252, decided per file.** A leading UTF-8 byte order
mark is dropped. If the rest is valid UTF-8, it is read as UTF-8, whatever the
extension says. Otherwise the whole file is read as Windows-1252. Deciding per file,
not per line, keeps a file consistent. Windows-1252 is the code page Western-European
Windows players commonly wrote legacy `.m3u` files in. Files in other legacy code pages
(CP1250 for Czech, CP1251 for Cyrillic) come out with wrong letters, so their paths
don't match and they fall back to tag matching, which fails too when the titles
hold those letters. An encoding detector was rejected as a new dependency for a rare
case. The system code page was rejected because the result would depend on the
machine.

**Paths.** An entry's location is turned into a candidate path:
- A `file://` URI (VLC writes these) is percent-decoded into a local path. Other
  URIs (`http:`, `content:`) have no path and go straight to tag matching.
- `\` and `/` are both separators on every platform. A relative path is resolved
  against the playlist file's folder, `.` and `..` are resolved lexically.
- When the file exists, the path is canonicalised like the scanner's (symlinks
  resolved, the real case on Windows and macOS), then looked up with
  `track_id_for_path`. When it doesn't exist, the lexical path is looked up as is,
  so an unavailable track still matches by its last known path, the same way the
  exporter wrote it.
- Any entry whose path gives no library track, whether its file exists or not, goes
  on to tag matching. If that fails too, it is unmatched; a file outside the library
  is never added to it (decisions above).

**Tags.** Only entries with an `#EXTINF` title reach tag matching. The text is split
at the first " - " into artist and title; without one, it is a title alone. Both
sides are compared after trimming, case-folding, removing diacritics (Unicode
decomposition, combining marks dropped) and collapsing runs of whitespace. Among
library tracks whose folded artist and title equal the entry's, the one whose length
is closest to the entry's seconds wins, however far it is; with no length (`-1` or
absent), the first in library order wins. The folding is done in Rust, not by FTS5,
because FTS5 finds words inside names and the match here must be whole-name
equality.

**Naming.** The playlist is named after the file name without its extension. When
that name is taken (ignoring case, as the database does), " 2", " 3" and so on are
added until it is free. Existing playlists are never changed. An empty name falls
back to the same default as "New playlist".

**No playlist without a match.** When nothing matched, or the file had no entries,
nothing is created, so a failed import leaves nothing to clean up.

**The report is a dialog, shown only when something didn't match.** A `Modal` on
the Playlists page, like the other dialogs, with "N of M entries imported" (or
"none of the entries are in the library" when nothing was created) and the
unmatched entries: the `#EXTINF` "Artist - Title" when there is one, with the path
as written in a dimmer line under it. It is closed with OK or Escape, and the
new playlist (when there is one) is open behind it. All strings go through
`@tr()`, colours and durations come from `ui/theme.slint`, and it is operable by
keyboard, as `docs/ui.md` requires.

**Import is an ordinary playlist creation.** No new table and no schema change. The
playlist row and its entries must be written in one transaction, so a failure
part-way leaves no empty or half-filled playlist. Today `create_playlist` and
`add_to_playlist` are separate calls, and only the entries are written in a
transaction.

## Weaknesses and risks

| Risk | Likelihood | Cost | What reduces it |
|---|---|---|---|
| A legacy `.m3u` in CP1250 or CP1251 reads with wrong letters, so its entries don't match by path or tags | Medium for Czech and Cyrillic owners with old Windows playlists | Those entries are listed as unmatched | Documented in `docs/library.md` under known limits; re-saving the file as `.m3u8` in the other player fixes it. A detector can be added later without changing anything else |
| Tag matching picks the wrong version (live versus studio, a remaster) because length only breaks ties | Medium on libraries with several versions | A wrong track in the playlist, unnoticed because the import reports it as matched | Owner removes it by hand; a length limit can be added later if this shows up |
| Diacritics-insensitive matching joins two different titles that differ only in accents | Low | A wrong track, as above | Accepted with the decision |
| An entry naming a file outside the library falls back to tags and picks up a library track that is a different recording | Low to medium (a playlist pointing at another copy of the music) | A wrong track, reported as matched | Accepted with the decision; the same tag rules and risks apply as above |
| Tie-breaks among equally close candidates (an available track versus an unavailable one with the same tags and length) | Low | The playlist may point at a missing file | Left to implementation: prefer an available track on an exact tie |
| Phone exports use paths from the phone (`/storage/emulated/0/Music/...`) or `content://` URIs that never match by path | High for phone files | Everything goes through tag matching; entries without `#EXTINF` are unmatched | Expected; checked by hand in `docs/manual-testing.md` |
| Case on Windows and macOS: a path written in other case than the stored one | Medium | Path match misses | Canonicalising existing files gives the real case; a missing file in other case falls back to tags |
| Large files (thousands of entries) | Low | A pause on the UI thread while matching | Library calls run on the UI thread like export; measure with a generated file of 10 000 entries before deciding to move it off |

## Out of scope

- Picking up M3U files in the music folders automatically, and keeping an imported
  playlist in step with its file.
- PLS, XSPF and other playlist formats.
- An MCP tool for agents to import playlists.
- Importing into Favourites.
- Dropping a playlist file onto the window.
- Adding files outside the music folders to the library during an import.
- Encoding detection beyond UTF-8 and Windows-1252.
- Fuzzy or similarity-based tag matching, and a length limit on tag matches.
- Replacing or merging into an existing playlist with the same name.
