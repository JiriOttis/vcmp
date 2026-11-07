//! Last.fm behind the `LastfmState` global: connecting an account through Last.fm's
//! page in the browser, who is connected, the plays `PlayerService` hands over, the
//! hearts `Hearts` hands over while "Love on Last.fm" is on, and importing the
//! account's loved tracks into Favourites. Plays and loves wait in the library
//! database (`scrobble_queue`, `love_queue`) until the scrobbler thread has sent
//! them; the account's session key is in the OS keyring, and only its user name is
//! in the settings.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::time::SystemTime;

use slint::ComponentHandle;
use vcmp_integrations::lastfm::{
    self, Client, ConnectError, Credentials, ImportError, Imported, Keyring, LoveStore, Loved,
    Report, ScrobbleStore, Scrobbler, Timing,
};
use vcmp_library::{Library, QueuedLove, QueuedScrobble, Track};

use crate::favourites::{Hearts, LoveSink};
use crate::library_service::LibraryPaths;
use crate::player_service::ScrobbleSink;
use crate::{AppWindow, LastfmImport, LastfmProblem, LastfmState, LastfmStatus};

/// Setting key: the connected account's Last.fm user name; absent or empty for none.
pub const SETTING_USER: &str = "lastfm.user";
/// Setting key: "Love on Last.fm", `1` for on; off when absent.
pub const SETTING_LOVE: &str = "lastfm.love";

/// The scrobbler thread, or a recorder in tests.
pub trait LastfmWorker {
    fn resume(&self, user: &str);
    fn connect(&self);
    fn cancel(&self);
    fn disconnect(&self);
    fn now_playing(&self, track: lastfm::Track);
    /// Plays were added to the queue.
    fn flush(&self);
    /// Loves or unloves were added to the queue.
    fn flush_loves(&self);
    /// Reads the account's loved tracks and likes the matching library tracks.
    fn import_loved(&self);
}

impl LastfmWorker for Scrobbler {
    fn resume(&self, user: &str) {
        Scrobbler::resume(self, user);
    }

    fn connect(&self) {
        Scrobbler::connect(self);
    }

    fn cancel(&self) {
        Scrobbler::cancel(self);
    }

    fn disconnect(&self) {
        Scrobbler::disconnect(self);
    }

    fn now_playing(&self, track: lastfm::Track) {
        Scrobbler::now_playing(self, track);
    }

    fn flush(&self) {
        Scrobbler::flush(self);
    }

    fn flush_loves(&self) {
        Scrobbler::flush_loves(self);
    }

    fn import_loved(&self) {
        Scrobbler::import_loved(self);
    }
}

/// Opens a page in the user's browser; a recorder in tests.
pub type OpenUrl = Box<dyn Fn(&str) -> Result<(), String>>;

pub fn open_in_browser(url: &str) -> Result<(), String> {
    webbrowser::open(url).map_err(|error| error.to_string())
}

/// The scrobbler's side of the queue: the library database, through a connection
/// of its own on the scrobbler thread.
pub struct QueueStore(pub Library);

impl ScrobbleStore for QueueStore {
    fn oldest(&mut self, limit: usize) -> Result<Vec<(i64, lastfm::Scrobble)>, String> {
        let queued = self
            .0
            .queued_scrobbles(limit)
            .map_err(|error| error.to_string())?;
        Ok(queued
            .into_iter()
            .map(|QueuedScrobble { id, scrobble }| {
                let play = lastfm::Scrobble {
                    track: lastfm::Track {
                        artist: scrobble.artist,
                        title: scrobble.title,
                        album: scrobble.album,
                        album_artist: scrobble.album_artist,
                        track_number: scrobble.track_number,
                        duration: scrobble.duration,
                    },
                    started: scrobble.played_at,
                };
                (id, play)
            })
            .collect())
    }

    fn remove(&mut self, ids: &[i64]) -> Result<(), String> {
        self.0
            .remove_scrobbles(ids)
            .map_err(|error| error.to_string())
    }

    fn clear(&mut self) -> Result<(), String> {
        self.0.clear_scrobbles().map_err(|error| error.to_string())
    }
}

impl LoveStore for QueueStore {
    fn oldest_loves(&mut self, limit: usize) -> Result<Vec<(i64, lastfm::Love)>, String> {
        let queued = self
            .0
            .queued_loves(limit)
            .map_err(|error| error.to_string())?;
        Ok(queued
            .into_iter()
            .map(|QueuedLove { id, love }| {
                let love = lastfm::Love {
                    artist: love.artist,
                    title: love.title,
                    love: love.love,
                };
                (id, love)
            })
            .collect())
    }

