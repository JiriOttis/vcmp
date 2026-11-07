# Library check

Status: agreed
Date: 2026-11-07
Area: vcmp-library (queries, schema migration), vcmp-app (finder page, Settings entry, navigation, cover picker, file manager reveal)

## Context of the change

VCMP has no way to find duplicate tracks, or tracks and albums with missing tags or
covers. The listening and library features spec
([2026-10-22-listening-and-library-features.md](2026-10-22-listening-and-library-features.md),
"Library check" in its plan and architecture decisions) planned it, and GitHub issue
#81 describes it; its implementation notes list it as still missing.

What exists today and what this change builds on:

- `tracks` (`crates/vcmp-library/src/schema.rs`) holds title, artist, album, track
  number, year, duration and `available` per file. A track whose file has no title
  gets the file name as its title (`apply` in `scan.rs`), so `tracks.title` alone
  can't tell a real title from the fallback. `track_tags` holds every text item the
  file has, and the title's key there is `TrackTitle`, so a track without that item
  has the file-name title.
- `albums.artwork_state` is 0 (not looked for yet), 1 (found: embedded or a folder
  image) or 2 (none found). `fetched_covers` holds covers the user fetched online,
  keyed by the album's group key. `albums.unknown = 1` marks the "Unknown" albums
  that tracks with no album tag fall into.
- Schema changes are positional migrations in `MIGRATIONS` (`schema.rs`); the last
  one merged adds `favourites`.
- Navigation (`crates/vcmp-app/src/navigation.rs`) remembers places (`Place`): a
  section, or an album, artist, playlist or label page. Settings is a section; the
  "Music folders" block is in `ui/library-settings.slint`.
- Edit tags (`src/tag_editor.rs`) can open for one track, a list of paths or an
  album. "Find cover online" (`ui/cover-picker.slint`, `src/cover_fetch.rs`) works
  only for the album the album page shows. The only "open in file manager" helper
  opens a folder (`themes.rs`); nothing selects a file in it.
- `unicode-normalization` is already in `Cargo.lock` through other crates.

## User request

Brainstorm and implement the feature described in GitHub issue #81, "Library check:
find duplicates and missing tags or covers", on a new feature branch in its own
worktree.

The issue asks for a finder page opened from Settings, Music folders, that is a
navigation place so Back works. Duplicates are available tracks grouped by title and
artist, ignoring case and accents, whose lengths lie within 2 s; "Not duplicates"
stores the group's track ids, a group that later gains a member shows again, and the
page explains the rule. Missing lists title, artist, album, track number and year per
track, using the `track_tags` table to tell a real title from the file-name fallback,
and missing covers per album. The actions are play, Edit tags..., show in the file
manager, Find cover online and Not duplicates. Nothing is deleted: the queries are
read-only in `vcmp-library`, the page never writes or removes music files itself, and
a test enforces this. The work touches `vcmp-library` (queries, and a table for "Not
duplicates" in a migration numbered after the merged ones) and `vcmp-app` (the page,
the Settings entry). Out of scope: deleting or moving files, audio fingerprints,
finding a missing album artist or genre, and fixing album grouping across formats.
The issue also asks for tests of the duplicate rule (case, accents, the 2 s window, a
group gaining a member after "Not duplicates"), of the missing-field detection
(file-name fallback) and of the no-write invariant; updates to `docs/library.md`,
`docs/ui.md` and `docs/manual-testing.md`; screenshots in both themes; and every new
string through `@tr()`.

## Decisions

### When should the Library check page work out its duplicates and missing tags or covers?

| Option | What it means |
|---|---|
| When page opens (Recommended) | Fresh queries every time the page opens and after each edit; nothing extra stored, but a big library may wait a moment on open |
| After each scan | Results kept up to date and a count shown on the Settings row, but needs storage or a cache that can go stale |
| Check button | Runs only when asked; gives a clear moment of work, but adds a step and results can be stale while the page is open |

**Answer: When page opens**

### Should items in the Missing lists (e.g. a track with no year on purpose) be hideable, like "Not duplicates"?

| Option | What it means |
|---|---|
| No, fix to clear (Recommended) | Matches the issue; no second table, but tracks that really have no year stay listed forever |
| Ignore per track+field | Stored like Not duplicates; a cleaner list, but a second table and a way to undo it |
| Filter toggles only | Per-field checkboxes on the page hide whole categories (e.g. year); nothing stored per track |

**Answer: No, fix to clear**

### Should AI agents (the MCP server) also get the library check findings?

