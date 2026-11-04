//! Favourites in the window, behind the `Favourites` global: the tracks the owner
//! liked (`vcmp_library` favourites), kept in the database only.
//!
//! Every heart and menu entry goes through [`Hearts::set`], the one place that likes
//! and unlikes. It writes the database, keeps the liked set for this thread, then
//! shows the change everywhere: the liked flag of every row on screen (track lists,
//! the songs table, the queue), the player bar's heart, and the Favourites playlist.
//!
//! - **Rows** carry their track's id and whether it is liked. Every list builds its
//!   rows from tracks, in several modules, so [`liked`] reads the set this module
//!   keeps for the window's thread.
//! - **Menus** act on several tracks (a selection, an album): they show "Remove from
//!   Favourites" when every one is liked, and "Add to Favourites" otherwise, which
//!   likes the rest. A menu asks just before it opens (`prepare`); the tracks are
//!   worked out again when the entry is chosen.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use slint::{ComponentHandle, Model, ModelRc};
use vcmp_library::{AlbumId, Library, TrackId};

use crate::playlists::PlaylistBrowser;
use crate::selection::{self, Selectable};
use crate::{
    AppWindow, Favourites, PlayerState, QueueRow, SongRow, SongSelection, TrackRow, TrackSelection,
};

thread_local! {
    /// The liked tracks, for the window on this thread.
    static LIKED: RefCell<HashSet<TrackId>> = RefCell::default();
}

/// Whether the track is among the favourites.
pub fn liked(track: TrackId) -> bool {
    LIKED.with(|liked| liked.borrow().contains(&track))
}

/// A track's id as rows hold it.
pub fn row_id(track: TrackId) -> i32 {
    track.0 as i32
}

pub struct Hearts {
    window: slint::Weak<AppWindow>,
    library: Rc<Library>,
    playlists: RefCell<Weak<PlaylistBrowser>>,
}

impl Hearts {
    /// Reads the favourites and answers the window's hearts and menus. Connect it
    /// before anything builds rows, and keep it alive as long as the window.
    pub fn connect(window: &AppWindow, library: Rc<Library>) -> Rc<Self> {
        let hearts = Rc::new(Self {
            window: window.as_weak(),
            library,
            playlists: RefCell::default(),
        });
        hearts.reload();

        let state = window.global::<Favourites>();
        let weak = Rc::downgrade(&hearts);
        state.on_toggle_track(move |id| {
            if let Some(hearts) = weak.upgrade()
                && id > 0
            {
                hearts.toggle(&[TrackId(i64::from(id))]);
            }
        });
        let weak = Rc::downgrade(&hearts);
        state.on_prepare_album(move |album| {
            if let Some(hearts) = weak.upgrade() {
                let tracks = hearts.tracks_of_album(album);
                hearts.prepare(&tracks);
            }
        });
        let weak = Rc::downgrade(&hearts);
        state.on_toggle_album(move |album| {
            if let Some(hearts) = weak.upgrade() {
                let tracks = hearts.tracks_of_album(album);
                hearts.toggle(&tracks);
            }
        });

        let weak = Rc::downgrade(&hearts);
        window
            .global::<TrackSelection>()
            .on_prepare_favourites(move |rows, row| {
                if let Some(hearts) = weak.upgrade() {
                    hearts.prepare(&tracks_of_rows(&rows, row));
                }
            });
        let weak = Rc::downgrade(&hearts);
        window
            .global::<TrackSelection>()
            .on_toggle_favourites(move |rows, row| {
                if let Some(hearts) = weak.upgrade() {
                    hearts.toggle(&tracks_of_rows(&rows, row));
                }
            });
        let weak = Rc::downgrade(&hearts);
        window
            .global::<SongSelection>()
            .on_prepare_favourites(move |rows, row| {
                if let Some(hearts) = weak.upgrade() {
                    hearts.prepare(&tracks_of_rows(&rows, row));
                }
            });
        let weak = Rc::downgrade(&hearts);
        window
            .global::<SongSelection>()
            .on_toggle_favourites(move |rows, row| {
                if let Some(hearts) = weak.upgrade() {
                    hearts.toggle(&tracks_of_rows(&rows, row));
                }
            });
        hearts
    }

    /// The playlists, told when the favourites change so the Favourites playlist
    /// shows them.
    pub(crate) fn set_playlists(&self, playlists: Weak<PlaylistBrowser>) {
        *self.playlists.borrow_mut() = playlists;
    }

