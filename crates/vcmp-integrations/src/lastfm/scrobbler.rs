//! The scrobbler thread: connecting an account, keeping its session key in the
//! keyring, "now playing" updates, sending the queued plays and loves, and reading
//! the account's loved tracks for an import.
//!
//! Plays wait in a [`ScrobbleStore`] until Last.fm has them. They go out in batches
//! of up to 50 whenever there is an account, and after a failure (offline, Last.fm
//! busy) the scrobbler waits before the next try, twice as long each time up to
//! half an hour. A "now playing" update that goes through ends the wait early: the
//! network is back. Loves and unloves wait in a [`LoveStore`] and go out one by one
//! after the plays, with the same wait.
//!
//! An import reads one page of loved tracks each time round, so plays, loves and
//! the other jobs are not held up while a long list comes in. Once every page is
//! read, the store likes the library tracks that match.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use super::{
    Client, Error, Imported, Love, Loved, MAX_BATCH, Remedy, Scrobble, Session, Track, Transport,
};

/// How often reading a page of loved tracks is tried while Last.fm is busy or
/// unreachable, before the import gives up.
const IMPORT_TRIES: u32 = 3;

/// Where session keys are kept, by user name: the OS keyring, or a map in tests.
pub trait SessionStore: Send + 'static {
    fn load(&mut self, user: &str) -> Result<Option<String>, String>;
    fn save(&mut self, user: &str, key: &str) -> Result<(), String>;
    fn delete(&mut self, user: &str) -> Result<(), String>;
}

/// The OS keyring: the Keychain on macOS, the Credential Manager on Windows, the
/// Secret Service (GNOME Keyring, KWallet) on Linux and the BSDs.
pub struct Keyring;

impl Keyring {
    /// The service the entries are filed under: the app's identifier.
    pub const SERVICE: &str = "io.github.dalibor-osu.vcmp";

    fn entry(user: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(Self::SERVICE, &format!("lastfm:{user}")).map_err(|e| e.to_string())
    }
}

impl SessionStore for Keyring {
    fn load(&mut self, user: &str) -> Result<Option<String>, String> {
        match Self::entry(user)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&mut self, user: &str, key: &str) -> Result<(), String> {
        Self::entry(user)?
            .set_password(key)
            .map_err(|error| error.to_string())
    }

    fn delete(&mut self, user: &str) -> Result<(), String> {
        match Self::entry(user)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// Plays not sent yet: the library database, or a list in tests.
pub trait ScrobbleStore: Send + 'static {
    /// The oldest `limit` plays, oldest first, with the ids `remove` takes.
    fn oldest(&mut self, limit: usize) -> Result<Vec<(i64, Scrobble)>, String>;
    fn remove(&mut self, ids: &[i64]) -> Result<(), String>;
    fn clear(&mut self) -> Result<(), String>;
}

/// Loves and unloves not sent yet, and the library an import likes tracks in: the
/// library database, or lists in tests.
pub trait LoveStore: Send + 'static {
    /// The oldest `limit` loves and unloves, oldest first, with the ids
    /// `remove_loves` takes.
    fn oldest_loves(&mut self, limit: usize) -> Result<Vec<(i64, Love)>, String>;
    fn remove_loves(&mut self, ids: &[i64]) -> Result<(), String>;
    fn clear_loves(&mut self) -> Result<(), String>;
    /// Likes the library tracks that match `loved`, without queueing loves for them.
    fn like_loved(&mut self, loved: &[Loved]) -> Result<Imported, String>;
}

/// How long the scrobbler waits for things; tests shorten them.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// Between asking whether the user has allowed VCMP yet.
    pub poll: Duration,
    /// How long connecting waits for the user before giving up.
    pub give_up: Duration,
    /// The wait after a failed send. It doubles with each failure in a row.
    pub backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_secs(3),
            give_up: Duration::from_secs(15 * 60),
            backoff: Duration::from_secs(30),
            max_backoff: Duration::from_secs(30 * 60),
        }
    }
}

