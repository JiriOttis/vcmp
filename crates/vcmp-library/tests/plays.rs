//! Play history: the log, counts and last played over all time or a range, plays
//! that follow a moved file, clearing, and the one-time backfill of added dates from
//! the files. Each test copies the fixtures into a temporary directory; nothing here
//! writes a music file's tags.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::tag::{Accessor, Tag, TagExt};
use tempfile::TempDir;
use vcmp_library::{Library, PlayStats, SETTING_ADDED_BACKFILLED, Scrobble, TrackId};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

struct Setup {
    // The database closes before the directory is deleted, which Windows requires.
    library: Library,
    base: PathBuf,
    _root: TempDir,
}

impl Setup {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(root.path()).unwrap();
        fs::create_dir(base.join("music")).unwrap();
        let library =
            Library::open(&base.join("data/library.sqlite3"), &base.join("cache")).unwrap();
        library.add_source(&base.join("music"), true).unwrap();
        Self {
            _root: root,
            base,
            library,
        }
    }

    fn music(&self) -> PathBuf {
        self.base.join("music")
    }

    fn database(&self) -> PathBuf {
        self.base.join("data/library.sqlite3")
    }

    /// Adds a one-second FLAC file at `name` under the music folder.
    fn add(&self, name: &str, title: &str, album: &str, number: u32) -> PathBuf {
        let path = self.music().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(Path::new(FIXTURES).join("silence.flac"), &path).unwrap();
        let file = lofty::read_from_path(&path).unwrap();
        let mut tag = Tag::new(file.primary_tag_type());
        tag.set_title(title.into());
        tag.set_artist("Band".into());
        tag.set_album(album.into());
        tag.set_track(number);
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        path
    }

    fn scan(&mut self) {
        let report = self.library.scan(&AtomicBool::new(false), |_| {}).unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    }

    fn id(&self, path: &Path) -> TrackId {
        self.library.track_id_for_path(path).unwrap().unwrap()
    }

    /// Three tracks: two on "First", one on "Second".
    fn three(&mut self) -> [TrackId; 3] {
        let a = self.add("a.flac", "One", "First", 1);
        let b = self.add("b.flac", "Two", "First", 2);
        let c = self.add("c.flac", "Three", "Second", 1);
        self.scan();
        [self.id(&a), self.id(&b), self.id(&c)]
    }

    fn added(&self, track: TrackId) -> SystemTime {
        self.library.track(track).unwrap().unwrap().added
    }
}

