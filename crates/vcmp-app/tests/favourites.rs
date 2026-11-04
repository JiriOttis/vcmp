//! Favourites in the window: a row's heart, the player bar's, the favourites entry of
//! the track and album menus, every heart showing a change at once, and the
//! Favourites playlist pinned first, read-only, played and exported.

mod common;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::{Song, add};
use i_slint_backend_testing::{AccessibleRole, ElementHandle, mock_elapsed_time};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, LogicalSize, Model, ModelRc};
use vcmp_app::library_view::LibraryView;
use vcmp_app::player_service::{PlayerControl, PlayerService};
use vcmp_app::{
    AppWindow, Favourites, Page, PlayerState, QueueSource, SelectionChange, SongSelection,
    TrackRow, TrackSelection,
};
use vcmp_library::{Library, TrackId};
use vcmp_playback::{Command, Event, NowPlaying, QueueEntry, QueueView, TrackRef};

#[derive(Clone, Default)]
struct Recorder(Rc<RefCell<Vec<Command>>>);

impl PlayerControl for Recorder {
    fn send(&self, command: Command) {
        self.0.borrow_mut().push(command);
    }
}

struct App {
    _dir: tempfile::TempDir,
    music: PathBuf,
    library: Rc<Library>,
    window: AppWindow,
    view: Rc<LibraryView>,
    service: Rc<PlayerService>,
    sent: Recorder,
}

/// Two songs of one album, and a single on another.
fn start() -> App {
    i_slint_backend_testing::init_no_event_loop();
    let dir = tempfile::tempdir().unwrap();
    let music = dunce::canonicalize(dir.path()).unwrap().join("music");
    add(&music, "1.flac", Song::new("One", "Band", "Album"));
    add(
        &music,
        "2.flac",
        Song {
            track: 2,
            ..Song::new("Two", "Band", "Album")
        },
    );
    add(&music, "single.flac", Song::new("Single", "Solo", "Lone"));
    let mut library = Library::open(
        &dir.path().join("library.sqlite3"),
        &dir.path().join("cache"),
    )
    .unwrap();
    library.add_source(&music, true).unwrap();
    library.scan(&AtomicBool::new(false), |_| {}).unwrap();
    let library = Rc::new(library);
    let window = vcmp_app::create_window().unwrap();
    let view = LibraryView::connect(&window, library.clone()).unwrap();
    let sent = Recorder::default();
    let service = PlayerService::connect(
        &window,
        library.clone(),
        view.clone(),
        Box::new(sent.clone()),
        None,
    );
    window.show().unwrap();
    window.window().set_size(LogicalSize::new(1100.0, 1200.0));
    mock_elapsed_time(Duration::ZERO);
    App {
        _dir: dir,
        music,
        library,
        window,
        view,
        service,
        sent,
    }
}

impl App {
    fn favourites(&self) -> Favourites<'_> {
        self.window.global::<Favourites>()
    }

    fn id(&self, title: &str) -> TrackId {
        self.library
            .tracks()
            .unwrap()
            .into_iter()
            .find(|track| track.title == title)
            .unwrap_or_else(|| panic!("no track {title:?}"))
            .id
    }

    /// The liked tracks' titles, newest first.
    fn liked(&self) -> Vec<String> {
        self.library
            .favourites()
            .unwrap()
            .into_iter()
            .map(|favourite| favourite.track.title)
            .collect()
    }

    fn open_album(&self, title: &str) -> ModelRc<TrackRow> {
        let index = self
            .window
            .get_albums()
            .iter()
            .position(|tile| tile.title == title)
            .unwrap();
        self.window.invoke_album_activated(index as i32);
        mock_elapsed_time(Duration::ZERO);
        self.window.get_album_tracks()
    }

    fn find(&self, role: AccessibleRole, label: &str) -> ElementHandle {
        ElementHandle::find_by_accessible_label(&self.window, label)
            .find(|element| element.accessible_role() == Some(role))
            .unwrap_or_else(|| panic!("no {role:?} labelled {label:?}"))
    }

    fn open_section(&self, label: &str) {
        self.find(AccessibleRole::Tab, label)
            .invoke_accessible_default_action();
        mock_elapsed_time(Duration::ZERO);
    }

    fn click(&self, position: LogicalPosition) {
        let window = self.window.window();
        let button = PointerEventButton::Left;
        window.dispatch_event(WindowEvent::PointerMoved { position });
        window.dispatch_event(WindowEvent::PointerPressed { position, button });
        window.dispatch_event(WindowEvent::PointerReleased { position, button });
    }

    fn entry(&self, file: &str) -> QueueEntry {
        QueueEntry::new(TrackRef::local(
            self.music.join(file).to_string_lossy().as_ref(),
        ))
    }
}

