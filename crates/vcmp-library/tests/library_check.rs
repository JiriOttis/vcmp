//! The library check: duplicate groups and "Not duplicates", missing tags (a title
//! the file holds against the file-name fallback), albums without a cover, and that
//! none of it writes a music file or any table but the "Not duplicates" marks. Each
//! test copies the fixtures into a temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::tag::{Accessor, ItemKey, Tag, TagExt};
use tempfile::TempDir;
use vcmp_library::{Library, MissingFields, TrackId};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[derive(Default)]
struct Tags<'a> {
    title: Option<&'a str>,
    artist: Option<&'a str>,
    album: Option<&'a str>,
    number: Option<u32>,
    year: Option<u32>,
}

/// Every field set.
fn full<'a>(title: &'a str, artist: &'a str, album: &'a str) -> Tags<'a> {
    Tags {
        title: Some(title),
        artist: Some(artist),
        album: Some(album),
        number: Some(1),
        year: Some(2001),
    }
}

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

    /// Adds a one-second FLAC file at `name` under the music folder with `tags`.
    fn add(&self, name: &str, tags: Tags) -> PathBuf {
        let path = self.music().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::copy(Path::new(FIXTURES).join("silence.flac"), &path).unwrap();
        let file = lofty::read_from_path(&path).unwrap();
        let mut tag = Tag::new(file.primary_tag_type());
        if let Some(title) = tags.title {
            tag.set_title(title.into());
        }
        if let Some(artist) = tags.artist {
            tag.set_artist(artist.into());
        }
        if let Some(album) = tags.album {
            tag.set_album(album.into());
        }
        if let Some(number) = tags.number {
            tag.set_track(number);
        }
        if let Some(year) = tags.year {
            tag.insert_text(ItemKey::Year, year.to_string());
        }
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

    /// The groups the page shows (hidden ones left out), as sorted track ids.
    fn shown_groups(&self) -> Vec<Vec<TrackId>> {
        self.library
            .duplicate_groups()
            .unwrap()
            .into_iter()
            .filter(|group| !group.hidden)
            .map(|group| {
                let mut ids: Vec<TrackId> = group.tracks.iter().map(|track| track.id).collect();
                ids.sort();
                ids
            })
            .collect()
    }

    fn missing(&self, path: &Path) -> Option<MissingFields> {
        let id = self.id(path);
        self.library
            .missing_tags()
            .unwrap()
            .into_iter()
            .flat_map(|album| album.tracks)
            .find(|gaps| gaps.track.id == id)
            .map(|gaps| gaps.missing)
    }
}

#[test]
fn same_title_and_artist_ignoring_case_and_accents_are_duplicates() {
    let mut setup = Setup::new();
    let a = setup.add("a.flac", full("Hoppípolla", "Sigur Rós", "Takk"));
    let b = setup.add("b.flac", full("HOPPIPOLLA", "sigur ros", "Live"));
    setup.add("c.flac", full("Glósóli", "Sigur Rós", "Takk"));
    // The same title by someone else is not a duplicate.
    setup.add("d.flac", full("Hoppípolla", "Cover Band", "Covers"));
    setup.scan();

    let mut expected = vec![setup.id(&a), setup.id(&b)];
    expected.sort();
    assert_eq!(setup.shown_groups(), [expected]);
}

#[test]
fn tracks_without_an_artist_or_a_real_title_are_not_duplicates() {
    let mut setup = Setup::new();
    let no_artist = |title| Tags {
        title: Some(title),
        ..Tags::default()
    };
    setup.add("one/x.flac", no_artist("Intro"));
    setup.add("two/x.flac", no_artist("Intro"));
    // Same file name, same artist, no title: both titled "01 Intro" by the fallback.
    let no_title = Tags {
        artist: Some("Band"),
        ..Tags::default()
    };
    setup.add("one/01 Intro.flac", no_title);
    setup.add(
        "two/01 Intro.flac",
        Tags {
            artist: Some("Band"),
            ..Tags::default()
        },
    );
    setup.scan();
    assert!(setup.library.duplicate_groups().unwrap().is_empty());
}

#[test]
fn not_duplicates_hides_a_group_until_a_track_joins_it() {
    let mut setup = Setup::new();
    let a = setup.add("a.flac", full("Song", "Band", "First"));
    let b = setup.add("b.flac", full("Song", "Band", "Second"));
    setup.scan();
    let (a, b) = (setup.id(&a), setup.id(&b));
    assert_eq!(setup.shown_groups().len(), 1);

    setup.library.mark_not_duplicates(&[a, b]).unwrap();
    assert!(setup.shown_groups().is_empty());
    let groups = setup.library.duplicate_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert!(groups[0].hidden);

    // A third copy joins: the group shows again, with all three.
    let c = setup.add("c.flac", full("Song", "Band", "Third"));
    setup.scan();
    let c = setup.id(&c);
    let mut all = vec![a, b, c];
    all.sort();
    assert_eq!(setup.shown_groups(), [all.clone()]);

    // Marked again, then a copy goes missing: still hidden.
    setup.library.mark_not_duplicates(&all).unwrap();
    fs::remove_file(setup.music().join("c.flac")).unwrap();
    setup.scan();
    assert!(setup.shown_groups().is_empty());

    // Undone: shown again, and no older mark hides it either.
    setup.library.unmark_not_duplicates(&[a, b]).unwrap();
    assert_eq!(setup.shown_groups().len(), 1);
    assert!(!setup.library.duplicate_groups().unwrap()[0].hidden);
}