| Option | What it means |
|---|---|
| Not in this change (Recommended) | Keeps the PR to library + UI; an agent tool can reuse the same queries later |
| Read-only tool too | Agents could find and propose fixes for missing tags now, but it adds mcp tool, docs/mcp.md and tests to this PR |

**Answer: Not in this change**

### Duplicates with the 2 s window: tracks A (3:00), B (3:01.5) and C (3:03) share title and artist. A-B and B-C are within 2 s, A-C is 3 s apart. How are groups formed?

| Option | What it means |
|---|---|
| Chain by gaps (Recommended) | Sort by length and start a new group at a gap over 2 s: A, B, C are one group; simple and stable, but a group can span more than 2 s |
| Within 2 s of shortest | Every member within 2 s of the group's shortest track: A+B grouped, C alone or with later tracks; strict, but which track starts a group shifts results |

**Answer: Chain by gaps**

### Which artist do duplicates match on, and what about tracks with no artist tag?

| Option | What it means |
|---|---|
| Track artist; skip none (Recommended) | Match the track's own artist tag; tracks without one are left to the Missing list, so untagged junk doesn't form big groups |
| Track artist; title only | Tracks without an artist still group by title alone; finds more, but many untitled 'Track 01' files may group wrongly |
| Album artist first | Use album artist when set, else track artist; misses a song that is on a compilation and on its own album |

**Answer: Track artist; skip none**

### Once a group is marked "Not duplicates", what happens later?

| Option | What it means |
|---|---|
| Hidden while subset; reset (Recommended) | Stays hidden while the current group is the stored ids or fewer (a file went missing); returns when a new member joins; a 'Show hidden groups' toggle on the page can undo it |
| Hidden while subset; no undo | Same rule, but no way to bring a dismissed group back except a new member joining |
| Exact match only | Hidden only while the group is exactly the stored ids; any change, even a lost member, shows it again |

**Answer: Hidden while subset; reset**

### How is the Library check page split between Duplicates, Missing tags and Missing covers?

| Option | What it means |
|---|---|
| Tabs (Recommended) | Three tabs with counts in their labels; each list scrolls on its own and stays short, but you see one kind at a time |
| One scrolling page | Three headed sections one after another; everything at a glance, but a long Missing list pushes covers far down |
| Summary then drill-in | Overview with three counts, each opening its own list; tidy, but one more click and more navigation places |

**Answer: Tabs**

### How are tracks with missing tags shown? (Missing year alone can be thousands of tracks.)

| Option | What it means |
|---|---|
| Grouped by album (Recommended) | One block per album listing its tracks with markers for the missing fields; fits fixing a whole album at once in Edit tags |
| Flat track table | One row per track with a column per field marked missing; sortable and dense, but loses the album context |
| One list per field | Sections 'No year', 'No track number'...; clear per problem, but a track missing three fields shows three times |

**Answer: Grouped by album**

### Albums with no album tag sit in an "Unknown" album per artist. Should those show under Missing covers?

| Option | What it means |
|---|---|
| Exclude them (Recommended) | An Unknown album is not a real release and Find cover online can't search it; its tracks already show under Missing album |
| Include them | Complete list, but entries you can't fix from the cover side |

**Answer: Exclude them**

### Play on a Library check row: what does it put in the queue?

| Option | What it means |
|---|---|
| Group/album context (Recommended) | In Duplicates it plays the group's tracks one after another (easy to compare); in Missing tags it plays the album from that track, like clicking a track on an album page |
| That track only | Replaces the queue with the single track; predictable, but comparing duplicates means clicking each one |
| Play next | Inserts the track after the current one and keeps the queue; least disruptive, but doesn't start playing at once |

**Answer: Group/album context**

### "Find cover online" is tied today to the album the album page shows. How should it work from the Missing covers tab?

| Option | What it means |
|---|---|
| Picker over the finder (Recommended) | The cover picker opens for that album on top of the Library check page; you stay in the list, but the picker is decoupled from the album page |
| Go to album page first | Opens the album page and its picker; no change to the picker, but you leave the list each time (Back returns) |

**Answer: Picker over the finder**

### "Show in file manager": today VCMP can only open a folder. How should a track's file be shown?

| Option | What it means |
|---|---|
| Select file everywhere (Recommended) | Explorer /select on Windows, open -R on macOS, D-Bus FileManager1.ShowItems on Linux (zbus is already in the stack), falling back to opening the folder |
| Select on Win/mac only | Linux just opens the folder with xdg-open; less code, but the file isn't highlighted there |
| Always open the folder | Reuses the existing helper; simplest, but in a big folder you still have to find the file |

