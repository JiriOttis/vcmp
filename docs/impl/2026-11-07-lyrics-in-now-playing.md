# Lyrics in now playing

Spec: docs/specs/2026-11-07-lyrics-in-now-playing.md
Date: 2026-11-07
Status: implemented
Area: vcmp-library, vcmp-integrations, vcmp-app (player service, now playing view,
player bar, Settings, setup wizard)

## What was done

- **vcmp-library** (`src/lyrics.rs`): the lyrics model, the LRC parser, reading SYLT,
  USLT and the Vorbis lyrics fields plus the `.lrc` file at play time, the priority
  function `choose_lyrics`, and the LRCLIB cache (`lyrics_lookups`, schema version
  14) with `lyrics_lookup_due` for the "same question" and 30-day miss rules.
- **vcmp-integrations** (`src/lrclib.rs`): the LRCLIB client and `LyricsFinder`, a
  worker that makes one request at a time, keeps only the latest waiting job and
  spaces requests 300 ms apart.
- **vcmp-app**: `src/player_service/lyrics.rs` reads lyrics off the UI thread when a
  track starts, asks LRCLIB through the cache, times line changes from positions
  plus a timer, and saves `ui.lyrics_shown` and `integrations.lyrics_online`.
  `ui/lyrics.slint` (`LyricsView`) is the new lyrics list. `ui/now-playing.slint` has
  the lyrics mode and its toggle, the player bar has the lyrics button and the
  "Find lyrics online" row is in Settings and the wizard. The new speech-bubble
  glyph is `ui/icons/lyrics.svg`.
- **Tests written:** unit tests for the parser, SYLT joining, encodings, priority
  and due rules (`vcmp-library`), the LRCLIB client against a fake transport
  (`vcmp-integrations`), fixture tests for every embedded form, the `.lrc` file and
  the cache (`crates/vcmp-library/tests/lyrics.rs`), and app tests for the wiring
  (`crates/vcmp-app/tests/lyrics.rs`).
- **Checks run:** `cargo check --all-targets` and `cargo clippy --all-targets -- -D
  warnings` on the three crates, both clean, and `cargo fmt --all`. **No tests were
  run**, at the owner's request (they run them after the open sessions merge), and
  the screenshots example wasn't run either.

## How to test

**Automated coverage.** `cargo test --workspace` runs everything. For this change
alone: `cargo test -p vcmp-library lyrics`, `cargo test -p vcmp-integrations lrclib`
and `cargo test -p vcmp-app --test lyrics`, plus `--test ui_invariants` and
`--test ui_behaviour` (the 720 px check covers the wider player bar). The
repository has no end-to-end suite beyond these headless Slint tests.
`VCMP_SHOT_ONLY=lyrics cargo run -p vcmp-app --example screenshots` takes
`now-playing-lyrics-light` and `-dark`.

**Setup.** Copy an album into a scratch folder, then start the app on a fresh
library: `VCMP_HOME=target/lyrics-try cargo run -p vcmp-app`, and add the folder in
the wizard. Next to one FLAC (say `01 Song.flac`) save `01 Song.lrc` with:

```
[offset:0]
[00:01.00]First line
[00:04.00]Second line
[00:07.00][00:12.00]Chorus line
[00:10.00]Fourth line
```

**Steps.**

1. Play another track of the album, one without an `.lrc` file. In the player bar,
   the speech-bubble "Lyrics" button left of "Now playing" is greyed.
2. Play `01 Song`. The "Lyrics" button is enabled.
3. Press it. Now playing opens with an 80 px cover, the title, artist and album at
   the top, and the five lines below. "First line" is bold in the accent after 1 s,
   "Second line" at 4 s, "Chorus line" at 7 s, "Fourth line" at 10 s, "Chorus line"
   again at 12 s. The highlighted line glides to the middle of the list.
4. Click "Second line": playback jumps to 0:04 and the highlight follows.
5. Press Tab until the lines get a focus ring, press Down twice and Enter: playback
   jumps to that line's time.
6. Press the "Show lyrics" button at the top right (left of the three dots): the
   view shows the large cover. Press the bar's "Lyrics" button: the view switches
   back to the lyrics. Press it again: the view closes.
7. Quit and start again, open now playing while `01 Song` plays: it opens on the
   lyrics (the choice is saved).
8. Settings, Playback: "Find lyrics online" is off. Turn it on, play a well-known
   track whose files have no lyrics: within a few seconds the lyrics appear (synced
   when LRCLIB has them), and the open view switches to them.

**Also worth trying.**

- An MP3 with plain USLT lyrics: they show as one text that scrolls with the wheel
  and Up/Down.
- With reduced motion on in Settings, the list jumps to each new line instead of
  gliding.
- A track LRCLIB doesn't know: nothing shows and, with `RUST_LOG=info`, playing it
  again sends no request; fixing its title in the tag editor sends one.
- Offline with the switch on: nothing shows, no error, and the next play after 10
  minutes asks again.
- The window at 720 px wide: the player bar's buttons, volume slider and title all
  fit.
- The setup wizard's Online services step lists "Find lyrics online".

## What is missing

Nothing: the spec is implemented in full. Running the tests and the screenshots in
both themes is left to the owner, as asked.

## Known problems

- Not run: the new tests may need fixes once run, mainly `crates/vcmp-app/tests/lyrics.rs`,
  which relies on the bar and view buttons existing in the headless window, and the
  SYLT fixture test, which relies on lofty writing a binary SYLT frame.
- The player bar didn't fit at 720 px with a fifth button. Below 800 px its gaps now
  narrow (24 to 16 px, 8 to 4 px) and the volume slider can shrink to 52 px; the
  owner should look at it (DESIGN.md, "Awaiting the owner's review").
- An `.lrc` file without any time stamps is read as plain text and ranks first
  among plain sources; the spec's order doesn't mention that case.
- Manually scrolling synced lyrics is undone at the next line change, which brings
  the line playing back to the middle.
- The lyrics view centres the line playing using its row's position after layout;
  a window resize doesn't re-centre until the next line.
- In lyrics mode the next track isn't shown; the header holds only the cover, title,
  artist and album.
- Every track start spawns a short thread to read its lyrics, even when the view is
  closed.

## Recommended follow-ups

- Run the tests and the screenshots, and have the owner review the lyrics layout and
  the narrower player bar at 720 to 800 px.
- Pause the follow-the-line scrolling for a few seconds after a manual scroll, as
  other players do.
- Detect `.lrc` encodings beyond UTF-8, UTF-16 and Latin-1 if users with
  Windows-1250 or CJK files report broken text.
- Wire Spotify tracks into the cache key (`LyricsKey`) when #86 lands.

## Changelog

- Show a song's lyrics in the now playing view, from an `.lrc` file next to it or
  from its tags
- Highlight the line playing in synced lyrics and keep it in view; click a line to
  play from there
- Add a lyrics button to the player bar that opens now playing on the lyrics
- Add "Find lyrics online" in Settings and the setup wizard to look lyrics up on
  LRCLIB, off by default
