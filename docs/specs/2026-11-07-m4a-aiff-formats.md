# M4A and AIFF formats

Status: draft
Date: 2026-11-07
Area: vcmp-library (formats, scan, tag reading and writing, other tags), vcmp-playback
(decoders, gapless), vcmp-app (lossless mark), vcmp-mcp (tag key descriptions)

## Context of the change

VCMP plays and tags MP3, FLAC, Ogg Vorbis and WAV only. The agreed spec
[2026-10-22-listening-and-library-features.md](2026-10-22-listening-and-library-features.md)
("More formats" and "Formats") already chose to add M4A (AAC and ALAC) and AIFF, read
and write; this change is that part, tracked as issue #80. This spec settles what the
earlier one left open.

What exists today:

- `Format` (`crates/vcmp-library/src/tags.rs`) has `Mp3`, `Flac`, `OggVorbis` and
  `Wav`. The scanner keeps a file when its extension maps to a format and lofty's
  detected type agrees; an Opus stream in an `.ogg` file is skipped that way. The
  format is stored per track as text (`tracks.format`, no constraint on its values).
- `Format::is_lossless` and `Format::name` drive the player bar's "Lossless (FLAC)"
  mark ([docs/ui.md](../ui.md)).
- Tag writing (`edit.rs`) has one arm per tag kind: ID3v2 for MP3 and WAV, Vorbis
  comments for Ogg and FLAC. Each goes through copy, edit, read-back check and
  replace ([docs/library.md](../library.md), "Tag editing").
