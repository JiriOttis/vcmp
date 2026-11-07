//! Lyrics read from the fixtures' tags and from `.lrc` files next to them, and the
//! cache of LRCLIB's answers. Each test tags temporary copies of the fixtures.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use lofty::TextEncoding;
use lofty::config::WriteOptions;
use lofty::id3::v2::{
    BinaryFrame, Frame, FrameId, Id3v2Tag, SyncTextContentType, SynchronizedTextFrame,
    TimestampFormat, UnsynchronizedTextFrame,
};
use lofty::ogg::tag::VorbisComments;
use lofty::tag::TagExt;
use tempfile::TempDir;
use vcmp_library::{
    LocalLyrics, LyricLine, LyricsKey, LyricsQuery, LyricsText, OnlineLyrics, TrackId,
    read_local_lyrics,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn copy(dir: &TempDir, fixture: &str) -> PathBuf {
    let path = dir.path().join(fixture);
    fs::copy(Path::new(FIXTURES).join(fixture), &path).unwrap();
    path
}

fn line(ms: u64, text: &str) -> LyricLine {
    LyricLine {
        start: Duration::from_millis(ms),
        text: text.into(),
    }
}

fn uslt(description: &str, text: &str) -> Frame<'static> {
    Frame::UnsynchronizedText(UnsynchronizedTextFrame::new(
        TextEncoding::UTF8,
        *b"eng",
        description.to_owned(),
        text.to_owned(),
    ))
}

fn sylt(format: TimestampFormat, content: Vec<(u32, String)>) -> Frame<'static> {
    let frame = SynchronizedTextFrame::new(
        TextEncoding::UTF8,
        *b"eng",
        format,
        SyncTextContentType::Lyrics,
        None,
        content,
    );
    let data = frame.as_bytes(WriteOptions::default()).unwrap();
    Frame::Binary(BinaryFrame::new(FrameId::new("SYLT").unwrap(), data))
}

fn tag_id3v2(path: &Path, frames: Vec<Frame<'static>>) {
    let mut tag = Id3v2Tag::new();
    for frame in frames {
        tag.insert(frame);
    }
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

fn tag_vorbis(path: &Path, fields: &[(&str, &str)]) {
    let mut tag = VorbisComments::new();
    for (key, value) in fields {
        tag.push((*key).to_owned(), (*value).to_owned());
    }
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[test]
fn plain_uslt_in_an_mp3_and_a_wav() {
    let dir = tempfile::tempdir().unwrap();
    for fixture in ["silence.mp3", "silence.wav"] {
        let path = copy(&dir, fixture);
        tag_id3v2(
            &path,
            vec![
                uslt("iTunes", "Described\nlyrics"),
                uslt("", "Line one\nLine two"),
            ],
        );
        assert_eq!(
            read_local_lyrics(&path),
            LocalLyrics {
                embedded_plain: Some("Line one\nLine two".into()),
                ..LocalLyrics::default()
            },
            "{fixture}: the frame without a description wins"
        );
    }
}

#[test]
fn lrc_text_in_uslt_counts_as_synced() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy(&dir, "silence.mp3");
    tag_id3v2(&path, vec![uslt("", "[00:01.00]One\n[00:02.50]Two")]);
    let local = read_local_lyrics(&path);
    assert_eq!(
        local.embedded_synced,
        Some(vec![line(1000, "One"), line(2500, "Two")])
    );
    assert_eq!(local.embedded_plain, None);
}

#[test]
fn sylt_in_milliseconds_is_read_and_in_mpeg_frames_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy(&dir, "silence.mp3");
    tag_id3v2(
        &path,
        vec![
            sylt(
                TimestampFormat::MS,
                vec![(500, "First".into()), (1500, "Second".into())],
            ),
            uslt("", "Plain text"),
        ],
    );
    let local = read_local_lyrics(&path);
    assert_eq!(
        local.embedded_synced,
        Some(vec![line(500, "First"), line(1500, "Second")])
    );
    assert_eq!(local.embedded_plain.as_deref(), Some("Plain text"));

    let frames = copy(&dir, "silence.wav");
    tag_id3v2(
        &frames,
        vec![sylt(TimestampFormat::MPEG, vec![(10, "Frames".into())])],
    );
    assert_eq!(read_local_lyrics(&frames), LocalLyrics::default());
}

#[test]
fn vorbis_lyrics_fields_in_flac_and_ogg() {
    let dir = tempfile::tempdir().unwrap();
    let flac = copy(&dir, "silence.flac");
    tag_vorbis(&flac, &[("TITLE", "Song"), ("LYRICS", "Plain\nwords")]);
    assert_eq!(
        read_local_lyrics(&flac).embedded_plain.as_deref(),
        Some("Plain\nwords")
    );

    let ogg = copy(&dir, "silence.ogg");
    tag_vorbis(&ogg, &[("UNSYNCEDLYRICS", "[00:03.00]Timed")]);
    assert_eq!(
        read_local_lyrics(&ogg).embedded_synced,
        Some(vec![line(3000, "Timed")])
    );
}

#[test]
fn the_lrc_file_of_the_same_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = copy(&dir, "silence.flac");
    assert_eq!(read_local_lyrics(&path), LocalLyrics::default());

    fs::write(
        path.with_extension("lrc"),
        "\u{feff}[ti:Song]\n[00:00.50]Hello\n[00:01.00]World\n",
    )
    .unwrap();
    assert_eq!(
        read_local_lyrics(&path).lrc_file,
        Some(LyricsText::Synced(vec![
            line(500, "Hello"),
            line(1000, "World")
        ]))
    );

    fs::write(path.with_extension("lrc"), b"No times, caf\xE9\n").unwrap();
    assert_eq!(
        read_local_lyrics(&path).lrc_file,
        Some(LyricsText::Plain("No times, caf\u{e9}".into())),
        "not UTF-8, so read as Latin-1"
    );
}

#[test]
fn lrclib_answers_are_kept_per_track_and_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let library = vcmp_library::Library::open(
        &dir.path().join("library.sqlite3"),
        &dir.path().join("cache"),
    )
    .unwrap();
    let key = LyricsKey::local(TrackId(7));
    let query = LyricsQuery {
        title: "Song".into(),
        artist: "Band".into(),
        album: None,
        duration_secs: 181,
    };
    assert_eq!(library.lyrics_lookup(&key).unwrap(), None);

    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    library.set_lyrics_lookup(&key, &query, None, at).unwrap();
    let stored = library.lyrics_lookup(&key).unwrap().unwrap();
    assert_eq!(
        (stored.query, stored.found, stored.looked_up),
        (query.clone(), None, at)
    );

    let found = OnlineLyrics {
        synced: Some("[00:01.00]Hi".into()),
        plain: Some("Hi".into()),
    };
    let renamed = LyricsQuery {
        album: Some("Record".into()),
        ..query
    };
    library
        .set_lyrics_lookup(&key, &renamed, Some(&found), at)
        .unwrap();
    let stored = library.lyrics_lookup(&key).unwrap().unwrap();
    assert_eq!((stored.query, stored.found), (renamed, Some(found)));
    assert_eq!(
        library
            .lyrics_lookup(&LyricsKey::local(TrackId(8)))
            .unwrap(),
        None
    );
}
