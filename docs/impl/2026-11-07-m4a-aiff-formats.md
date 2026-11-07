# M4A and AIFF formats

Spec: docs/specs/2026-11-07-m4a-aiff-formats.md
Date: 2026-11-07
Status: implemented
Area: vcmp-playback (decoders, gapless tests), vcmp-library (formats, scan, tag
reading and writing, other tags, examples), vcmp-mcp (instructions), vcmp-app
(lossless mark test), docs

## What was done

- **Playback:** symphonia is built with `isomp4`, `aac`, `alac` and `aiff`.
  `make-fixtures.sh` makes `tone-a/b.aiff`, `.alac.m4a` and `.aac.m4a`. The decoding,
  seeking and engine gapless tests cover AIFF and ALAC.
  `aac_in_mp4_keeps_its_encoder_delay_and_padding` records the AAC limit:
  1024 frames of delay plus padding to whole packets, 1119 extra frames (25 ms)
  between the fixture halves.
- **Formats and scan** (`tags.rs`): `Format` gains `Aiff`, `Aac` and `Alac` (`aiff`,
  `aac`, `alac` in `tracks.format`). `is_lossless` is true for ALAC and AIFF, and the
  player bar shows "Lossless (ALAC)" and "Lossless (AIFF)". The scanner takes
  `.aif`, `.aiff`, `.aifc` and `.m4a` as candidates. The format comes from lofty's
  file type and, for MP4, its codec. An MP4 holding MP3 or FLAC is skipped. So is
  one with a `vide` track handler, found by a small box walker since lofty reads
  only the audio track.
- **Tag reading:** AIFF reads ID3v2 like WAV, with the text chunks as a fallback.
  MP4 lists lofty's common names, plus `mp4:<fourcc>` and
  `mp4:----:<mean>:<name>` for the rest. Numbers and flags are listed in decimal.
- **Tag writing** (`edit.rs`, `other_tags.rs`):
  - AIFF joins the ID3v2 arm.
  - M4A edits the `ilst` in place. The album artist is `aART`, and the cover is the
    first `covr` picture, since MP4 keeps no picture types.
  - Native `mp4:` keys replace their atom. An atom holding numbers or flags keeps
    that type, and one holding something else refuses the file.
  - Common names go through lofty's generic tag for the affected atoms and the atom
    the key is written to, so `trkn` and `disk` keep the part not edited.
  - Copy, read-back check, untouched-keys check and replace are unchanged.
- **Other:**
  - `edit_check` and `scan_profile` take the new extensions.
  - The MCP instructions name the `mp4:` prefix.
  - Docs: library.md, playback.md, ui.md, README.md, manual-testing.md and the
    fixture README.
- **Checks:**
  - `cargo fmt --all --check` and
    `cargo clippy --workspace --all-targets -- -D warnings` are clean.
  - At the owner's request the full `cargo test --workspace` was not run. Run by
    name and passing: the whole `vcmp-playback` suite (95 tests), the
    `other_tags` unit tests, the two new M4A tests in `tag_writing`, the four shared
    `tag_writing` tests extended to M4A, `an_invalid_other_tag_touches_no_file`,
    `undo_puts_back_every_value_as_it_was` (all seven formats), and the two scan
    tests.
  - Not run:
    - the other `tag_writing` tests now looping over AIFF;
    - `the_lossless_mark_shows_for_alac_and_aiff_but_not_aac` in `vcmp-app`;
    - everything else.

## How to test

**Automated coverage.** `cargo test --workspace`. The new cases:

- `cargo test -p vcmp-playback --lib local`
- `cargo test -p vcmp-library --test tag_writing m4a`
- `cargo test -p vcmp-library --test scan reads_every_supported_format`
- `cargo test -p vcmp-app --test player the_lossless_mark`

**Setup.**
- A folder of copies of real music (never the collection itself) holding:
  - ALAC and AAC `.m4a` files, iTunes purchases among them if you have any;
  - `.aiff` files;
  - an `.m4a` music video;
  - an `.mp4` file.