- "Other tags" (`other_tags.rs`, the MCP tools' `track_tags` and tag edits) know two
  kinds of native key: `id3:` and `vorbis:` ([docs/library.md](../library.md#other-tags)).
- symphonia 0.6.1 is built with `mp3`, `flac`, `pcm`, `wav`, `ogg` and `vorbis`. It
  offers `isomp4`, `aac`, `alac` and `aiff`. lofty 0.25 reads and writes MP4 `ilst`
  atoms and ID3v2 in AIFF, and reports an MP4 file's codec (`Mp4Codec`: AAC, ALAC,
  MP3, FLAC).
- Gapless playback trims encoder delay per format ([docs/playback.md](../playback.md),
  "Gapless playback"); symphonia has no delay or padding for AAC in MP4.
- Scans at startup (while `library.auto_update` is on) find files never seen before,
  so libraries that hold skipped M4A and AIFF files pick them up without a forced
  rescan.

## User request

Brainstorm and implement the feature described in issue #80, "Play and tag M4A (AAC,
ALAC) and AIFF files". VCMP plays only MP3, WAV, FLAC and Ogg Vorbis, and the scanner
skips M4A (AAC and ALAC) and AIFF files; the earlier spec adds both, reading and
writing.

- Decoding: symphonia gains `isomp4`, `aac`, `alac` and `aiff`.
- Scanning: the scanner accepts `m4a`, `aif`, `aiff` and `aifc`, checked against
  lofty's detected type. An MP4 that holds video is skipped.
- Tags: `Format` gains M4A and AIFF. Tags are written as MP4 atoms for M4A (covers
  as `covr`) and as ID3v2 for AIFF, through the same copy, read-back check and
  replace as today. The "every untouched key reads back unchanged" check must cover
  MP4 freeform atoms.
- Fixtures: the fixture scripts generate the new formats (change the scripts, not
  the files).
- Gapless: tests cover ALAC and AIFF. AAC's gapless behaviour is measured and
  documented as a known limit.
- Lossless mark: the player bar's mark shows for ALAC and AIFF, not for AAC.
- Before merging, run `--example edit_check` on real M4A and AIFF files (copies only,
  never the real collection); MP4 tag writing is lofty's least tested path.
- Out of scope: Opus, WMA, APE, WavPack, `.m4b` audiobooks and MP4 video.
- Also update `docs/library.md`, `docs/playback.md` (formats, the AAC gapless
  limit), `README.md` if it lists formats, and `docs/manual-testing.md`.

The work happens on its own feature branch in its own worktree.

## Decisions

### M4A is a container holding either AAC (lossy) or ALAC (lossless). How should VCMP's `Format` (stored per track in the database, used for the player bar's lossless mark) represent it?

| Option | What it means |
|---|---|
| Aac and Alac (Recommended) | Two variants, like OggVorbis names the codec; the lossless mark and its label ("ALAC") fall out for free, but the scanner must read the codec before it knows the format. |
| One M4a + codec column | Format stays a container; a separate stored codec drives the mark. Cleaner container model, but a schema change and two fields to keep in sync. |
| One M4a, codec at play | No schema change; the playback engine reports the codec for the mark. Simplest, but the library can never tell AAC from ALAC (no filtering, no MCP info). |

**Answer: Aac and Alac**

### Should the scanner also accept `.mp4` files that hold only audio, or just `.m4a` (plus `.aif`, `.aiff`, `.aifc`) as the issue lists?

| Option | What it means |
|---|---|
| Only .m4a (Recommended) | Matches the issue and spec; no video detection on `.mp4` files, but rare audio-only `.mp4` downloads stay invisible. |
| Also audio-only .mp4 | Catches audio saved as `.mp4`; every `.mp4` in a music folder must be opened to check for a video track, slower if videos live there. |

**Answer: Only .m4a**

### M4A tags need a native key prefix for the "other tags" list and the MCP tools, next to today's `id3:TXXX:MY FIELD` and `vorbis:MYFIELD`. MP4 has fourcc atoms (`(c)wrk`, `tmpo`) and freeform atoms (`----` with a mean and a name, mostly `com.apple.iTunes`). How should they be keyed?

| Option | What it means |
|---|---|
| mp4: full form (Recommended) | `mp4:(c)wrk` and `mp4:----:com.apple.iTunes:MY FIELD`, mirroring `id3:TXXX:desc`; every atom readable and writable, keys are long but unambiguous. |
| mp4: short freeform | `mp4:MY FIELD` for com.apple.iTunes freeform atoms, `mp4:(c)wrk` for fourccs; friendlier for agents, but freeform atoms of other means need a second spelling. |
| Read-only MP4 keys | List MP4 native keys but refuse writing them (common names like Composer still write); least risk on lofty's weakest path, a visible gap for agents. |

**Answer: mp4: full form**

(`(c)` stands for the copyright sign, byte 0xA9, which the keys carry as the real
character; the spec is ASCII only.)

### symphonia gives no encoder delay or padding for AAC in MP4, so AAC tracks may play with short gaps (live albums). The issue says measure and document. Should this change do more?

| Option | What it means |
|---|---|
| Measure and document (Recommended) | As the issue says: a test records the gap, docs/playback.md lists it as a known limit; no new engine code, AAC albums keep small gaps. |
| Trim from iTunSMPB | Read iTunes' iTunSMPB atom and trim AAC in the engine (could later serve MP3 too); real gapless for iTunes files, more engine code and tests, files without it still gap. |
| Trim from edit list | Parse the MP4 `elst` edit list that encoders like FFmpeg write; standard and broad, but needs own MP4 atom parsing next to symphonia. |

**Answer: Measure and document**

## High-level plan

**Playback.** Turn on symphonia's `isomp4`, `aac`, `alac` and `aiff` features. The
engine already probes content, so the new files play once decoders exist. Fixtures
for AAC, ALAC and AIFF come from `make-fixtures.sh`; the decoding and gapless tests
extend to ALAC and AIFF, and a test measures the AAC gap so the documented limit
states a number. No dependency on the library work.

**Formats and scanning.** `Format` gains `Aac`, `Alac` and `Aiff`. The scanner
accepts `.m4a`, `.aif`, `.aiff` and `.aifc` and takes the format from what lofty
reads: an MP4 holding AAC or ALAC, an AIFF lofty detects as such. Library fixtures
(silent files and the README's commands) gain the three formats. The lossless mark
follows from `is_lossless` (ALAC and AIFF yes, AAC no).

**Tag reading.** Every MP4 atom VCMP lists is read through lofty's MP4 type: common
names where lofty has one, `mp4:` native keys for the rest. AIFF reads its ID3v2 tag
like WAV, falling back to the AIFF text chunks the way WAV falls back to RIFF INFO.
Depends on the formats work.

**Tag writing.** A third arm for MP4 atoms (covers in `covr`, JPEG or PNG as today)
and AIFF joining the ID3v2 arm. Both use the existing copy, read-back check and
replace, and the untouched-keys check covers MP4 freeform atoms. Other tags gain the
`mp4:` key kind, and the MCP tool descriptions name it. Depends on tag reading.

**Verification and docs.** `edit_check` and `scan` examples accept the new
extensions; runs on copied real M4A and AIFF files before merging. `docs/library.md`,
`docs/playback.md`, `README.md` (if it lists formats) and `docs/manual-testing.md`
are updated. Last, since it needs everything above.

## Architecture decisions

| Decision | Why it beat the alternative |
|---|---|
| `Format` names the codec: `Aac` and `Alac` are separate variants, stored as `aac` and `alac` in `tracks.format`; AIFF is `Aiff` (`aiff`) | Matches how `OggVorbis` names its codec; `is_lossless` and `name` stay one match each, and gapless runs already split where the format changes. A container-level `M4a` would need a second stored field to drive the mark. |
| The extension decides only whether a file is a candidate; the format comes from lofty's file type and, for MP4, its `Mp4Codec` | An `.m4a` extension can't tell AAC from ALAC. An MP4 holding MP3 or FLAC, or anything lofty doesn't read as AAC or ALAC, is skipped like an Opus stream in `.ogg`. |
| `.m4a` only; `.mp4` files are never candidates | Matches the issue; no need to open every video in a music folder. An `.m4a` that holds a video track is still skipped, as the issue asks. |
| MP4 native keys are `mp4:` plus the atom: `mp4:<fourcc>` (with `(c)` as the real copyright sign) and `mp4:----:<mean>:<name>` for freeform atoms | Mirrors `id3:TXXX:<description>`: one spelling for every atom, nothing ambiguous between means, and every key read is a key that can be written. |
| AIFF tags are ID3v2, through the same code as WAV; AIFF text chunks are read as a fallback and left untouched on write | Same rules and same known limit as RIFF INFO in WAV; nothing new for the edit path. |
| A tag VCMP writes into an M4A without an `ilst` is created and seeded from what the scan read, and the untouched-keys check is skipped for that file | Same rule as creating ID3v2 in an MP3 with only ID3v1. |
| AAC gapless is measured and documented, not fixed | The issue's scope; trimming from `iTunSMPB` or the edit list is a separate change. |
| No schema version bump | `tracks.format` takes any text and the new files were never stored, so startup and manual scans add them. |

Invariants for tests:

- Every `Format` variant has a fixture in both fixture sets, plays, scans and writes
  tags with its cover.
- The untouched-keys check passes with MP4 freeform atoms present and fails when
  one is lost.
- `.m4a` holding video, and an MP4 with a codec other than AAC or ALAC, is skipped.
- The lossless mark shows for ALAC and AIFF and not for AAC.

## Weaknesses and risks

| Risk | Likelihood | Cost | What reduces it |
|---|---|---|---|
| MP4 tag writing is lofty's least tested path; freeform atoms or `covr` written wrong | Medium | File damage, prevented by the read-back check, but edits refused | The read-back check refuses to replace a file that reads back wrong; `edit_check` on copied real M4A files before merging. Whether the owner's library has enough M4A and AIFF files to test with is not known yet |
| AAC tracks play with short gaps between them | High for AAC | Audible on live and classical albums | Measured by a test, documented as a known limit; trimming from `iTunSMPB` or the edit list is a later change |
| Telling an `.m4a` with video apart needs more than lofty, which reports no video tracks | Medium | A music video's audio shows up in the library | Detect it during implementation (symphonia's track list or the MP4 `hdlr` atoms); a fixture with a video track covers it |
| AIFF-C with a compression symphonia can't decode (`ulaw`, `alaw`, `ima4`...) scans fine and fails to play | Low | The track is skipped at play time | Behaves like any undecodable file today; listed as a known limit. Big-endian and `sowt` PCM, the common cases, decode |
| AIFF text chunks keep their old values after an edit, and players preferring them show stale tags | Low | Confusing in other players | Same as RIFF INFO in WAV; documented |
| Keys with the copyright sign are awkward for agents to type | Low | Agents copy keys from `track_tags`, so they rarely type them | The MCP tool description shows an example |
| Same album in M4A and FLAC merges into one album with doubled tracks | Medium | Noise | Already a known limit for other format pairs; not changed here |

Left open: how video in `.m4a` is detected (implementation detail, the behaviour is
decided); the measured AAC gap, which goes into `docs/playback.md` once measured.

## Out of scope

- Opus, WMA, APE, WavPack, `.m4b` audiobooks and MP4 video, as in the earlier spec.
- `.mp4` files holding audio only.
- MP4 holding MP3 or FLAC.
- Trimming AAC (or MP3) from `iTunSMPB` or MP4 edit lists.
- Writing AIFF text chunks, or the other tags in a file (ID3v1, APE, RIFF INFO).
- Fixing album grouping across formats.