fn liked_rows(rows: &ModelRc<TrackRow>) -> Vec<(String, bool)> {
    rows.iter()
        .map(|row| (row.title.to_string(), row.liked))
        .collect()
}

/// Where a row's heart is: between the title and the time, at its right end.
fn heart_of(row: &ElementHandle) -> LogicalPosition {
    let (position, size) = (row.absolute_position(), row.size());
    // The row's right padding, the time's column, the space before it, half the heart.
    LogicalPosition::new(
        position.x + size.width - 8.0 - 64.0 - 16.0 - 12.0,
        position.y + size.height / 2.0,
    )
}

#[test]
fn a_rows_heart_likes_its_track_and_every_heart_shows_it() {
    let app = start();
    app.open_section("Albums");
    let rows = app.open_album("Album");
    assert_eq!(
        liked_rows(&rows),
        [("One".to_string(), false), ("Two".to_string(), false)]
    );
    assert!(rows.iter().all(|row| row.track_id > 0), "rows carry ids");
    app.window.invoke_songs_needed();

    // A click on the heart likes the track; it plays nothing and selects nothing.
    let one = app.find(AccessibleRole::ListItem, "One");
    app.click(heart_of(&one));
    assert_eq!(app.liked(), ["One"]);
    assert!(
        !app.sent
            .0
            .borrow()
            .iter()
            .any(|command| matches!(command, Command::PlayContext { .. })),
        "the heart plays nothing"
    );
    let rows = app.window.get_album_tracks();
    assert_eq!(
        liked_rows(&rows),
        [("One".to_string(), true), ("Two".to_string(), false)]
    );
    assert!(!rows.row_data(0).unwrap().selected);
    // The row says so, and the songs table knows too.
    app.find(AccessibleRole::ListItem, "One, in Favourites");
    let songs = app.window.get_songs();
    let one = songs.iter().find(|row| row.title == "One").unwrap();
    assert!(one.liked);

    // Again takes it out.
    let one = app.find(AccessibleRole::ListItem, "One, in Favourites");
    app.click(heart_of(&one));
    assert!(app.liked().is_empty());
    assert!(!app.window.get_album_tracks().row_data(0).unwrap().liked);
}

#[test]
fn a_menu_likes_the_chosen_tracks_or_takes_them_out_when_all_are_liked() {
    let app = start();
    let rows = app.open_album("Album");
    let selection = app.window.global::<TrackSelection>();
    selection.invoke_prepare_favourites(rows.clone(), 0);
    assert!(!app.favourites().get_menu_removes());
    selection.invoke_toggle_favourites(rows.clone(), 0);
    assert_eq!(app.liked(), ["One"]);

    // One liked and one not: the menu adds the other, and keeps One's time.
    let rows = app.window.get_album_tracks();
    selection.invoke_change(rows.clone(), 1, 0, SelectionChange::Range);
    selection.invoke_prepare_favourites(rows.clone(), 1);
    assert!(!app.favourites().get_menu_removes());
    std::thread::sleep(Duration::from_millis(5));
    selection.invoke_toggle_favourites(rows.clone(), 1);
    assert_eq!(app.liked(), ["Two", "One"]);
    // All liked: the entry takes them all out.
    selection.invoke_prepare_favourites(rows.clone(), 1);
    assert!(app.favourites().get_menu_removes());
    selection.invoke_toggle_favourites(rows, 1);
    assert!(app.liked().is_empty());

    // The songs table's menu, by its rows.
    app.window.invoke_songs_needed();
    let songs = app.window.get_songs();
    let single = songs.iter().position(|row| row.title == "Single").unwrap() as i32;
    let songs_menu = app.window.global::<SongSelection>();
    songs_menu.invoke_prepare_favourites(songs.clone(), single);
    assert!(!app.favourites().get_menu_removes());
    songs_menu.invoke_toggle_favourites(songs, single);
    assert_eq!(app.liked(), ["Single"]);
    let songs = app.window.get_songs();
    assert!(songs.row_data(single as usize).unwrap().liked);
}

#[test]
fn an_albums_menu_likes_its_tracks_or_takes_them_out() {
    let app = start();
    let album = app
        .library
        .track(app.id("One"))
        .unwrap()
        .unwrap()
        .album_id
        .unwrap()
        .0 as i32;
    app.library.like(&[app.id("Two")]).unwrap();
    app.view.hearts().reload();
    let favourites = app.favourites();
    favourites.invoke_prepare_album(album);
    assert!(!favourites.get_menu_removes(), "some are liked, not all");
    favourites.invoke_toggle_album(album);
    let mut liked = app.liked();
    liked.sort();
    assert_eq!(liked, ["One", "Two"]);
    favourites.invoke_prepare_album(album);
    assert!(favourites.get_menu_removes());
    favourites.invoke_toggle_album(album);
    assert!(app.liked().is_empty());
}

