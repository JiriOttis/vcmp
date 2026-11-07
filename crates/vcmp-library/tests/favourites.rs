//! Favourites: liking and unliking, the newest-first list, hearts that follow a
//! moved file or wait for a missing one, and the M3U export. Each test copies the
//! fixtures into a temporary directory; nothing here writes a music file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::tag::{Accessor, Tag, TagExt};
use tempfile::TempDir;
use vcmp_library::{FavouritesSummary, Library, TrackId};

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

    fn titles(&self) -> Vec<String> {
        self.library
            .favourites()
            .unwrap()
            .into_iter()
            .map(|favourite| favourite.track.title)
            .collect()
    }
}

#[test]
fn tracks_are_liked_and_unliked() {
    let mut setup = Setup::new();
    let [a, b, c] = setup.three();
    let library = &setup.library;
    assert!(library.liked_tracks().unwrap().is_empty());
    assert_eq!(
        library.favourites_summary().unwrap(),
        FavouritesSummary::default()
    );

    library.like(&[a, c]).unwrap();
    // Liking again changes nothing.
    library.like(&[a]).unwrap();
    assert_eq!(library.liked_tracks().unwrap(), [a, c].into());
    assert_eq!(
        library.favourites_summary().unwrap(),
        FavouritesSummary {
            track_count: 2,
            available_count: 2,
            duration: Duration::from_secs(2),
        }
    );

    library.unlike(&[a, b]).unwrap();
    assert_eq!(library.liked_tracks().unwrap(), [c].into());
}

#[test]
fn the_newest_come_first_and_an_album_keeps_its_order() {
    let mut setup = Setup::new();
    let [a, b, c] = setup.three();
    // Liked together, newest last in the call: the album's order holds.
    setup.library.like(&[b, a]).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    setup.library.like(&[c]).unwrap();
    assert_eq!(setup.titles(), ["Three", "One", "Two"]);

    // Liking a liked track again doesn't move it to the top.
    std::thread::sleep(Duration::from_millis(5));
    setup.library.like(&[a]).unwrap();
    assert_eq!(setup.titles(), ["Three", "One", "Two"]);
    let favourites = setup.library.favourites().unwrap();
    assert!(favourites[0].liked > favourites[1].liked);
}

#[test]
fn a_heart_follows_a_moved_file_and_waits_for_a_missing_one() {
    let mut setup = Setup::new();
    let [a, b, _] = setup.three();
    setup.library.like(&[a, b]).unwrap();

    let moved = setup.music().join("elsewhere/a.flac");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(setup.music().join("a.flac"), &moved).unwrap();
    fs::rename(setup.music().join("b.flac"), setup.base.join("away.flac")).unwrap();
    setup.scan();

    assert_eq!(setup.id(&moved), a);
    assert_eq!(setup.library.liked_tracks().unwrap(), [a, b].into());
    let favourites = setup.library.favourites().unwrap();
    assert_eq!(favourites.len(), 2, "the missing one is listed too");
    let missing = favourites.iter().find(|f| f.track.id == b).unwrap();
    assert!(!missing.track.available);
    assert_eq!(
        setup.library.favourites_summary().unwrap(),
        FavouritesSummary {
            track_count: 2,
            available_count: 1,
            duration: Duration::from_secs(1),
        }
    );

    fs::rename(setup.base.join("away.flac"), setup.music().join("b.flac")).unwrap();
    setup.scan();
    assert_eq!(
        setup.library.favourites_summary().unwrap().available_count,
        2
    );
}

#[test]
fn favourites_export_newest_first() {
    let mut setup = Setup::new();
    let [a, b, c] = setup.three();
    setup.library.like(&[a, b]).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    setup.library.like(&[c]).unwrap();

    let file = setup.music().join("Favourites.m3u8");
    let report = setup.library.export_favourites(&file).unwrap();
    assert_eq!(report.written, 3);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "#EXTM3U\n\
         #EXTINF:1,Band - Three\nc.flac\n\
         #EXTINF:1,Band - One\na.flac\n\
         #EXTINF:1,Band - Two\nb.flac\n"
    );
}

#[test]
fn likes_at_a_given_time_keep_an_earlier_heart() {
    let mut setup = Setup::new();
    let [a, b, c] = setup.three();
    setup.library.like(&[c]).unwrap();
    let before = setup.library.favourites().unwrap()[0].liked;
    let old = std::time::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    let older = std::time::UNIX_EPOCH + Duration::from_secs(1_500_000_000);
    let liked = setup
        .library
        .like_at(&[(a, old), (b, older), (c, older), (TrackId(9999), old)])
        .unwrap();
    assert_eq!(liked, 2, "c was liked already and 9999 is no track");
    assert_eq!(setup.titles(), ["Three", "One", "Two"]);
    let favourites = setup.library.favourites().unwrap();
    assert_eq!(favourites[0].liked, before);
    assert_eq!(favourites[1].liked, old);
    assert_eq!(favourites[2].liked, older);
}

#[test]
fn track_names_include_missing_files() {
    let mut setup = Setup::new();
    let [a, _, c] = setup.three();
    fs::remove_file(setup.music().join("c.flac")).unwrap();
    setup.scan();
    let mut names = setup.library.track_names().unwrap();
    names.sort();
    assert_eq!(names.len(), 3);
    assert_eq!(names[0], (a, "Band".to_string(), "One".to_string()));
    assert!(names.contains(&(c, "Band".to_string(), "Three".to_string())));
}