    fn remove_loves(&mut self, ids: &[i64]) -> Result<(), String> {
        self.0.remove_loves(ids).map_err(|error| error.to_string())
    }

    fn clear_loves(&mut self) -> Result<(), String> {
        self.0.clear_loves().map_err(|error| error.to_string())
    }

    /// Likes every library track, available or not, whose artist and title match a
    /// loved track, as of its love date (now, when Last.fm gives none).
    fn like_loved(&mut self, loved: &[Loved]) -> Result<Imported, String> {
        let tracks = self.0.track_names().map_err(|error| error.to_string())?;
        let (likes, imported) = lastfm::match_loved(loved, tracks);
        let now = SystemTime::now();
        let likes: Vec<_> = likes
            .into_iter()
            .map(|(track, at)| (track, at.unwrap_or(now)))
            .collect();
        self.0.like_at(&likes).map_err(|error| error.to_string())?;
        Ok(imported)
    }
}

thread_local! {
    /// The service scrobbler reports are delivered to, on the UI thread.
    static SERVICE: RefCell<Weak<LastfmService>> = RefCell::default();
}

/// Starts Last.fm when this build has VCMP's API key (`credentials`): the scrobbler
/// thread, with its own database connection, and the Settings row. Without a key
/// nothing starts, nothing is sent and Settings shows no Last.fm row.
pub fn start(
    window: &AppWindow,
    library: Rc<Library>,
    paths: &LibraryPaths,
    credentials: Option<Credentials>,
) -> Option<Rc<LastfmService>> {
    let credentials = credentials?;
    let queue = match Library::open(&paths.database, &paths.cache) {
        Ok(connection) => QueueStore(connection),
        Err(error) => {
            log::error!("Last.fm is off, the scrobble queue could not be opened: {error}");
            return None;
        }
    };
    let scrobbler = Scrobbler::start(
        Client::online(credentials),
        Keyring,
        queue,
        Timing::default(),
        |report| {
            LastfmService::post(report);
        },
    );
    Some(LastfmService::connect(
        window,
        library,
        Box::new(scrobbler),
        Box::new(open_in_browser),
    ))
}

pub struct LastfmService {
    window: slint::Weak<AppWindow>,
    library: Rc<Library>,
    worker: Box<dyn LastfmWorker>,
    open_url: OpenUrl,
    // The page where the user allows VCMP, while connecting.
    auth_url: RefCell<Option<String>>,
    // Shown again after an import likes tracks.
    hearts: RefCell<Weak<Hearts>>,
}

impl LastfmService {
    /// Shows the Last.fm row and resumes the saved account, if there is one.
    pub fn connect(
        window: &AppWindow,
        library: Rc<Library>,
        worker: Box<dyn LastfmWorker>,
        open_url: OpenUrl,
    ) -> Rc<Self> {
        let service = Rc::new(Self {
            window: window.as_weak(),
            library,
            worker,
            open_url,
            auth_url: RefCell::default(),
            hearts: RefCell::default(),
        });
        SERVICE.with(|current| *current.borrow_mut() = Rc::downgrade(&service));

        let state = window.global::<LastfmState>();
        state.set_available(true);
        state.set_love(service.love_saved());
        if let Some(user) = service.saved_user() {
            state.set_status(LastfmStatus::Connected);
            state.set_user(user.as_str().into());
            service.worker.resume(&user);
        }
        service.show_unsent();

        let weak = Rc::downgrade(&service);
        let with = move |action: fn(&LastfmService)| {
            let weak = weak.clone();
            move || {
                if let Some(service) = weak.upgrade() {
                    action(&service);
                }
            }
        };
        state.on_connect(with(LastfmService::start_connecting));
        state.on_cancel(with(LastfmService::cancel));
        state.on_disconnect(with(LastfmService::disconnect));
        state.on_open_auth_page(with(|service| {
            let url = service.auth_url.borrow().clone();
            if let Some(url) = url {
                service.open(&url);
            }
        }));
        state.on_import_loved(with(LastfmService::import_loved));
        let weak = Rc::downgrade(&service);
        state.on_set_love(move |on| {
            if let Some(service) = weak.upgrade() {
                service.set_love(on);
            }
        });
        service
    }

    /// Hears every heart given or taken away from now on, and shows an import's
    /// likes through them.
    pub fn attach(self: &Rc<Self>, hearts: &Rc<Hearts>) {
        *self.hearts.borrow_mut() = Rc::downgrade(hearts);
        let sink: Weak<dyn LoveSink> = Rc::downgrade(self) as Weak<dyn LoveSink>;
        hearts.set_love_sink(sink);
    }