    /// Reads the liked tracks again.
    pub fn reload(&self) {
        let liked = self.library.liked_tracks().unwrap_or_else(|error| {
            log::error!("could not read the favourites: {error}");
            HashSet::new()
        });
        LIKED.with(|set| *set.borrow_mut() = liked);
    }

    /// Likes the tracks, or (`false`) takes them out of the favourites, then shows
    /// the change everywhere.
    pub fn set(&self, tracks: &[TrackId], like: bool) {
        if tracks.is_empty() {
            return;
        }
        let result = if like {
            self.library.like(tracks)
        } else {
            self.library.unlike(tracks)
        };
        if let Err(error) = result {
            log::error!(
                "could not change the favourites of {} tracks: {error}",
                tracks.len()
            );
        }
        self.reload();
        self.show();
    }

    /// Likes the tracks, unless every one is liked: then takes them all out.
    pub fn toggle(&self, tracks: &[TrackId]) {
        self.set(tracks, !all_liked(tracks));
    }

    /// What a menu for these tracks offers.
    fn prepare(&self, tracks: &[TrackId]) {
        if let Some(window) = self.window.upgrade() {
            window
                .global::<Favourites>()
                .set_menu_removes(all_liked(tracks));
        }
    }

    /// The tracks an album has now.
    fn tracks_of_album(&self, album: i32) -> Vec<TrackId> {
        self.library
            .album_tracks(AlbumId(i64::from(album)))
            .unwrap_or_else(|error| {
                log::error!("could not read the tracks of album {album}: {error}");
                Vec::new()
            })
            .into_iter()
            .map(|track| track.id)
            .collect()
    }

    /// Every heart on screen shows the liked set: the rows of every list, the player
    /// bar, and the Favourites playlist.
    fn show(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        for rows in [
            window.get_album_tracks(),
            window.get_artist_appearances(),
            window.get_playlist_tracks(),
            window.get_label_tracks(),
            window.get_search_tracks(),
            window.get_folder_files(),
        ] {
            refresh(&rows);
        }
        refresh(&window.get_songs());
        let player = window.global::<PlayerState>();
        refresh(&player.get_up_next());
        refresh(&player.get_upcoming());
        let playing = player.get_track_id();
        player.set_liked(playing > 0 && liked(TrackId(i64::from(playing))));
        if let Some(playlists) = self.playlists.borrow().upgrade() {
            playlists.favourites_changed();
        }
    }
}

/// Whether there are tracks and every one is liked.
fn all_liked(tracks: &[TrackId]) -> bool {
    !tracks.is_empty() && tracks.iter().all(|track| liked(*track))
}

/// The tracks a list's menu acts on: the selected rows when `row` is one of them.
fn tracks_of_rows<T: Selectable + 'static>(rows: &ModelRc<T>, row: i32) -> Vec<TrackId> {
    selection::chosen_ids(rows, row)
        .into_iter()
        .filter(|id| *id > 0)
        .map(|id| TrackId(i64::from(id)))
        .collect()
}

/// A row that shows whether its track is liked.
trait Hearted: Clone {
    fn track_id(&self) -> i32;
    fn liked(&self) -> bool;
    fn set_liked(&mut self, liked: bool);
}

impl Hearted for TrackRow {
    fn track_id(&self) -> i32 {
        self.track_id
    }
    fn liked(&self) -> bool {
        self.liked
    }
    fn set_liked(&mut self, liked: bool) {
        self.liked = liked;
    }
}

impl Hearted for SongRow {
    fn track_id(&self) -> i32 {
        self.track_id
    }
    fn liked(&self) -> bool {
        self.liked
    }
    fn set_liked(&mut self, liked: bool) {
        self.liked = liked;
    }
}

impl Hearted for QueueRow {
    fn track_id(&self) -> i32 {
        self.track_id
    }
    fn liked(&self) -> bool {
        self.liked
    }
    fn set_liked(&mut self, liked: bool) {
        self.liked = liked;
    }
}

/// Writes the liked flag of the rows whose track changed, and only those.
fn refresh<T: Hearted + 'static>(rows: &ModelRc<T>) {
    for index in 0..rows.row_count() {
        let Some(mut row) = rows.row_data(index) else {
            continue;
        };
        let now = row.track_id() > 0 && liked(TrackId(i64::from(row.track_id())));
        if now != row.liked() {
            row.set_liked(now);
            rows.set_row_data(index, row);
        }
    }
}