#[test]
fn a_file_name_title_is_missing_and_a_real_one_is_not() {
    let mut setup = Setup::new();
    let untitled = setup.add(
        "Album/03 Something.flac",
        Tags {
            title: None,
            ..full("", "Band", "Album")
        },
    );
    // A real title that happens to match the file name is not missing.
    let titled = setup.add(
        "Album/Named.flac",
        Tags {
            year: None,
            ..full("Named", "Band", "Album")
        },
    );
    let complete = setup.add("Album/Done.flac", full("Done", "Band", "Album"));
    setup.scan();

    let track = setup.library.track(setup.id(&untitled)).unwrap().unwrap();
    assert_eq!(track.title, "03 Something");
    assert_eq!(
        setup.missing(&untitled),
        Some(MissingFields {
            title: true,
            ..MissingFields::default()
        })
    );
    assert_eq!(
        setup.missing(&titled),
        Some(MissingFields {
            year: true,
            ..MissingFields::default()
        })
    );
    assert_eq!(setup.missing(&complete), None);

    // Grouped under their album, in album order.
    let albums = setup.library.missing_tags().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].album.title, "Album");
    assert_eq!(albums[0].tracks.len(), 2);
}

#[test]
fn every_missing_field_is_reported() {
    let mut setup = Setup::new();
    let bare = setup.add("Nothing.flac", Tags::default());
    setup.scan();
    assert_eq!(
        setup.missing(&bare),
        Some(MissingFields {
            title: true,
            artist: true,
            album: true,
            track_number: true,
            year: true,
        })
    );
}

#[test]
fn albums_without_a_cover_leave_out_unknown_albums() {
    let mut setup = Setup::new();
    setup.add("Bare/a.flac", full("A", "Band", "Bare"));
    let covered = setup.add("Covered/b.flac", full("B", "Band", "Covered"));
    image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 40]))
        .save(covered.with_file_name("cover.png"))
        .unwrap();
    setup.add(
        "Loose/c.flac",
        Tags {
            album: None,
            ..full("C", "Band", "")
        },
    );
    setup.scan();

    let titles: Vec<String> = setup
        .library
        .albums_without_cover()
        .unwrap()
        .into_iter()
        .map(|album| album.title)
        .collect();
    assert_eq!(titles, ["Bare"]);
}

/// Every music file's bytes and modification time.
fn files(dir: &Path) -> Vec<(PathBuf, Vec<u8>, SystemTime)> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let path = entry.path().to_path_buf();
            let modified = entry.metadata().unwrap().modified().unwrap();
            found.push((path.clone(), fs::read(&path).unwrap(), modified));
        }
    }
    found
}

/// Every row of every table but the "Not duplicates" marks, as text.
fn tables(db: &Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(db).unwrap();
    let names: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name != 'not_duplicates'
             ORDER BY name",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let mut rows = Vec::new();
    for name in names {
        let mut statement = conn.prepare(&format!("SELECT * FROM \"{name}\"")).unwrap();
        let columns = statement.column_count();
        let mut query = statement.query([]).unwrap();
        while let Some(row) = query.next().unwrap() {
            let values: Vec<String> = (0..columns)
                .map(|i| format!("{:?}", row.get_ref(i).unwrap()))
                .collect();
            rows.push(format!("{name}: {}", values.join(", ")));
        }
    }
    rows
}

#[test]
fn the_check_writes_no_music_file_and_no_other_table() {
    let mut setup = Setup::new();
    let a = setup.add("a.flac", full("Song", "Band", "First"));
    let b = setup.add("b.flac", full("Song", "Band", "Second"));
    setup.add("c.flac", Tags::default());
    setup.scan();
    let (a, b) = (setup.id(&a), setup.id(&b));
    let db = setup.base.join("data/library.sqlite3");
    let files_before = files(&setup.music());
    let tables_before = tables(&db);

    let library = &setup.library;
    assert_eq!(library.duplicate_groups().unwrap().len(), 1);
    assert!(!library.missing_tags().unwrap().is_empty());
    library.albums_without_cover().unwrap();
    library.mark_not_duplicates(&[a, b]).unwrap();
    library.duplicate_groups().unwrap();
    library.unmark_not_duplicates(&[a, b]).unwrap();

    assert_eq!(files(&setup.music()), files_before);
    assert_eq!(tables(&db), tables_before);
}