    /// Runs `report` through the service on the UI thread, from any thread. False
    /// once the event loop is gone.
    pub fn post(report: Report) -> bool {
        slint::invoke_from_event_loop(move || {
            if let Some(service) = SERVICE.with(|current| current.borrow().upgrade()) {
                service.report(report);
            }
        })
        .is_ok()
    }

    /// Takes what the scrobbler reports.
    pub fn report(&self, report: Report) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<LastfmState>();
        let waiting = state.get_status() == LastfmStatus::Waiting;
        match report {
            Report::Authorize(url) if waiting => {
                *self.auth_url.borrow_mut() = Some(url.clone());
                self.open(&url);
            }
            Report::Connected { user, saved } => {
                if !waiting {
                    // Cancelled as the user allowed VCMP: the cancel wins.
                    self.worker.disconnect();
                    return;
                }
                self.save_user(&user);
                *self.auth_url.borrow_mut() = None;
                state.set_status(LastfmStatus::Connected);
                state.set_user(user.into());
                match saved {
                    Ok(()) => set_problem(&state, LastfmProblem::None, ""),
                    Err(error) => {
                        log::warn!("the Last.fm key could not be saved in the keyring: {error}");
                        set_problem(&state, LastfmProblem::Keyring, &error);
                    }
                }
            }
            Report::ConnectFailed(error) if waiting => {
                *self.auth_url.borrow_mut() = None;
                state.set_status(LastfmStatus::Disconnected);
                match error {
                    ConnectError::TimedOut => set_problem(&state, LastfmProblem::TimedOut, ""),
                    ConnectError::Failed(error) => {
                        log::info!("connecting to Last.fm failed: {error}");
                        set_problem(&state, LastfmProblem::ConnectFailed, &error.to_string());
                    }
                }
            }
            Report::KeyMissing => {
                self.save_user("");
                state.set_status(LastfmStatus::Disconnected);
                set_problem(&state, LastfmProblem::KeyMissing, "");
            }
            // The account stays saved, so the next start tries the keyring again.
            Report::KeyUnreadable(error) => {
                log::warn!("the Last.fm key could not be read from the keyring: {error}");
                state.set_status(LastfmStatus::Disconnected);
                set_problem(&state, LastfmProblem::Keyring, &error);
            }
            Report::Rejected => {
                self.save_user("");
                state.set_status(LastfmStatus::Disconnected);
                set_problem(&state, LastfmProblem::Rejected, "");
            }
            Report::Sent(_) | Report::Loved(_) => {}
            Report::ImportProgress { page, pages } => {
                state.set_import_page(page as i32);
                state.set_import_pages(pages as i32);
            }
            Report::Imported(imported) => {
                state.set_importing(false);
                state.set_import_matched(imported.matched as i32);
                state.set_import_unmatched(imported.unmatched as i32);
                state.set_import_result(LastfmImport::Done);
                if let Some(hearts) = self.hearts.borrow().upgrade() {
                    hearts.refresh();
                }
            }
            Report::ImportFailed(error) => {
                state.set_importing(false);
                let (result, detail) = match error {
                    ImportError::NoAccount => (LastfmImport::NoAccount, String::new()),
                    ImportError::Lastfm(error) => {
                        log::info!("importing loved tracks failed: {error}");
                        (LastfmImport::Failed, error.to_string())
                    }
                    ImportError::Library(error) => {
                        log::error!("could not like the loved tracks: {error}");
                        (LastfmImport::NotLiked, error)
                    }
                };
                state.set_import_result(result);
                state.set_import_detail(detail.into());
            }
            Report::Authorize(_) | Report::ConnectFailed(_) => {}
        }
        self.show_unsent();
    }

    fn import_loved(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<LastfmState>();
        if state.get_importing() || !self.connected() {
            return;
        }
        state.set_importing(true);
        state.set_import_page(0);
        state.set_import_pages(0);
        state.set_import_result(LastfmImport::None);
        state.set_import_detail("".into());
        self.worker.import_loved();
    }

    /// "Love on Last.fm": hearts from now on are sent, or no longer queued. Loves
    /// waiting already still go out.
    fn set_love(&self, on: bool) {
        if let Some(window) = self.window.upgrade() {
            window.global::<LastfmState>().set_love(on);
        }
        if let Err(error) = self
            .library
            .set_setting(SETTING_LOVE, if on { "1" } else { "0" })
        {
            log::warn!("could not save setting {SETTING_LOVE}: {error}");
        }
    }

    fn love_saved(&self) -> bool {
        self.library
            .setting(SETTING_LOVE)
            .unwrap_or_else(|error| {
                log::warn!("could not read setting {SETTING_LOVE}: {error}");
                None
            })
            .is_some_and(|value| value == "1")
    }

    fn love_on(&self) -> bool {
        self.window
            .upgrade()
            .is_some_and(|window| window.global::<LastfmState>().get_love())
    }

    fn start_connecting(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<LastfmState>();
        *self.auth_url.borrow_mut() = None;
        state.set_status(LastfmStatus::Waiting);
        set_problem(&state, LastfmProblem::None, "");
        self.worker.connect();
    }

    fn cancel(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<LastfmState>();
        if state.get_status() == LastfmStatus::Waiting {
            *self.auth_url.borrow_mut() = None;
            state.set_status(LastfmStatus::Disconnected);
            set_problem(&state, LastfmProblem::None, "");
            self.worker.cancel();
        }
    }

    /// Forgets the account and the plays not sent yet.
    fn disconnect(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<LastfmState>();
        self.worker.disconnect();
        self.save_user("");
        state.set_status(LastfmStatus::Disconnected);
        state.set_user("".into());
        set_problem(&state, LastfmProblem::None, "");
        // The scrobbler empties the queues, after any request it is sending, and
        // stops an import.
        state.set_unsent(0);
        state.set_unsent_loves(0);
        state.set_importing(false);
        state.set_import_result(LastfmImport::None);
    }

    fn open(&self, url: &str) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<LastfmState>();
        match (self.open_url)(url) {
            Ok(()) => {
                if state.get_problem() == LastfmProblem::Browser {
                    set_problem(&state, LastfmProblem::None, "");
                }
            }
            Err(error) => {
                log::warn!("could not open {url}: {error}");
                set_problem(&state, LastfmProblem::Browser, &error);
            }
        }
    }

    fn connected(&self) -> bool {
        self.window.upgrade().is_some_and(|window| {
            window.global::<LastfmState>().get_status() == LastfmStatus::Connected
        })
    }

    fn show_unsent(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let unsent = self
            .library
            .queued_scrobble_count()
            .unwrap_or_else(|error| {
                log::warn!("could not count the scrobble queue: {error}");
                0
            });
        let loves = self.library.queued_love_count().unwrap_or_else(|error| {
            log::warn!("could not count the love queue: {error}");
            0
        });
        let state = window.global::<LastfmState>();
        state.set_unsent(unsent as i32);
        state.set_unsent_loves(loves as i32);
    }

    fn saved_user(&self) -> Option<String> {
        self.library
            .setting(SETTING_USER)
            .unwrap_or_else(|error| {
                log::warn!("could not read setting {SETTING_USER}: {error}");
                None
            })
            .filter(|user| !user.is_empty())
    }

    fn save_user(&self, user: &str) {
        if let Err(error) = self.library.set_setting(SETTING_USER, user) {
            log::warn!("could not save setting {SETTING_USER}: {error}");
        }
    }
}