**Answer: Select file everywhere**

### In the Missing tags tab (grouped by album), what does "Edit tags..." open?

| Option | What it means |
|---|---|
| Row or whole block (Recommended) | Each track row edits that track; the album block's header edits all its listed tracks at once (fill in a year for the whole album) |
| Multi-select rows | Select any rows (Ctrl/Shift) and edit them together, like the Songs page; flexible, but more selection code on a new page |
| Single track only | One track at a time; simplest, but filling a year for a 12-track album takes 12 edits |

**Answer: Row or whole block**

### What does each member of a duplicate group show, to help decide which copy to keep?

| Option | What it means |
|---|---|
| Album, format, length, path (Recommended) | Enough to tell a FLAC from an MP3 and a live album from a studio one; the path is the full file path, shortened in the middle |
| Plus bitrate and size | Better quality comparison, but needs bitrate read from files (not stored today) or a schema addition |
| Album and length only | Cleanest rows; format and location only through Show in file manager |

**Answer: Album, format, length, path**

### Tracks whose file is not there now (unplugged drive, deleted): should they appear under Missing tags?

| Option | What it means |
|---|---|
| Available only (Recommended) | Same as Duplicates; their tags can't be edited anyway, and a disconnected drive would flood the list |
| Include, marked | Shown greyed with 'File not found'; complete, but nothing on those rows can be acted on |

**Answer: Available only**

### Should tracks with only a file-name title be left out of duplicate grouping?

| Option | What it means |
|---|---|
| Yes, leave them out | Matches how tracks without an artist are handled; they show under Missing tags instead |
| No, include them | Two files both named "01 Intro" by the same artist with similar lengths would show as duplicates |

**Answer: Yes, leave them out**
## High-level plan

**1. Library queries and the "Not duplicates" table** (`vcmp-library`, first). A
migration adds the table for dismissed groups. A library-check module provides three
read-only queries over available tracks: duplicate groups (with the dismissed ones
marked, so the page can show them on request), tracks with missing fields grouped by
album, and albums without a cover. It also writes and clears "Not duplicates"
entries; that table is the only thing the module writes. Tests cover the duplicate
rule, the file-name fallback, the subset rule and the no-write invariant.

**2. Reusable actions decoupled from their pages** (`vcmp-app`, after 1 or alongside
it). "Find cover online" takes an album to search for instead of reading the album
page; "Show in file manager" gets a helper that selects a file on each platform,
falling back to its folder. Both keep working from their current places.

**3. The Library check page** (`vcmp-app`, after 1 and 2). A navigation place opened
from a button in Settings, Music folders. Three tabs with counts: Duplicates,
Missing tags, Missing covers. Each tab explains its rule in one line. The queries run
when the page opens and again after an edit, a cover change or a scan finishes.
Duplicates lists each group with its members (album, format, length, path) and Play
(the group), Edit tags..., Show in file manager and Not duplicates, plus a "Show
hidden groups" toggle that can undo it. Missing tags lists album blocks whose rows
mark the missing fields, with Play (album from that track), Edit tags... per row and
per block, and Show in file manager. Missing covers lists albums with Find cover
online, opened over the page.

**4. Documentation and screenshots** (with 3). The library docs (rules, the new
table), the UI docs (the page), the manual-testing list (file manager reveal on each
platform, the cover picker from the page), screenshots of each tab in both themes,
and the implementation notes.

## Architecture decisions