/// What the scrobbler tells its owner, on the scrobbler's thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// Connecting: the user allows VCMP on this page.
    Authorize(String),
    /// An account is connected. `saved` says whether its key reached the keyring;
    /// if not, the account lasts until VCMP closes.
    Connected {
        user: String,
        saved: Result<(), String>,
    },
    ConnectFailed(ConnectError),
    /// The keyring has no key for the account to resume; connect again.
    KeyMissing,
    /// The keyring could not be read, so the account cannot resume.
    KeyUnreadable(String),
    /// Last.fm no longer takes the account's key: the user revoked VCMP's access.
    /// The key is forgotten and the queue kept, for when they connect again.
    Rejected,
    /// Last.fm has these plays now (it may have ignored some); they left the queue.
    Sent(usize),
    /// Last.fm has these loves and unloves now, or refused them; they left the queue.
    Loved(usize),
    /// An import has read `page` of `pages` pages of loved tracks.
    ImportProgress {
        page: u32,
        pages: u32,
    },
    /// An import is done.
    Imported(Imported),
    ImportFailed(ImportError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
    /// No account is connected, so there are no loved tracks to read.
    NoAccount,
    /// Last.fm refused, or could not be reached.
    Lastfm(Error),
    /// The library could not like the tracks.
    Library(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectError {
    /// The user did not allow VCMP in time.
    TimedOut,
    /// Last.fm refused, or could not be reached.
    Failed(Error),
}

enum Job {
    Resume(String),
    Connect,
    Cancel,
    Disconnect,
    NowPlaying(Track),
    Flush,
    Loves,
    Import,
}

/// Runs the scrobbler thread. Dropping it ends the thread once the request in
/// flight is done.
pub struct Scrobbler {
    jobs: Sender<Job>,
}

impl Scrobbler {
    pub fn start<T: Transport>(
        client: Client<T>,
        sessions: impl SessionStore,
        queue: impl ScrobbleStore + LoveStore,
        timing: Timing,
        on_report: impl Fn(Report) + Send + 'static,
    ) -> Self {
        let (jobs, inbox) = mpsc::channel();
        let mut worker = Worker {
            client,
            sessions,
            queue,
            timing,
            report: Box::new(on_report),
            account: None,
            waiting: None,
            unsent: false,
            unloved: false,
            import: None,
            retry_at: None,
            backoff: timing.backoff,
            batch: MAX_BATCH,
        };
        thread::Builder::new()
            .name("vcmp-lastfm".into())
            .spawn(move || worker.run(inbox))
            .expect("spawning the Last.fm thread");
        Self { jobs }
    }

    /// Scrobbles for `user` with the key the keyring holds for them, and sends
    /// what is left in the queue.
    pub fn resume(&self, user: &str) {
        self.send(Job::Resume(user.to_string()));
    }

    /// Starts connecting an account: [`Report::Authorize`] names the page to open.
    pub fn connect(&self) {
        self.send(Job::Connect);
    }

    /// Stops waiting for the user to allow VCMP.
    pub fn cancel(&self) {
        self.send(Job::Cancel);
    }

    /// Forgets the account (its key leaves the keyring) and the plays not sent.
    pub fn disconnect(&self) {
        self.send(Job::Disconnect);
    }

    /// Shows `track` as playing now; not retried if it fails.
    pub fn now_playing(&self, track: Track) {
        self.send(Job::NowPlaying(track));
    }

    /// Plays were added to the queue.
    pub fn flush(&self) {
        self.send(Job::Flush);
    }

    /// Loves or unloves were added to the queue.
    pub fn flush_loves(&self) {
        self.send(Job::Loves);
    }

    /// Reads the connected account's loved tracks and likes the library tracks that
    /// match: [`Report::ImportProgress`] for each page, then [`Report::Imported`] or
    /// [`Report::ImportFailed`]. Asked again while one runs, that one goes on.
    pub fn import_loved(&self) {
        self.send(Job::Import);
    }

    fn send(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

struct Waiting {
    token: String,
    next_poll: Instant,
    until: Instant,
}

/// An import under way.
struct Import {
    user: String,
    // The page to read next, and how many there are as far as is known.
    next_page: u32,
    pages: u32,
    loved: Vec<Loved>,
    // Failed tries of this page, and when to read it.
    tries: u32,
    at: Instant,
}

struct Worker<T, S, Q> {
    client: Client<T>,
    sessions: S,
    queue: Q,
    timing: Timing,
    report: Box<dyn Fn(Report) + Send>,
    account: Option<Session>,
    // The token the user is asked to allow.
    waiting: Option<Waiting>,
    // The queue may hold plays; false once it was found empty.
    unsent: bool,
    // The same for loves.
    unloved: bool,
    import: Option<Import>,
    // After a failed send: when to try again, and the wait after the next failure.
    retry_at: Option<Instant>,
    backoff: Duration,
    // Plays per request: one at a time while looking for a play Last.fm refuses.
    batch: usize,
}

impl<T: Transport, S: SessionStore, Q: ScrobbleStore + LoveStore> Worker<T, S, Q> {
    fn run(&mut self, inbox: Receiver<Job>) {
        loop {
            let job = match self.next_wake() {
                Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
                None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match job {
                Ok(job) => self.handle(job),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.poll();
            self.send_queue();
            self.send_loves();
            self.import_page();
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        let poll = self.waiting.as_ref().map(|waiting| waiting.next_poll);
        let send = ((self.unsent || self.unloved) && self.account.is_some())
            .then(|| self.retry_at.unwrap_or_else(Instant::now));
        let import = self.import.as_ref().map(|import| import.at);
        poll.into_iter().chain(send).chain(import).min()
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Resume(user) => match self.sessions.load(&user) {
                Ok(Some(key)) => {
                    self.account = Some(Session { user, key });
                    self.unsent = true;
                    self.unloved = true;
                }
                Ok(None) => (self.report)(Report::KeyMissing),
                Err(error) => (self.report)(Report::KeyUnreadable(error)),
            },
            Job::Connect => match self.client.token() {
                Ok(token) => {
                    let now = Instant::now();
                    (self.report)(Report::Authorize(self.client.auth_url(&token)));
                    self.waiting = Some(Waiting {
                        token,
                        next_poll: now + self.timing.poll,
                        until: now + self.timing.give_up,
                    });
                }
                Err(error) => (self.report)(Report::ConnectFailed(ConnectError::Failed(error))),
            },
            Job::Cancel => self.waiting = None,
            Job::Disconnect => {
                self.waiting = None;
                if let Some(account) = self.account.take()
                    && let Err(error) = self.sessions.delete(&account.user)
                {
                    log::warn!("could not remove the Last.fm key from the keyring: {error}");
                }
                if let Err(error) = self.queue.clear() {
                    log::warn!("could not empty the scrobble queue: {error}");
                }
                if let Err(error) = self.queue.clear_loves() {
                    log::warn!("could not empty the love queue: {error}");
                }
                self.unsent = false;
                self.unloved = false;
                self.import = None;
                self.retry_at = None;
                self.backoff = self.timing.backoff;
            }
            Job::NowPlaying(track) => {
                let Some(account) = &self.account else {
                    return;
                };
                match self.client.now_playing(&account.key, &track) {
                    // Last.fm is reachable again: no need to wait out the backoff.
                    Ok(()) => self.retry_at = None,
                    Err(error) if error.remedy() == Remedy::Reconnect => self.rejected(),
                    Err(error) => log::info!("Last.fm did not take \"now playing\": {error}"),
                }
            }
            Job::Flush => self.unsent = true,
            Job::Loves => self.unloved = true,
            Job::Import => {
                if self.import.is_some() {
                    return;
                }
                match &self.account {
                    Some(account) => {
                        self.import = Some(Import {
                            user: account.user.clone(),
                            next_page: 1,
                            pages: 1,
                            loved: Vec::new(),
                            tries: 0,
                            at: Instant::now(),
                        });
                    }
                    None => (self.report)(Report::ImportFailed(ImportError::NoAccount)),
                }
            }
        }
    }

    /// Asks whether the user has allowed VCMP yet, when it is time to.
    fn poll(&mut self) {
        let Some(waiting) = &self.waiting else {
            return;
        };
        let now = Instant::now();
        if now < waiting.next_poll {
            return;
        }
        let failure = match self.client.session(&waiting.token) {
            Ok(session) => {
                self.waiting = None;
                if let Some(old) = self.account.take()
                    && old.user != session.user
                    && let Err(error) = self.sessions.delete(&old.user)
                {
                    log::warn!("could not remove the old Last.fm key: {error}");
                }
                let saved = self.sessions.save(&session.user, &session.key);
                (self.report)(Report::Connected {
                    user: session.user.clone(),
                    saved,
                });
                self.account = Some(session);
                self.unsent = true;
                self.unloved = true;
                return;
            }
            Err(error) if error.is_expired_token() => ConnectError::TimedOut,
            // Not allowed yet, or a dropped connection: ask again in a moment.
            Err(error) if error.is_not_allowed_yet() || error.remedy() == Remedy::RetryLater => {
                if now >= waiting.until {
                    ConnectError::TimedOut
                } else {
                    let poll = self.timing.poll;
                    if let Some(waiting) = &mut self.waiting {
                        waiting.next_poll = now + poll;
                    }
                    return;
                }
            }
            Err(error) => ConnectError::Failed(error),
        };
        self.waiting = None;
        (self.report)(Report::ConnectFailed(failure));
    }

    /// Sends the queue in batches, when there is an account and no wait is due.
    fn send_queue(&mut self) {
        while self.unsent {
            let Some(account) = &self.account else {
                return;
            };
            if self.retry_at.is_some_and(|at| Instant::now() < at) {
                return;
            }
            let batch = match self.queue.oldest(self.batch) {
                Ok(batch) => batch,
                Err(error) => {
                    log::warn!("could not read the scrobble queue: {error}");
                    self.wait();
                    return;
                }
            };
            if batch.is_empty() {
                self.unsent = false;
                self.batch = MAX_BATCH;
                return;
            }
            let (ids, plays): (Vec<i64>, Vec<Scrobble>) = batch.into_iter().unzip();
            match self.client.scrobble(&account.key, &plays) {
                Ok(sent) => {
                    if sent.ignored > 0 {
                        log::info!("Last.fm ignored {} of {} plays", sent.ignored, plays.len());
                    }
                    self.retry_at = None;
                    self.backoff = self.timing.backoff;
                    self.taken(&ids);
                }
                Err(error) => match error.remedy() {
                    Remedy::RetryLater => {
                        log::info!("scrobbling failed, trying again later: {error}");
                        self.wait();
                        return;
                    }
                    Remedy::Reconnect => {
                        self.rejected();
                        return;
                    }
                    Remedy::NewKey => {
                        log::error!("Last.fm refuses VCMP's API key: {error}");
                        self.retry_at = Some(Instant::now() + self.timing.max_backoff);
                        return;
                    }
                    // A play Last.fm cannot take would hold up the rest forever: find
                    // it by sending one at a time, and drop it.
                    Remedy::Drop if ids.len() > 1 => self.batch = 1,
                    Remedy::Drop => {
                        log::warn!("Last.fm refused a play, dropping it: {error}");
                        self.taken(&ids);
                        self.batch = MAX_BATCH;
                    }
                },
            }
        }
    }

    /// Sends the queued loves and unloves one by one, once the plays are out.
    fn send_loves(&mut self) {
        while self.unloved && !self.unsent {
            let Some(account) = &self.account else {
                return;
            };
            if self.retry_at.is_some_and(|at| Instant::now() < at) {
                return;
            }
            let (id, love) = match self.queue.oldest_loves(1) {
                Ok(mut loves) if !loves.is_empty() => loves.remove(0),
                Ok(_) => {
                    self.unloved = false;
                    return;
                }
                Err(error) => {
                    log::warn!("could not read the love queue: {error}");
                    self.wait();
                    return;
                }
            };
            let sent = if love.love {
                self.client.love(&account.key, &love.artist, &love.title)
            } else {
                self.client.unlove(&account.key, &love.artist, &love.title)
            };
            match sent {
                Ok(()) => {
                    self.retry_at = None;
                    self.backoff = self.timing.backoff;
                    self.loves_taken(id);
                }
                Err(error) => match error.remedy() {
                    Remedy::RetryLater => {
                        log::info!("sending a love failed, trying again later: {error}");
                        self.wait();
                        return;
                    }
                    Remedy::Reconnect => {
                        self.rejected();
                        return;
                    }
                    Remedy::NewKey => {
                        log::error!("Last.fm refuses VCMP's API key: {error}");
                        self.retry_at = Some(Instant::now() + self.timing.max_backoff);
                        return;
                    }
                    Remedy::Drop => {
                        log::warn!("Last.fm refused a love, dropping it: {error}");
                        self.loves_taken(id);
                    }
                },
            }
        }
    }

    fn loves_taken(&mut self, id: i64) {
        match self.queue.remove_loves(&[id]) {
            Ok(()) => (self.report)(Report::Loved(1)),
            Err(error) => {
                log::warn!("could not take a sent love out of the queue: {error}");
                self.wait();
            }
        }
    }

    /// Reads the next page of loved tracks for the import under way, when it is time
    /// to; after the last page, likes the matching tracks.
    fn import_page(&mut self) {
        let Some(import) = &mut self.import else {
            return;
        };
        let now = Instant::now();
        if now < import.at {
            return;
        }
        let failure = match self.client.loved_tracks(&import.user, import.next_page) {
            Ok(page) => {
                import.loved.extend(page.tracks);
                import.pages = page.pages;
                import.tries = 0;
                (self.report)(Report::ImportProgress {
                    page: import.next_page.min(import.pages),
                    pages: import.pages,
                });
                if import.next_page < import.pages {
                    import.next_page += 1;
                    return;
                }
                None
            }
            Err(error)
                if error.remedy() == Remedy::RetryLater && import.tries + 1 < IMPORT_TRIES =>
            {
                log::info!("reading loved tracks failed, trying again: {error}");
                import.tries += 1;
                import.at = now + self.timing.backoff * 2u32.pow(import.tries - 1);
                return;
            }
            Err(error) => Some(error),
        };
        let Some(import) = self.import.take() else {
            return;
        };
        let report = match failure {
            Some(error) => Report::ImportFailed(ImportError::Lastfm(error)),
            None => match self.queue.like_loved(&import.loved) {
                Ok(imported) => Report::Imported(imported),
                Err(error) => Report::ImportFailed(ImportError::Library(error)),
            },
        };
        (self.report)(report);
    }

    /// Plays Last.fm is done with leave the queue.
    fn taken(&mut self, ids: &[i64]) {
        match self.queue.remove(ids) {
            Ok(()) => (self.report)(Report::Sent(ids.len())),
            Err(error) => {
                log::warn!("could not take sent plays out of the queue: {error}");
                self.wait();
            }
        }
    }

    fn wait(&mut self) {
        self.retry_at = Some(Instant::now() + self.backoff);
        self.backoff = (self.backoff * 2).min(self.timing.max_backoff);
    }

    fn rejected(&mut self) {
        if let Some(account) = self.account.take()
            && let Err(error) = self.sessions.delete(&account.user)
        {
            log::warn!("could not remove the rejected Last.fm key: {error}");
        }
        (self.report)(Report::Rejected);
    }
}

/// Session keys in memory, for tests.
#[derive(Clone, Debug, Default)]
pub struct MemorySessions(pub std::sync::Arc<std::sync::Mutex<HashMap<String, String>>>);

impl SessionStore for MemorySessions {
    fn load(&mut self, user: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(user).cloned())
    }

    fn save(&mut self, user: &str, key: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(user.into(), key.into());
        Ok(())
    }

    fn delete(&mut self, user: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(user);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::tests::{Requests, Scripted, api_error, credentials, ok, track, value};
    use super::*;

    /// Plays and loves in memory, shared with the test.
    #[derive(Clone, Default)]
    struct Plays(Arc<Mutex<Vec<(i64, Scrobble)>>>, Arc<Mutex<Loves>>);

    #[derive(Default)]
    struct Loves {
        queue: Vec<(i64, Love)>,
        // What imports asked to like.
        imported: Vec<Loved>,
    }

    impl Plays {
        fn with(count: usize) -> Self {
            let plays = Self::default();
            for index in 0..count {
                plays.0.lock().unwrap().push((
                    index as i64,
                    Scrobble {
                        track: track(&format!("Track {index}")),
                        started: UNIX_EPOCH + Duration::from_secs(1_700_000_000 + index as u64),
                    },
                ));
            }
            plays
        }

        fn titles(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .map(|(_, play)| play.track.title.clone())
                .collect()
        }
    }

    impl ScrobbleStore for Plays {
        fn oldest(&mut self, limit: usize) -> Result<Vec<(i64, Scrobble)>, String> {
            Ok(self.0.lock().unwrap().iter().take(limit).cloned().collect())
        }

        fn remove(&mut self, ids: &[i64]) -> Result<(), String> {
            self.0.lock().unwrap().retain(|(id, _)| !ids.contains(id));
            Ok(())
        }

        fn clear(&mut self) -> Result<(), String> {
            self.0.lock().unwrap().clear();
            Ok(())
        }
    }

    impl Plays {
        fn love(&self, title: &str, love: bool) {
            let mut loves = self.1.lock().unwrap();
            let id = loves.queue.len() as i64 + 1;
            loves.queue.push((
                id,
                Love {
                    artist: "Band".into(),
                    title: title.into(),
                    love,
                },
            ));
        }

        fn loves_left(&self) -> usize {
            self.1.lock().unwrap().queue.len()
        }

        fn imported(&self) -> Vec<String> {
            let loves = self.1.lock().unwrap();
            loves
                .imported
                .iter()
                .map(|loved| loved.title.clone())
                .collect()
        }
    }

    impl LoveStore for Plays {
        fn oldest_loves(&mut self, limit: usize) -> Result<Vec<(i64, Love)>, String> {
            let loves = self.1.lock().unwrap();
            Ok(loves.queue.iter().take(limit).cloned().collect())
        }

        fn remove_loves(&mut self, ids: &[i64]) -> Result<(), String> {
            self.1
                .lock()
                .unwrap()
                .queue
                .retain(|(id, _)| !ids.contains(id));
            Ok(())
        }

        fn clear_loves(&mut self) -> Result<(), String> {
            self.1.lock().unwrap().queue.clear();
            Ok(())
        }

        /// Every loved track "matches", but one titled "Nowhere".
        fn like_loved(&mut self, loved: &[Loved]) -> Result<Imported, String> {
            let mut loves = self.1.lock().unwrap();
            loves.imported.extend_from_slice(loved);
            let unmatched = loved
                .iter()
                .filter(|loved| loved.title == "Nowhere")
                .count();
            Ok(Imported {
                matched: loved.len() - unmatched,
                unmatched,
            })
        }
    }

    const QUICK: Timing = Timing {
        poll: Duration::from_millis(5),
        give_up: Duration::from_secs(5),
        backoff: Duration::from_millis(40),
        max_backoff: Duration::from_millis(100),
    };

    struct Running {
        scrobbler: Scrobbler,
        reports: Receiver<(Report, Instant)>,
        requests: Requests,
        sessions: MemorySessions,
    }

    impl Running {
        fn next(&self) -> Report {
            self.next_at().0
        }

        fn next_at(&self) -> (Report, Instant) {
            self.reports
                .recv_timeout(Duration::from_secs(5))
                .expect("a report from the scrobbler")
        }

        fn methods(&self) -> Vec<String> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .map(|form| value(form, "method").unwrap_or_default().to_string())
                .collect()
        }

        /// The keyring holds a key for "listener".
        fn store_key(&self) {
            self.sessions
                .0
                .lock()
                .unwrap()
                .insert("listener".into(), "sk-1".into());
        }

        /// Gives the scrobbler time to act on the jobs sent so far, to check that
        /// they lead to nothing more.
        fn settle(&self) {
            thread::sleep(Duration::from_millis(50));
        }

        /// Waits until the scrobbler has made `count` requests.
        fn requested(&self, count: usize) {
            let until = Instant::now() + Duration::from_secs(5);
            while self.requests.lock().unwrap().len() < count {
                assert!(
                    Instant::now() < until,
                    "only {:?} were requested",
                    self.methods()
                );
                thread::sleep(Duration::from_millis(2));
            }
        }
    }

    fn start(
        answers: impl IntoIterator<Item = Result<(u16, String), String>>,
        plays: Plays,
        timing: Timing,
    ) -> Running {
        let (transport, requests) = Scripted::new(answers);
        let sessions = MemorySessions::default();
        let (sender, reports) = mpsc::channel();
        let scrobbler = Scrobbler::start(
            Client::new(transport, credentials()),
            sessions.clone(),
            plays,
            timing,
            move |report| {
                let _ = sender.send((report, Instant::now()));
            },
        );
        Running {
            scrobbler,
            reports,
            requests,
            sessions,
        }
    }

    fn accepted(count: usize) -> Result<(u16, String), String> {
        ok(&format!(
            r#"{{"scrobbles":{{"scrobble":[],"@attr":{{"accepted":{count},"ignored":0}}}}}}"#
        ))
    }

    fn session(user: &str, key: &str) -> Result<(u16, String), String> {
        ok(&format!(
            r#"{{"session":{{"name":"{user}","key":"{key}","subscriber":0}}}}"#
        ))
    }

    #[test]
    fn connecting_waits_until_the_user_allows_vcmp() {
        let plays = Plays::default();
        let running = start(
            [
                ok(r#"{"token":"tok"}"#),
                api_error(14),
                api_error(14),
                session("listener", "sk-1"),
            ],
            plays,
            QUICK,
        );
        running.scrobbler.connect();
        assert_eq!(
            running.next(),
            Report::Authorize("https://www.last.fm/api/auth/?api_key=the-key&token=tok".into())
        );
        assert_eq!(
            running.next(),
            Report::Connected {
                user: "listener".into(),
                saved: Ok(())
            }
        );
        assert_eq!(
            running.methods(),
            [
                "auth.getToken",
                "auth.getSession",
                "auth.getSession",
                "auth.getSession"
            ]
        );
        assert_eq!(
            running.sessions.0.lock().unwrap().get("listener"),
            Some(&"sk-1".to_string())
        );
    }

    #[test]
    fn cancelling_stops_the_asking() {
        let running = start(
            [ok(r#"{"token":"tok"}"#)],
            Plays::default(),
            Timing {
                poll: Duration::from_millis(100),
                ..QUICK
            },
        );
        running.scrobbler.connect();
        assert!(matches!(running.next(), Report::Authorize(_)));
        running.scrobbler.cancel();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(running.methods(), ["auth.getToken"]);
    }

    #[test]
    fn connecting_gives_up_after_a_while() {
        let running = start(
            [
                ok(r#"{"token":"tok"}"#),
                api_error(14),
                api_error(14),
                api_error(14),
            ],
            Plays::default(),
            Timing {
                poll: Duration::from_millis(30),
                give_up: Duration::from_millis(50),
                ..QUICK
            },
        );
        running.scrobbler.connect();
        assert!(matches!(running.next(), Report::Authorize(_)));
        assert_eq!(
            running.next(),
            Report::ConnectFailed(ConnectError::TimedOut)
        );

        let running = start(
            [ok(r#"{"token":"tok"}"#), api_error(15)],
            Plays::default(),
            QUICK,
        );
        running.scrobbler.connect();
        assert!(matches!(running.next(), Report::Authorize(_)));
        assert_eq!(
            running.next(),
            Report::ConnectFailed(ConnectError::TimedOut)
        );
    }

    #[test]
    fn a_refused_key_fails_connecting() {
        let running = start([api_error(10)], Plays::default(), QUICK);
        running.scrobbler.connect();
        assert!(matches!(
            running.next(),
            Report::ConnectFailed(ConnectError::Failed(Error::Api { code: 10, .. }))
        ));
    }

    #[test]
    fn the_queue_drains_in_batches_of_fifty() {
        let plays = Plays::with(120);
        let running = start(
            [accepted(50), accepted(50), accepted(20)],
            plays.clone(),
            QUICK,
        );
        running.store_key();
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::Sent(50));
        assert_eq!(running.next(), Report::Sent(50));
        assert_eq!(running.next(), Report::Sent(20));
        assert!(plays.titles().is_empty());
        let requests = running.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(value(&requests[0], "sk"), Some("sk-1"));
        assert_eq!(value(&requests[0], "track[0]"), Some("Track 0"));
        assert_eq!(value(&requests[0], "track[49]"), Some("Track 49"));
        assert_eq!(value(&requests[0], "track[50]"), None);
        assert_eq!(value(&requests[2], "track[0]"), Some("Track 100"));
        assert_eq!(value(&requests[2], "track[19]"), Some("Track 119"));
    }

    #[test]
    fn plays_left_offline_go_out_after_a_restart() {
        let plays = Plays::with(3);
        let offline = start(
            [Err("offline".into())],
            plays.clone(),
            Timing {
                backoff: Duration::from_secs(60),
                ..QUICK
            },
        );
        offline.store_key();
        offline.scrobbler.resume("listener");
        offline.requested(1);
        drop(offline);
        assert_eq!(plays.titles().len(), 3, "nothing was lost");

        let online = start([accepted(3)], plays.clone(), QUICK);
        online.store_key();
        online.scrobbler.resume("listener");
        assert_eq!(online.next(), Report::Sent(3));
        assert!(plays.titles().is_empty());
    }

    #[test]
    fn failures_wait_longer_each_time() {
        let plays = Plays::with(1);
        let running = start(
            [
                Err("offline".into()),
                Err("offline".into()),
                api_error(16),
                accepted(1),
            ],
            plays.clone(),
            Timing {
                backoff: Duration::from_millis(40),
                max_backoff: Duration::from_millis(100),
                ..QUICK
            },
        );
        running.store_key();
        let begun = Instant::now();
        running.scrobbler.resume("listener");
        let (report, at) = running.next_at();
        assert_eq!(report, Report::Sent(1));
        // 40 ms, then 80 ms, then 100 ms (the cap) between the four tries.
        assert!(at - begun >= Duration::from_millis(220), "{:?}", at - begun);
        assert!(plays.titles().is_empty());
    }

    #[test]
    fn now_playing_that_goes_through_ends_the_wait() {
        let plays = Plays::with(1);
        let running = start(
            [
                Err("offline".into()),
                ok(r#"{"nowplaying":{}}"#),
                accepted(1),
            ],
            plays.clone(),
            Timing {
                backoff: Duration::from_secs(60),
                ..QUICK
            },
        );
        running.store_key();
        running.scrobbler.resume("listener");
        running.requested(1);
        running.scrobbler.now_playing(track("Opening"));
        assert_eq!(running.next(), Report::Sent(1));
        assert_eq!(
            running.methods(),
            ["track.scrobble", "track.updateNowPlaying", "track.scrobble"]
        );
    }

    #[test]
    fn a_play_last_fm_refuses_is_found_and_dropped() {
        let plays = Plays::with(3);
        let running = start(
            [api_error(6), accepted(1), api_error(6), accepted(1)],
            plays.clone(),
            QUICK,
        );
        running.store_key();
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::Sent(1));
        assert_eq!(running.next(), Report::Sent(1), "the refused play left");
        assert_eq!(running.next(), Report::Sent(1));
        assert!(plays.titles().is_empty());
        let requests = running.requests.lock().unwrap();
        assert_eq!(value(&requests[0], "track[2]"), Some("Track 2"));
        assert_eq!(value(&requests[1], "track[1]"), None, "one at a time");
        assert_eq!(value(&requests[3], "track[0]"), Some("Track 2"));
    }

    #[test]
    fn a_revoked_key_is_forgotten_and_the_queue_kept() {
        let plays = Plays::with(2);
        let running = start([api_error(9)], plays.clone(), QUICK);
        running.store_key();
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::Rejected);
        assert!(running.sessions.0.lock().unwrap().is_empty());
        assert_eq!(plays.titles().len(), 2);
        // Without an account nothing more is sent.
        running.scrobbler.flush();
        running.scrobbler.now_playing(track("Opening"));
        running.settle();
        assert_eq!(running.methods(), ["track.scrobble"]);
    }

    #[test]
    fn disconnecting_forgets_the_key_and_the_queue() {
        let plays = Plays::default();
        let running = start([ok(r#"{"nowplaying":{}}"#)], plays.clone(), QUICK);
        running.store_key();
        running.scrobbler.resume("listener");
        running.scrobbler.now_playing(track("Opening"));
        running.requested(1);
        *plays.0.lock().unwrap() = Plays::with(2).0.lock().unwrap().clone();
        running.scrobbler.disconnect();
        running.scrobbler.flush();
        running.settle();
        assert!(running.sessions.0.lock().unwrap().is_empty());
        assert!(plays.titles().is_empty());
        assert_eq!(running.methods(), ["track.updateNowPlaying"]);
    }

    #[test]
    fn a_missing_key_is_reported() {
        let running = start([], Plays::with(1), QUICK);
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::KeyMissing);
        assert!(running.methods().is_empty());
    }

    #[test]
    fn plays_carry_when_they_started() {
        let plays = Plays::default();
        plays.0.lock().unwrap().push((
            7,
            Scrobble {
                track: track("Opening"),
                started: SystemTime::UNIX_EPOCH + Duration::from_secs(1_234_567_890),
            },
        ));
        let running = start([accepted(1)], plays, QUICK);
        running.store_key();
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::Sent(1));
        let requests = running.requests.lock().unwrap();
        assert_eq!(value(&requests[0], "timestamp[0]"), Some("1234567890"));
    }

    fn loved_page(page: u32, pages: u32, titles: &[&str]) -> Result<(u16, String), String> {
        let tracks: Vec<String> = titles
            .iter()
            .map(|title| {
                format!(r#"{{"name":"{title}","artist":{{"name":"Band"}},"date":{{"uts":"100"}}}}"#)
            })
            .collect();
        ok(&format!(
            r#"{{"lovedtracks":{{"track":[{}],"@attr":{{"page":"{page}","totalPages":"{pages}"}}}}}}"#,
            tracks.join(",")
        ))
    }

    #[test]
    fn loves_go_out_one_by_one_after_the_plays() {
        let plays = Plays::with(1);
        plays.love("Opening", true);
        plays.love("Closing", false);
        let running = start([accepted(1), ok("{}"), ok("{}")], plays.clone(), QUICK);
        running.store_key();
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::Sent(1));
        assert_eq!(running.next(), Report::Loved(1));
        assert_eq!(running.next(), Report::Loved(1));
        assert_eq!(
            running.methods(),
            ["track.scrobble", "track.love", "track.unlove"]
        );
        let requests = running.requests.lock().unwrap();
        assert_eq!(value(&requests[1], "track"), Some("Opening"));
        assert_eq!(value(&requests[2], "track"), Some("Closing"));
        assert_eq!(value(&requests[2], "sk"), Some("sk-1"));
        assert_eq!(plays.loves_left(), 0);
    }

    #[test]
    fn a_love_queued_later_is_sent_on_flush() {
        let plays = Plays::default();
        let running = start([ok("{}")], plays.clone(), QUICK);
        running.store_key();
        running.scrobbler.resume("listener");
        running.settle();
        plays.love("Opening", true);
        running.scrobbler.flush_loves();
        assert_eq!(running.next(), Report::Loved(1));
        assert_eq!(running.methods(), ["track.love"]);
    }

    #[test]
    fn a_love_last_fm_refuses_is_dropped_and_an_offline_one_waits() {
        let plays = Plays::default();
        plays.love("Refused", true);
        plays.love("Later", true);
        let running = start(
            [api_error(6), Err("offline".into()), ok("{}")],
            plays.clone(),
            QUICK,
        );
        running.store_key();
        let begun = Instant::now();
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::Loved(1));
        let (report, at) = running.next_at();
        assert_eq!(report, Report::Loved(1));
        assert!(at - begun >= QUICK.backoff, "{:?}", at - begun);
        assert_eq!(running.methods(), ["track.love"; 3]);
        assert_eq!(plays.loves_left(), 0);
    }

    #[test]
    fn a_revoked_key_keeps_the_loves() {
        let plays = Plays::default();
        plays.love("Opening", true);
        let running = start([api_error(9)], plays.clone(), QUICK);
        running.store_key();
        running.scrobbler.resume("listener");
        assert_eq!(running.next(), Report::Rejected);
        assert_eq!(plays.loves_left(), 1);
    }

    #[test]
    fn disconnecting_forgets_the_loves() {
        let plays = Plays::default();
        let running = start([], plays.clone(), QUICK);
        plays.love("Opening", true);
        running.scrobbler.disconnect();
        running.settle();
        assert_eq!(plays.loves_left(), 0);
    }

    #[test]
    fn an_import_reads_every_page_then_likes_the_matches() {
        let plays = Plays::default();
        let running = start(
            [
                loved_page(1, 2, &["One", "Two"]),
                loved_page(2, 2, &["Nowhere"]),
            ],
            plays.clone(),
            QUICK,
        );
        running.store_key();
        running.scrobbler.resume("listener");
        running.scrobbler.import_loved();
        assert_eq!(running.next(), Report::ImportProgress { page: 1, pages: 2 });
        assert_eq!(running.next(), Report::ImportProgress { page: 2, pages: 2 });
        assert_eq!(
            running.next(),
            Report::Imported(Imported {
                matched: 2,
                unmatched: 1
            })
        );
        assert_eq!(plays.imported(), ["One", "Two", "Nowhere"]);
        let requests = running.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for (index, form) in requests.iter().enumerate() {
            assert_eq!(value(form, "method"), Some("user.getLovedTracks"));
            assert_eq!(value(form, "user"), Some("listener"));
            assert_eq!(value(form, "page"), Some((index + 1).to_string().as_str()));
        }
    }

    #[test]
    fn an_empty_loved_list_imports_nothing() {
        let plays = Plays::default();
        let running = start([loved_page(1, 0, &[])], plays.clone(), QUICK);
        running.store_key();
        running.scrobbler.resume("listener");
        running.scrobbler.import_loved();
        assert_eq!(running.next(), Report::ImportProgress { page: 1, pages: 1 });
        assert_eq!(running.next(), Report::Imported(Imported::default()));
    }

    #[test]
    fn an_import_needs_an_account() {
        let running = start([], Plays::default(), QUICK);
        running.scrobbler.import_loved();
        assert_eq!(running.next(), Report::ImportFailed(ImportError::NoAccount));
    }

    #[test]
    fn an_import_tries_a_busy_page_again_then_gives_up() {
        let plays = Plays::default();
        let running = start(
            [
                Err("offline".into()),
                loved_page(1, 2, &["One"]),
                api_error(29),
                api_error(29),
                api_error(29),
            ],
            plays.clone(),
            QUICK,
        );
        running.store_key();
        running.scrobbler.resume("listener");
        running.scrobbler.import_loved();
        assert_eq!(running.next(), Report::ImportProgress { page: 1, pages: 2 });
        assert!(matches!(
            running.next(),
            Report::ImportFailed(ImportError::Lastfm(Error::Api { code: 29, .. }))
        ));
        assert!(plays.imported().is_empty(), "nothing is liked from a part");
        assert_eq!(running.methods(), ["user.getLovedTracks"; 5]);
    }

    #[test]
    fn loves_go_out_while_an_import_waits_for_a_page() {
        let plays = Plays::default();
        let running = start(
            [
                Err("offline".into()),
                ok("{}"),
                loved_page(1, 2, &["One"]),
                loved_page(2, 2, &["Two"]),
            ],
            plays.clone(),
            Timing {
                backoff: Duration::from_millis(200),
                ..QUICK
            },
        );
        running.store_key();
        running.scrobbler.resume("listener");
        running.scrobbler.import_loved();
        running.requested(1);
        plays.love("Opening", true);
        running.scrobbler.flush_loves();
        assert_eq!(running.next(), Report::Loved(1));
        assert_eq!(running.next(), Report::ImportProgress { page: 1, pages: 2 });
        assert_eq!(running.next(), Report::ImportProgress { page: 2, pages: 2 });
        assert!(matches!(running.next(), Report::Imported(_)));
        assert_eq!(
            running.methods(),
            [
                "user.getLovedTracks",
                "track.love",
                "user.getLovedTracks",
                "user.getLovedTracks"
            ]
        );
    }
}