fn at(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn set_modified(path: &Path, time: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

#[test]
fn plays_give_counts_and_the_last_play() {
    let mut setup = Setup::new();
    let [a, b, c] = setup.three();
    setup
        .library
        .record_play(a, at(1_000), Some("album:1"))
        .unwrap();
    setup.library.record_play(a, at(3_000), None).unwrap();
    setup
        .library
        .record_play(a, at(2_000), Some("search"))
        .unwrap();
    setup
        .library
        .record_play(b, at(500), Some("playlist:4"))
        .unwrap();

    let stats = setup.library.play_stats(None).unwrap();
    assert_eq!(stats.len(), 2, "c was never played");
    assert_eq!(
        stats[&a],
        PlayStats {
            count: 3,
            last: at(3_000)
        }
    );
    assert_eq!(
        stats[&b],
        PlayStats {
            count: 1,
            last: at(500)
        }
    );
    assert_eq!(setup.library.track_play_stats(a).unwrap(), Some(stats[&a]));
    assert_eq!(setup.library.track_play_stats(c).unwrap(), None);
}

#[test]
fn every_play_keeps_its_origin_as_given() {
    let mut setup = Setup::new();
    let [a, _, _] = setup.three();
    setup
        .library
        .record_play(a, at(1_000), Some("folder:/m/x"))
        .unwrap();
    setup.library.record_play(a, at(2_000), None).unwrap();
    let conn = rusqlite::Connection::open(setup.database()).unwrap();
    let origins: Vec<Option<String>> = conn
        .prepare("SELECT origin FROM plays ORDER BY played_ms")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(origins, [Some("folder:/m/x".to_string()), None]);
}

#[test]
fn counts_in_a_time_range_leave_out_older_plays() {
    let mut setup = Setup::new();
    let [a, b, _] = setup.three();
    for time in [100, 200, 300, 400] {
        setup.library.record_play(a, at(time), None).unwrap();
    }
    setup.library.record_play(b, at(150), None).unwrap();

    let since = setup.library.play_stats(Some(at(300))).unwrap();
    assert_eq!(
        since,
        [(
            a,
            PlayStats {
                count: 2,
                last: at(400)
            }
        )]
        .into(),
        "the range starts at its first moment"
    );
    assert_eq!(setup.library.play_stats(Some(at(500))).unwrap().len(), 0);
    assert_eq!(setup.library.play_stats(None).unwrap()[&a].count, 4);
}

#[test]
fn history_follows_a_moved_file_and_stays_with_a_missing_one() {
    let mut setup = Setup::new();
    let [a, b, _] = setup.three();
    setup.library.record_play(a, at(1_000), None).unwrap();
    setup.library.record_play(b, at(2_000), None).unwrap();

    let moved = setup.music().join("elsewhere/a.flac");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(setup.music().join("a.flac"), &moved).unwrap();
    fs::rename(setup.music().join("b.flac"), setup.base.join("away.flac")).unwrap();
    setup.scan();

    assert_eq!(setup.id(&moved), a);
    let stats = setup.library.play_stats(None).unwrap();
    assert_eq!(stats[&a].count, 1);
    assert_eq!(stats[&b].count, 1, "kept while the file is gone");
}

#[test]
fn clearing_deletes_only_the_play_log() {
    let mut setup = Setup::new();
    let [a, b, _] = setup.three();
    setup.library.record_play(a, at(1_000), None).unwrap();
    setup.library.record_play(b, at(2_000), None).unwrap();
    setup.library.like(&[a]).unwrap();
    setup
        .library
        .queue_scrobble(&Scrobble {
            artist: "Band".into(),
            title: "One".into(),
            album: Some("First".into()),
            album_artist: None,
            track_number: Some(1),
            duration: Some(Duration::from_secs(1)),
            played_at: at(1_000),
        })
        .unwrap();
    let added = setup.added(a);

    assert_eq!(setup.library.clear_play_history().unwrap(), 2);
    assert!(setup.library.play_stats(None).unwrap().is_empty());
    assert_eq!(setup.library.queued_scrobble_count().unwrap(), 1);
    assert_eq!(setup.added(a), added);
    assert_eq!(setup.library.liked_tracks().unwrap(), [a].into());
    assert_eq!(setup.library.clear_play_history().unwrap(), 0);
}

#[test]
fn a_new_track_is_added_when_first_seen() {
    let mut setup = Setup::new();
    let before = SystemTime::now() - Duration::from_secs(1);
    let path = setup.add("a.flac", "One", "First", 1);
    set_modified(&path, at(978_307_200));
    setup.scan();
    let added = setup.added(setup.id(&path));
    assert!(added >= before, "first seen, not the file's date");
    assert!(added <= SystemTime::now());
    assert_eq!(
        setup.library.setting(SETTING_ADDED_BACKFILLED).unwrap(),
        Some("true".into()),
        "a new library has nothing to backfill"
    );
}

#[test]
fn the_first_scan_after_the_update_takes_added_dates_from_the_files_once() {
    let mut setup = Setup::new();
    let [a, b, c] = setup.three();
    let year_2001 = at(978_307_200);

    // As an older version left the library: every track at its first scan's time, b
    // somehow earlier, and the backfill not done yet.
    let conn = rusqlite::Connection::open(setup.database()).unwrap();
    conn.execute(
        "DELETE FROM settings WHERE key = ?1",
        [SETTING_ADDED_BACKFILLED],
    )
    .unwrap();
    conn.execute("UPDATE tracks SET added_ms = 1000 WHERE id = ?1", [b.0])
        .unwrap();
    drop(conn);
    let c_added = setup.added(c);

    set_modified(&setup.music().join("a.flac"), year_2001);
    set_modified(&setup.music().join("b.flac"), year_2001);
    // c's file is away during this scan, and d is new to it.
    fs::rename(setup.music().join("c.flac"), setup.base.join("away.flac")).unwrap();
    let d = setup.add("d.flac", "Four", "Third", 1);
    set_modified(&d, year_2001);
    let before = SystemTime::now() - Duration::from_secs(1);
    setup.scan();

    assert_eq!(setup.added(a), year_2001, "the file's date");
    assert_eq!(
        setup.added(b),
        UNIX_EPOCH + Duration::from_millis(1000),
        "never later"
    );
    assert_eq!(setup.added(c), c_added, "a missing file keeps its date");
    assert!(
        setup.added(setup.id(&d)) >= before,
        "first seen in this scan"
    );
    assert_eq!(
        setup.library.setting(SETTING_ADDED_BACKFILLED).unwrap(),
        Some("true".into())
    );

    // Only once: an earlier date later on, or the missing file coming back, changes
    // nothing.
    fs::rename(setup.base.join("away.flac"), setup.music().join("c.flac")).unwrap();
    set_modified(&setup.music().join("a.flac"), at(631_152_000));
    setup.scan();
    assert_eq!(setup.added(a), year_2001);
    assert_eq!(setup.added(c), c_added);
}