- Start with `VCMP_HOME=target/m4a-check cargo run -p vcmp-app` and add the folder
  in the setup wizard.

**Steps.**
1. Let the scan finish. The ALAC, AAC and AIFF files are listed on Songs with
   their tags and covers. The music video and the `.mp4` file are not listed.
2. Play an ALAC album through a track change. The player bar shows the waveform
   mark, and pointing at it says "Lossless (ALAC)". There is no gap between tracks.
3. Play an AIFF track. The mark says "Lossless (AIFF)".
4. Play an AAC album through a track change. There is no lossless mark, and a gap
   of 23 to 46 ms is audible between tracks on a live album (the documented
   limit).
5. Open the tag editor on an ALAC track. Change the title, album artist, year and
   genre, set a new cover, and save. The song list and album page show the new
   values and cover.
6. Open the file in iTunes or Music, foobar2000 or Mp3tag. They show the new values
   and cover, and the iTunes fields (`stik`, purchase IDs, `----` atoms) are still
   there.
7. Do the same on an AIFF file. Other players read the new ID3v2 values.
8. Run `cargo run --release -p vcmp-library --example edit_check -- <that folder>
   40`. It should end with "0 failed edits".

**Also worth trying.**
- With agents on, `get_tracks` on an M4A track lists `mp4:` keys. Proposing a change
  to `mp4:----:com.apple.iTunes:MY FIELD` writes it once accepted, and undo removes
  it.
- Proposing text for a number atom (`mp4:stik` = `music`) fails that file with a
  write error, and the file is unchanged.
- An M4A whose first picture is not the front cover: saving a new cover replaces
  that picture (the documented MP4 limit).
- Seeking in an AAC track lands about 23 ms early in the music (the encoder delay).

## What is missing

- `edit_check` runs on real M4A and AIFF files, which the issue asks for before
  merging. Only the owner's collection can supply them.
- A full `cargo test --workspace` run. The owner runs it once the other sessions
  are merged.

## Known problems

- AAC in MP4 is not gapless: 23 to 46 ms between tracks, and durations include the
  padding. symphonia reads neither the edit list nor `iTunSMPB`.
- In an M4A file the cover is always the first picture. A file with a back cover
  first loses that picture on a cover edit.
- lofty writes MP4 numbers in as few bytes as fit, so `tmpo` or `stik` written by
  VCMP may be shorter than the four bytes iTunes writes. Whether iTunes reads them
  is not checked here.
- `AppleXid` is no longer refused before files are touched: MP4 holds it, so it
  fails per MP3, FLAC or AIFF file instead. The old example in the
  invalid-edit test is replaced by two `mp4:` cases.
- AIFF-C with A-law or mu-law counts as lossless for the mark, though those
  encodings are lossy. Such files are rare.
- An `.m4a` with chapter pictures in a video track (some enhanced podcasts) is
  skipped as video.
- `stop_and_the_end_of_the_queue_clear_stop_after_the_current_track` failed once
  in four `vcmp-playback` runs, on a WAV file this change does not touch. It is a
  timing flake that predates this change.

## Recommended follow-ups

- Read `iTunSMPB`, or the MP4 edit list, to trim AAC (and MP3 with only
  `iTunSMPB`), so iTunes albums play gaplessly.
- Keep each MP4 number atom's original byte width when writing, if lofty gains a
  way to, or by writing `AtomData::Unknown` with the old width.
- Accept `.mp4` files that hold only audio, if the owner wants them later.

## Changelog

- Play M4A (AAC and ALAC) and AIFF files, and show them in the library
- Edit the tags and cover of M4A and AIFF files, including iTunes' own fields
- Show the lossless mark for ALAC and AIFF tracks
- Skip `.m4a` music videos and MP4 files that hold neither AAC nor ALAC
