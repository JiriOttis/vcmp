# Lyrics in now playing

Status: agreed
Date: 2026-11-07
Area: vcmp-library (lyrics from tags and .lrc files, LRC parser, priority, lyrics
cache, schema version 14), vcmp-integrations (LRCLIB client), vcmp-app (now playing
view, player bar, Settings, setup wizard, player service); all three platforms

## Context of the change

VCMP shows no lyrics. The now playing view (`crates/vcmp-app/ui/now-playing.slint`,
[ui.md, Now playing view](../ui.md#now-playing-view)) holds the cover, title, artist,
album and the next track, and a comment marks it as the future place for lyrics.

The agreed spec [2026-10-20-lyrics-themes-spotify.md](2026-10-20-lyrics-themes-spotify.md)
planned lyrics as its workstream 2, after themes (done in #91). Issue #85 restates
that workstream. These decisions were taken there and are not asked again here:

| Decided in the 2026-10-20 spec | Answer |
|---|---|
| Sources | Embedded tags, a sidecar `.lrc` of the same name, LRCLIB online |
| Where lyrics are shown | In the now playing view, synced: the current line highlighted and followed, a click on a line seeks; plain lyrics scroll |
| What happens to LRCLIB results | Kept in VCMP's data only, never written to files |
| Priority | `.lrc`, embedded synced, LRCLIB synced, embedded plain, LRCLIB plain; LRCLIB asked only when nothing synced is local |
| When LRCLIB is asked | Automatically, behind a "Find lyrics online" switch, off by default |

What changed since that spec was agreed:

- **Plain tag lyrics are already in the database.** Since schema version 10 every
  scan stores every text item of a file's tag in `track_tags`
  ([library.md, All tags](../library.md#all-tags)), lyrics included: `UnsyncLyrics`
  or `id3:USLT:<description>` in ID3v2, `Lyrics` in Vorbis comments. They make up
  two thirds of that table's text on the reference library. Timed lyrics (ID3v2
  `SYLT`) are binary frames and are not stored. The old spec's reason for reading at
  play time, "the database doesn't grow by every song's text", no longer holds; the
  others (no rescan, an `.lrc` found without the watcher, SYLT needs the file anyway)
  still do.
- **The setup wizard** ([ui.md, Setup wizard](../ui.md#setup-wizard)) now has an
  "Online services" step that lists every switch that makes VCMP send something
  online.
- **The pattern to follow exists.** The Discord presence's cover lookup
  (`crates/vcmp-library/src/lookups.rs`, `CoverSearch` in
  `crates/vcmp-app/src/player_service.rs`) looks up the playing album once, stores
  "found" and "not found" in SQLite, retries misses after 30 days
  (`RETRY_NOT_FOUND`) and failures after 10 minutes (`RETRY_FAILED`), and is driven
  through a trait the tests replace with a recorder.
- **Position events** come from the engine every 250 ms
  (`POSITION_INTERVAL`, `crates/vcmp-playback/src/engine.rs`), and
  `Theme.reduced-motion` already turns every UI duration to 0.

## User request

Brainstorm and implement the feature described in issue #85, "Lyrics in the now
playing view, synced where possible": read and analyse the issue, then come back
with brainstorming questions. The work happens on a new feature branch in its own
worktree.

The issue asks for lyrics from embedded tags (SYLT and USLT in ID3v2, LYRICS in
Vorbis comments, through lofty), a sidecar `.lrc` of the same name and LRCLIB online,
read when a track starts playing rather than at scan time. One model holds the
source, optional timed lines and optional plain text; the LRC parser handles
`[offset:]` and several timestamps on one line and ignores word-level tags, and
embedded text in LRC form counts as synced. The priority is `.lrc`, embedded synced,
LRCLIB synced, embedded plain, LRCLIB plain, and LRCLIB is asked only when nothing
synced is local. The LRCLIB client lives in `vcmp-integrations` and the lookup in
`vcmp-library`: behind a "Find lyrics online" switch that is off by default, an exact
match on title, artist, album and duration within a tolerance, a VCMP User-Agent,
one request at a time; results, "not found" included, cached in SQLite by the
library track id, misses retried after 30 days and failures on a later play, written
source-neutral so Spotify tracks (#86) can use it later. The now playing view gets
lyrics: synced lyrics highlight the current line in the accent and follow it, a
click on a line seeks to it, plain lyrics scroll; sync uses the engine's position
events, interpolated between them; line changes scroll smoothly, or jump with
reduced motion on. Out of scope: Spotify's own lyrics, writing lyrics or `.lrc`
files, editing lyrics, lyrics search, word-by-word highlighting, a lyrics panel
outside now playing. The issue also asks for tests (the LRC parser, the priority
order, the cache and retry rules with a fake LRCLIB, each embedded form read from
fixtures), updates to `docs/ui.md`, `docs/library.md` and `docs/manual-testing.md`
(LRCLIB with real tracks), screenshots in both themes, every new string through
`@tr()`, and keyboard-reachable lines.

## Decisions

### What does this change include, compared with issue #85 as written?

| Option | What it means |
|---|---|
| Issue as written (Recommended) | Local sources, LRCLIB, cache and the synced column in one change. One PR to review, larger; the feature is complete when it lands. |
| Local first, LRCLIB later | This change reads tags and .lrc and shows them; LRCLIB, its switch and cache come in a second change. Smaller, offline-only first PR; two specs' worth of review. |
| Issue plus agent access | As written, plus an MCP tool so AI agents can read a track's resolved lyrics (with source). Agents already see plain tag lyrics in track_tags; adds .lrc/LRCLIB text to their view and more surface to secure. |

**Answer: Issue as written**

### Embedded plain lyrics (USLT, LYRICS) are already stored in track_tags at scan time. Where should the lyrics lookup take them from?

| Option | What it means |
|---|---|
| File at play time (Recommended) | One read of the file gives SYLT, USLT and LYRICS together, plus the .lrc next to it. Always matches the file now; one extra tag read per track start, off the UI thread. |
| track_tags, file for SYLT | Plain text from the database instantly, the file read only for SYLT and .lrc. Two code paths; stale if the file changed since the last scan. |

**Answer: File at play time**

### Where do the lyrics go in the now playing view?

| Option | What it means |
|---|---|
| Under the title (Recommended) | Wide: the text column keeps title, artist, album on top, lyrics fill the height below, next track at the bottom; cover unchanged. Narrow: cover shrinks, lyrics below the texts. No new controls; less room per line in narrow windows. |
| Third column | Wide: cover, texts, lyrics side by side; the cover shrinks to make room. Narrow: lyrics below as in the first option. Most lyrics visible at once; the cover gets smaller even for tracks people look at for the art. |
| Lyrics toggle | A Lyrics button in the view swaps the cover area for full-size lyrics (the cover goes small beside the title). Same in wide and narrow, most room for lyrics; one more control, lyrics hidden until pressed (or the choice is remembered). |

**Answer: Lyrics toggle**

### What does the view show for a track without lyrics?

| Option | What it means |
|---|---|
| Layout as today (Recommended) | No lyrics, no column: the view looks exactly as now. Uncluttered for untagged libraries; the layout changes between tracks with and without lyrics. |
| Empty state | The lyrics area stays, saying 'No lyrics for this track' (plus a hint to the online switch while it is off). Stable layout, discoverable switch; wasted space on most tracks of an untagged library. |

**Answer: Layout as today**

### Is the lyrics toggle remembered?

| Option | What it means |
|---|---|
| Remembered (Recommended) | Once on, every track with lyrics shows them, across tracks and restarts (a saved setting); a track without lyrics shows the cover layout and hides the button. Set once; someone who wants the cover must switch it off. |
| Per opening | The view always opens on the cover; lyrics stay on only until it closes. Cover first every time; lyrics fans press it on each opening. |

**Answer: Remembered**

### Where can lyrics be turned on?

| Option | What it means |
|---|---|
| In the view only (Recommended) | A Lyrics button inside now playing, near the close button. Player bar unchanged; reaching lyrics takes two clicks from a page. |
| View and player bar | Also a lyrics button in the player bar that opens now playing straight on the lyrics. One click from anywhere; one more icon in a bar that already shrinks at 720 px. |

**Answer: View and player bar**

### How does the player bar's lyrics button behave for a track without lyrics?

| Option | What it means |
|---|---|
| Disabled (Recommended) | Always in its place, greyed when the track has no lyrics (or while they are still being looked up). Bar never shifts; a dead button on most tracks of an untagged library. |
| Hidden | Appears only when the track has lyrics. Clean bar; the buttons beside it move between tracks. |

**Answer: Disabled**

### Where does the 'Find lyrics online' switch appear?

| Option | What it means |
|---|---|
| Settings and wizard (Recommended) | A row in Settings plus a row in the setup wizard's Online services step, which lists every online switch. Consistent with Discord, Last.fm and updates; one more row in the wizard. |
| Settings only | Just the Settings row. Wizard unchanged; the wizard no longer lists everything VCMP may send online. |

**Answer: Settings and wizard**

### A track's title, artist, album or duration changes (tag edit, agent change, rescan) after LRCLIB answered for it. What happens to the cached answer?

| Option | What it means |
|---|---|
| Forget it (Recommended) | The cache row stores what was asked; a different question looks LRCLIB up again on the next play. Fixing tags fixes lyrics; one more request after edits. |
| Keep it | The answer stays until the 30-day retry (misses only). No extra requests; a found result from bad tags stays wrong forever. |

**Answer: Forget it**

### LRCLIB returns lyrics for the wrong song or version. Can the user do anything about it in the app?

| Option | What it means |
|---|---|
| Nothing in this change (Recommended) | Fix the tags (which, with 'forget it', triggers a new lookup) or add an .lrc, which wins. No new UI; a correct-tag mismatch stays until LRCLIB changes. |
| 'Not these lyrics' action | A small action on online lyrics that hides them and stops asking LRCLIB for that track. Direct fix; one more control and a rejected state in the cache. |

**Answer: Nothing in this change**

## High-level plan

One change, in four workstreams. The first two have no UI and can be built and
tested on their own; the view needs both.

**1. Local lyrics (vcmp-library).** The lyrics model, the LRC parser, and the
reading of a track's lyrics from its file and its `.lrc` at play time: SYLT and USLT
in ID3v2, the lyrics fields of Vorbis comments, each turned into timed lines when it
is in LRC form. Tests use new tiny fixtures holding each form. Depends on nothing.

**2. LRCLIB and the cache (vcmp-integrations, vcmp-library).** The LRCLIB client
(exact `get` by title, artist, album and duration, VCMP User-Agent, one request at a
time), the cache table in a schema migration to version 14, the retry rules, and the
priority function that merges local lyrics with a cached LRCLIB answer. Tests run
against a fake LRCLIB. Depends on the model from workstream 1.

**3. Wiring (vcmp-app, player service).** When a track starts, its local lyrics are
read off the UI thread; if nothing synced is local and the switch is on, the cache
is consulted and LRCLIB asked when needed, through a trait the tests replace, as the
cover lookup does. The resolved lyrics and the current line reach the UI through
`PlayerState`. The current line comes from the position events, interpolated between
them. The "Find lyrics online" setting and the remembered lyrics toggle are stored
in `settings`. Depends on 1 and 2.

**4. The view and the controls (vcmp-app UI).** The lyrics mode of the now playing
view, its toggle, the player bar's lyrics button, the Settings row and the wizard
row. Then the docs (ui.md, library.md, manual-testing.md, DESIGN.md for the owner's
layout choices) and screenshots in light and dark themes. Depends on 3.

## Architecture decisions

### Model and sources

- **One model, in vcmp-library:** where the lyrics came from (`.lrc`, embedded,
  LRCLIB), timed lines (start time and text) when synced, plain text otherwise. It
  carries no Slint type and no LRCLIB type. Reason: the library and integrations
  crates don't depend on each other; the app joins them, as it does for covers.
- **The LRC parser** reads `[mm:ss.xx]` and `[mm:ss.xxx]` stamps, several on one
  line (the line repeats at each), `[offset:+/-ms]` (applied to every stamp), ignores
  other ID tags and word-level `<mm:ss.xx>` stamps, and sorts lines by time. Text is
  "in LRC form" when at least one line carries a time stamp; lines without a stamp
  in such text are dropped. Reason: taggers put LRC text into USLT and LYRICS, and
  the issue asks for it to count as synced.
- **Embedded lyrics are read from the file when the track starts**, never from
  `track_tags`. One tag read, on a background thread, gives SYLT, USLT and the
  Vorbis lyrics fields (`LYRICS`, `UNSYNCEDLYRICS`). lofty 0.25 keeps SYLT as a
  binary frame and parses it on request (`SynchronizedTextFrame::parse`); only SYLT
  with millisecond time stamps is used, a SYLT timed in MPEG frames is ignored. Among several frames, one without a description wins
  over described ones. Reason: the owner's choice; one code path that always matches
  the file now.
- **The `.lrc` file** is the track's path with the extension replaced by `lrc`
  (`.lrc` or `.LRC`). UTF-8 with or without a BOM and UTF-16 with a BOM are read;
  other bytes that aren't valid UTF-8 are read as Latin-1. Reason: no new dependency,
  and the common encodings of LRC tools.

### LRCLIB and the cache

- **The client lives in vcmp-integrations**, on ureq like MusicBrainz and Last.fm,
  with VCMP's existing User-Agent, a request timeout, and one worker thread that
  handles one request at a time and drops a queued request for a track no longer
  playing. It uses LRCLIB's exact `get` (title, artist, album, duration in seconds),
  whose server-side duration tolerance is 2 seconds; no search endpoint and no fuzzy
  match. A 404 (`TrackNotFound`) and an answer marked instrumental count as "not
  found". A 429 is a failure that waits at least for its `Retry-After`. Requests are
  sequential, as LRCLIB's documentation asks. Reason: a wrong match is worse than
  none, and the musicbrainz client already works this way.
- **The cache lives in vcmp-library**, a new table in schema version 14, keyed
  source-neutrally by source and track id (`local` and the library track id now;
  `spotify` and its URI later). Each row stores the question asked (title, artist,
  album, duration), the answer (synced text, plain text, or nothing found), and when.
  Failures are not stored; they are remembered in memory and retried on a later play
  after 10 minutes, as cover lookups are. Reason: the cover lookup's proven pattern,
  with a key that does not assume a library track.
- **A cached answer counts only for the question it answered.** When the track's
  title, artist, album or duration (whole seconds) differ from the stored question,
  the row is treated as absent and LRCLIB is asked again. A "not found" for the same
  question is retried after 30 days. Reason: the owner's "forget it"; tag edits,
  agent changes and rescans need no hook into the cache.
- **The priority is one pure function** of the local lyrics and the cached answer,
  tested on its own: `.lrc`, embedded synced, LRCLIB synced, embedded plain, LRCLIB
  plain. LRCLIB is asked only when the switch is on and nothing synced is local.
  While a lookup runs, local plain lyrics (if any) show; a synced answer then
  replaces them.

### Playback sync

- **The current line is computed in Rust**, from the last position event and the
  time since it while playing, and re-evaluated at the next line's start time, so a
  line changes on time instead of up to 250 ms late. A seek or pause takes the next
  position event as the new base. Reason: Slint has no clock of the engine's, and the
  player service already handles positions and seeks.
- **A click on a line, or Enter on it, seeks** to its start time through the
  existing seek path.

### The view and the controls

- **Lyrics mode is a state of `NowPlayingView`**, not a new page or panel. In lyrics
  mode the lyrics take the cover's place at full size and the cover shows small
  beside the title; wide and narrow windows use the same arrangement. The current
  line is in the accent (its job is "what plays now"); other lines in the text and
  muted colours from the theme. Line changes scroll with `Theme` durations, so
  reduced motion jumps. Plain lyrics are a scrollable text.
- **The toggle is remembered** as a `settings` key (proposed: `ui.lyrics_shown`).
  It applies to every track that has lyrics; a track without lyrics shows the view
  as today and hides the in-view toggle. Turning lyrics off is the toggle again.
  Lyrics that arrive after the track started (an LRCLIB answer a moment later)
  switch the open view from the cover layout to lyrics mode when they land, and
  enable the bar button then; a synced answer replacing local plain text keeps
  lyrics mode and swaps the text.
- **The player bar gets a lyrics button** beside the now playing and queue buttons,
  always present, disabled while the track has no lyrics or they are still being
  looked up. Pressed with the view closed, it opens the view on the lyrics and turns
  the remembered toggle on; pressed while the view shows lyrics, it closes the view;
  pressed while the view shows the cover, it switches to lyrics. It shows as active
  while the view shows lyrics. The ui.md design decision "now playing needs no
  controls of its own" gets the toggle as its one exception.
- **The lyrics lines are one tab stop** moved with Up/Down, Page Up/Down, Home/End
  and Enter to seek, following the track lists' keyboard pattern; their accessible
  labels are the line texts. Every new string goes through `@tr()`.
- **"Find lyrics online"** is a `settings` key (proposed: `integrations.lyrics_online`,
  absent means off) shown as one row component used by both Settings and the setup
  wizard's Online services step, as the Discord row is. Its description says that
  the title, artist, album and duration of playing tracks are sent to lrclib.net.

### Invariants a test enforces

- The LRC parser on stamps, multiple stamps, offset, word stamps and unstamped lines.
- Each embedded form (USLT, SYLT, Vorbis LYRICS, LRC text inside USLT and LYRICS)
  read from fixtures, and the `.lrc` found next to the file.
- The priority order over every combination of present sources.
- With the switch off, no request reaches the fake LRCLIB, whatever is cached.
- The cache: found and not-found stored; a changed question asks again; a miss
  retried only after 30 days; a failure retried on a later play only.
- The UI invariants already enforced (no colours or durations outside `theme.slint`,
  `@tr()`, contrast in every built-in theme) cover the new components.

## Weaknesses and risks

| Risk | Likelihood | Cost | Mitigation |
|---|---|---|---|
| The player bar gets crowded at 720 px with one more button | Medium | Low-medium: buttons squeezed or clipped | Check the 720 px layout in screenshots and settle how the bar narrows during the UI work (see the open questions) |
| A disabled lyrics button sits on most tracks of a library without lyrics | High for untagged libraries | Low: visual noise | The owner's choice for a stable bar; revisit if it feels dead in use |
| LRCLIB matches the wrong version (live, remaster) or misses because of tag spelling | Medium | Low | Exact `get` only, duration tolerance; fixing the tags asks again; an `.lrc` always wins. No in-app "wrong lyrics" action by decision |
| LRCLIB is slow, down or retired | Low-medium | Low: no online lyrics | Timeout, one worker, failures retried later, local sources first |
| The view jumps from the cover to lyrics a moment after a track starts, when an LRCLIB answer arrives | High with the switch on and an uncached track | Low: one layout change per track, once (later plays hit the cache) | Accepted with "layout as today"; the change fades with `Theme` durations, or jumps with reduced motion |
| A track LRCLIB lacks today may be added soon: its documentation says misses are picked up by a background fetcher, but VCMP retries a miss only after 30 days | Medium | Low | Keeps the cover lookup's rule; a shorter miss retry is a one-constant change if it matters in use |
| Reading the file at every track start costs a disk read (and a spin-up on a sleeping disk) | Medium on hard disks | Low: lyrics appear a moment late | Off the UI thread; the engine already opened the same file, so it is usually in the OS cache |
| SYLT timed in MPEG frames, or with odd encodings, is ignored | Low | Low: those tracks fall back to plain or online | Stated as a known limit in library.md |
| `.lrc` files in an encoding other than UTF-8, UTF-16 with BOM or Latin-1 (for example Windows-1250 or GBK) show wrong characters | Low-medium for non-Western libraries | Low-medium | Known limit; an encoding detector can come later if it matters |
| Synced lines drift (late position events, `.lrc` timed for another rip) | Medium | Low | Interpolation, `[offset:]`; no manual offset control in this change |
| The cache grows with every looked-up track's text | Low | Low: a few KB per track | Only played tracks with the switch on; rows keyed per track, replaced, never duplicated |
| Cache rows of tracks that no longer exist stay | Low | Very low | Tracks are kept as rows even when unavailable; no cleanup needed for now |

**Questions deliberately left open**

- The exact look of lyrics mode (line size, spacing, how many lines around the
  current one, where the small cover sits) is for the UI work and the owner's review
  in DESIGN.md, within the toggle layout decided here.
- The player bar's behaviour at its narrowest width is settled during the UI work
  against the 720 px screenshots.

## Out of scope

- Spotify's own lyrics, and the Spotify side of the lookup (the cache key is ready
  for it; #86 wires it).
- Writing lyrics into files or `.lrc` files, editing lyrics, lyrics search and
  word-by-word highlighting.
- A lyrics panel outside the now playing view.
- An MCP tool for agents to read resolved lyrics (agents still see plain tag lyrics
  in `track_tags`).
- A "not these lyrics" action, a manual sync offset control, and choosing between
  several LRCLIB candidates.
- Lyrics for formats VCMP doesn't play, and lyrics in tags VCMP doesn't read (APE,
  ID3v1, RIFF INFO).