#[test]
fn the_favourites_playlist_comes_first_and_only_plays_and_exports() {
    let app = start();
    app.open_section("Playlists");
    let card = app.window.get_playlists().row_data(0).unwrap();
    assert!(card.favourites);
    assert_eq!((card.title.as_str(), card.track_count), ("Favourites", 0));

    // The sidebar opens it, empty, saying how to fill it.
    app.open_section("Albums");
    app.find(AccessibleRole::Button, "Favourites")
        .invoke_accessible_default_action();
    mock_elapsed_time(Duration::ZERO);
    assert_eq!(app.window.get_current_page(), Page::Playlists);
    assert!(app.window.get_playlist_open());
    assert_eq!(app.window.get_open_playlist_index(), 0);
    let detail = app.window.get_playlist_detail();
    assert!(detail.favourites);
    ElementHandle::find_by_accessible_label(
        &app.window,
        "Nothing is in your favourites yet. To add a track, click the heart beside its time, or open its menu with a right click or the Menu key and choose Add to Favourites.",
    )
    .next()
    .expect("the empty Favourites says how to fill it");

    // Liked from elsewhere, it shows them at once, newest first.
    app.favourites()
        .invoke_toggle_track(app.id("Single").0 as i32);
    std::thread::sleep(Duration::from_millis(5));
    app.favourites().invoke_toggle_track(app.id("Two").0 as i32);
    assert_eq!(
        liked_rows(&app.window.get_playlist_tracks()),
        [("Two".to_string(), true), ("Single".to_string(), true)]
    );
    assert_eq!(
        app.window.get_playlists().row_data(0).unwrap().track_count,
        2
    );
    app.find(AccessibleRole::Button, "Play Favourites");
    app.find(AccessibleRole::Button, "More for Favourites");

    // Renaming and deleting do nothing to it, and it has no entries to move.
    app.window.invoke_rename_playlist("Mine".into());
    app.window.invoke_delete_playlist();
    app.window.invoke_move_playlist_track(0, 1);
    assert!(app.window.get_playlist_open());
    assert_eq!(app.liked(), ["Two", "Single"]);
    assert!(app.window.get_playlist_detail().favourites);

    // It plays as a context of its own, which the queue names.
    app.window.invoke_play_playlist(-1, false);
    let origin = app
        .sent
        .0
        .borrow()
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::PlayContext {
                entries, origin, ..
            } => Some((entries.len(), origin.clone())),
            _ => None,
        })
        .expect("something played");
    assert_eq!(origin, (2, Some("favourites".to_string())));
    app.service.handle_event(Event::QueueChanged(QueueView {
        origin: origin.1,
        ..QueueView::default()
    }));
    assert_eq!(
        app.window.global::<PlayerState>().get_queue_source(),
        QueueSource::Favourites
    );

    // Its heart takes a track out, and the page follows.
    app.favourites().invoke_toggle_track(app.id("Two").0 as i32);
    assert_eq!(
        liked_rows(&app.window.get_playlist_tracks()),
        [("Single".to_string(), true)]
    );

    // It exports, newest first.
    app.favourites().invoke_toggle_track(app.id("One").0 as i32);
    let file = app.music.join("Favourites.m3u8");
    app.view.playlists().export_open(&file);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "#EXTM3U\n#EXTINF:1,Band - One\n1.flac\n#EXTINF:1,Solo - Single\nsingle.flac\n"
    );
    assert!(!app.window.get_playlist_detail().export_failed);
}

#[test]
fn the_player_bar_and_the_queue_show_the_playing_tracks_heart() {
    let app = start();
    app.service.handle_event(Event::QueueChanged(QueueView {
        upcoming: vec![app.entry("2.flac")],
        ..QueueView::default()
    }));
    app.service
        .handle_event(Event::TrackChanged(Some(NowPlaying {
            entry: app.entry("1.flac"),
            duration: None,
        })));
    mock_elapsed_time(Duration::ZERO);
    let state = app.window.global::<PlayerState>();
    assert_eq!(state.get_track_id(), app.id("One").0 as i32);
    assert!(!state.get_liked());

    app.find(AccessibleRole::Button, "In Favourites")
        .invoke_accessible_default_action();
    assert!(state.get_liked());
    assert_eq!(app.liked(), ["One"]);

    // The queue's rows carry their hearts too, for its menu.
    let upcoming = state.get_upcoming().row_data(0).unwrap();
    assert_eq!(upcoming.track_id, app.id("Two").0 as i32);
    assert!(!upcoming.liked);
    app.favourites().invoke_toggle_track(upcoming.track_id);
    assert!(state.get_upcoming().row_data(0).unwrap().liked);

    // Taken out elsewhere (the album page's heart), the bar follows.
    app.favourites().invoke_toggle_track(app.id("One").0 as i32);
    assert!(!state.get_liked());
}