| Decision | Reason it beat the alternative |
|---|---|
| Findings are computed on demand when the page opens and after changes; nothing is cached | Read-only and always current; no stale counts. A scan-time cache would need storage and invalidation for a page opened rarely |
| Duplicate grouping runs in Rust, not SQL: fetch available tracks with an artist tag (id, title, artist, duration), fold title and artist (Unicode NFKD, combining marks removed, lowercase, surrounding whitespace trimmed), group by the folded pair, sort by length and start a new group at a gap over 2 s; groups of one are dropped | SQLite has no accent folding outside FTS, and the gap rule is a sequential scan. Chaining was chosen over "within 2 s of the shortest" because it gives stable groups independent of order |
| `unicode-normalization` becomes a direct dependency of `vcmp-library` | Already in `Cargo.lock` through other crates, so no new code is pulled in; the stack table in `CLAUDE.md` lists it |
| Tracks without a track artist, or without a real title (file-name fallback, by the same test as Missing title), are not grouped as duplicates | They appear under Missing tags; grouping by title alone or by file names would put unrelated "Track 01" or "01 Intro" files together |
| New table, one row per dismissed track: (group id, track id), with the group id shared by the tracks dismissed together | A current group is hidden while every member is in one stored group (subset rule): a member going missing keeps it hidden, a new member shows it again. "Show hidden groups" lists the hidden ones and removes a group's rows to undo. Track ids are stable across re-links, like favourites |
| Missing title is "no `TrackTitle` item in `track_tags`, or only blank ones"; the other four fields are their `tracks` columns being NULL (or blank text) | `tracks.title` always has a value because of the file-name fallback; `track_tags` records what the file really holds |
| Missing tags and Missing covers count only available tracks; Missing covers skips Unknown albums | Unavailable files can't be edited and a disconnected drive would flood the list; an Unknown album is not a release that can be searched for |
| An album lacks a cover when `artwork_state = 2` and it has no `fetched_covers` row; albums not looked at yet (state 0) are left out until a scan has looked | Folder images and embedded art already count as found (state 1); a fetched cover is shown in their place |
| The page is a new `Place` variant in navigation, not a Settings sub-state | Back and forward work like on every other page, and Settings stays a single section |
| `CoverSearch` takes the album it searches for from its caller | The page opens the picker over itself without navigating; the album page passes its own album |
| A file-reveal helper: `explorer /select,` on Windows, `open -R` on macOS, the `org.freedesktop.FileManager1.ShowItems` D-Bus call through zbus on Linux, and the folder through the existing helper when that fails | Selects the file where the platform allows; zbus is already a dependency, so no new crate |
| No-write invariant enforced by tests: running every check query leaves the music files' bytes and modification times unchanged and leaves every table other than the dismissed-groups table unchanged; the page's Rust glue has no path to file removal, and its only writes go through Edit tags and the cover picker | The issue's "no deleting" promise is a rule a later change could break silently |

## Weaknesses and risks

| Risk | Likelihood | Cost | Mitigation |
|---|---|---|---|
| Live or remastered versions with the same title and artist within 2 s show as duplicates; the same album in FLAC and MP3 already merges into one album with doubled tracks and shows every track as a duplicate | Medium | Noise | "Not duplicates", the album and path on each member, and the rule explained on the page |
| Chaining can build long groups from many slightly different lengths (several live versions 1-2 s apart) | Low | One large group | Members are listed with their lengths; "Not duplicates" hides the whole group |
| Open time on a big library: every available track is fetched and folded in Rust | Low-medium | Slow page open | Run the queries off the UI thread like other library work, show a busy state; measure on the owner's library and record the numbers in `docs/library.md` |
| Missing year or track number can list thousands of tracks | Medium | Long list | Grouped by album with Edit tags... per block; the list is virtualised like other long lists |
| Positional migrations collide if `feature/play-history` (or another schema branch) merges first | Medium | Renumbering, broken dev databases | Rebase and renumber before merging; test with a throwaway `VCMP_HOME` |
| Linux file managers without `FileManager1` (some minimal ones) | Medium on Linux | Folder opens without the file selected | Fallback to the folder; listed in `docs/manual-testing.md` per file manager tried |
| Explorer reports failure even when it opened the window (already seen in `themes.rs`) | High on Windows | A wrong error message | Don't show an error for Explorer's exit status, as the existing helper does |
| Decoupling `CoverSearch` from the album page could break the album page's picker | Low | Regression | Keep the existing UI tests for the album page picker passing; add one for the page |
| Accent folding by NFKD misses letters that don't decompose (for example "o" with stroke, "ss" for sharp s) | Low | A few missed duplicates | Accepted; documented as a known limit |

Left open, to settle during implementation: the exact wording of the rule texts and
where in the Music folders block the button sits (owner's call on wording, per
`docs/DESIGN.md`).
## Out of scope

- Deleting, moving or renaming music files, from the page or anywhere else.
- Audio fingerprints or any comparison of the audio itself.
- Finding a missing album artist or genre.
- Fixing album grouping across formats (the same album in FLAC and MP3 merging into
  one album with doubled tracks).
- Hiding individual Missing tags items; only fixing the tag clears one.
- Bitrate and file size on duplicate members.
- Unavailable tracks in Missing tags; Unknown albums in Missing covers.
- A count or badge on the Settings row, or any scan-time caching of findings.
- A tool for AI agents (MCP) exposing the findings; it can reuse the queries later.
- A sidebar entry for the page.