fn set_problem(state: &LastfmState, problem: LastfmProblem, detail: &str) {
    state.set_problem(problem);
    state.set_problem_detail(detail.into());
}

impl ScrobbleSink for LastfmService {
    fn now_playing(&self, track: lastfm::Track) {
        if self.connected() {
            self.worker.now_playing(track);
        }
    }

    /// Plays are kept only while an account is connected.
    fn scrobble(&self, play: lastfm::Scrobble) {
        if !self.connected() {
            return;
        }
        let track = play.track;
        let queued = self.library.queue_scrobble(&vcmp_library::Scrobble {
            artist: track.artist,
            title: track.title,
            album: track.album,
            album_artist: track.album_artist,
            track_number: track.track_number,
            duration: track.duration,
            played_at: play.started,
        });
        match queued {
            Ok(_) => self.worker.flush(),
            Err(error) => log::warn!("could not queue a play for Last.fm: {error}"),
        }
        self.show_unsent();
    }
}

impl LoveSink for LastfmService {
    /// Queued only while an account is connected and "Love on Last.fm" is on. Tracks
    /// without an artist can't be loved, as they can't be scrobbled.
    fn hearts_changed(&self, tracks: &[Track], liked: bool) {
        if !self.connected() || !self.love_on() {
            return;
        }
        let loves: Vec<vcmp_library::Love> = tracks
            .iter()
            .filter_map(|track| {
                Some(vcmp_library::Love {
                    artist: track.artist.clone()?,
                    title: track.title.clone(),
                    love: liked,
                })
            })
            .collect();
        if loves.is_empty() {
            return;
        }
        match self.library.queue_loves(&loves) {
            Ok(()) => self.worker.flush_loves(),
            Err(error) => log::warn!("could not queue loves for Last.fm: {error}"),
        }
        self.show_unsent();
    }
}
