//! Lyrics from LRCLIB (lrclib.net), a free database of plain and synced lyrics that
//! needs no key. Only its exact lookup is used: title, artist, album and duration,
//! which LRCLIB matches within two seconds. There is no search and no fuzzy
//! matching, because lyrics of another song or version are worse than none.
//!
//! Lookups run one at a time on a worker thread, spaced as LRCLIB's documentation
//! asks, and carry VCMP's User-Agent. A lookup asked for while another runs replaces
//! any that waits, since only the track playing now matters.

use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::musicbrainz::USER_AGENT;

const GET_URL: &str = "https://lrclib.net/api/get";
/// LRCLIB asks for sequential requests a few hundred milliseconds apart.
const INTERVAL: Duration = Duration::from_millis(300);
/// LRCLIB takes durations from 1 second to an hour.
const LONGEST: Duration = Duration::from_secs(3600);

/// The track to look up, as its tags name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricsQuery {
    pub title: String,
    pub artist: String,
    /// Left out of the request when the track has none.
    pub album: Option<String>,
    pub duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// LRC text, plain text, or both.
    Found {
        synced: Option<String>,
        plain: Option<String>,
    },
    /// LRCLIB has no lyrics for the track, or knows it as instrumental.
    NotFound,
    /// LRCLIB could not be asked (offline, an error, too many requests); worth
    /// trying later, not before `retry_after` when it said so.
    Failed {
        error: String,
        retry_after: Option<Duration>,
    },
}

/// What came back for a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
    /// The Retry-After header, in seconds, when there was one.
    pub retry_after: Option<Duration>,
}

/// The HTTP side of a lookup, replaced in tests.
pub trait Transport: Send + 'static {
    /// GETs `url` with `query` parameters.
    fn get(&mut self, url: &str, query: &[(&str, &str)]) -> Result<Response, String>;
}

/// The real network.
pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(15)))
            .build()
            .into();
        Self { agent }
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for Http {
    fn get(&mut self, url: &str, query: &[(&str, &str)]) -> Result<Response, String> {
        let mut request = self.agent.get(url).header("Accept", "application/json");
        for (key, value) in query {
            request = request.query(*key, *value);
        }
        let mut response = request.call().map_err(|error| error.to_string())?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("Retry-After")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        Ok(Response {
            status,
            body,
            retry_after,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default)]
    instrumental: bool,
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
}

/// Asks LRCLIB for `query` through `transport`.
pub fn look_up<T: Transport>(transport: &mut T, query: &LyricsQuery) -> Answer {
    let seconds = query.duration.as_secs_f64().round() as u64;
    let duration = seconds.to_string();
    let mut params = vec![
        ("track_name", query.title.as_str()),
        ("artist_name", query.artist.as_str()),
    ];
    if let Some(album) = &query.album {
        params.push(("album_name", album.as_str()));
    }
    if (1..=LONGEST.as_secs()).contains(&seconds) {
        params.push(("duration", duration.as_str()));
    }
    let response = match transport.get(GET_URL, &params) {
        Ok(response) => response,
        Err(error) => {
            return Answer::Failed {
                error,
                retry_after: None,
            };
        }
    };
    match response.status {
        200 => match serde_json::from_str::<Record>(&response.body) {
            Ok(record) => {
                let text = |text: Option<String>| text.filter(|text| !text.trim().is_empty());
                let synced = text(record.synced_lyrics);
                let plain = text(record.plain_lyrics);
                if record.instrumental || (synced.is_none() && plain.is_none()) {
                    Answer::NotFound
                } else {
                    Answer::Found { synced, plain }
                }
            }
            Err(error) => Answer::Failed {
                error: format!("LRCLIB's answer could not be read: {error}"),
                retry_after: None,
            },
        },
        404 => Answer::NotFound,
        status => Answer::Failed {
            error: format!("LRCLIB answered {status}"),
            retry_after: response.retry_after,
        },
    }
}

/// Looks lyrics up on a worker thread and hands each answer, with its key and the
/// question asked, to `on_result` (on the worker thread). Of the lookups asked for
/// while one runs, only the latest is made. Dropping the finder lets the worker
/// finish its current lookup and end.
pub struct LyricsFinder<K> {
    jobs: Sender<(K, LyricsQuery)>,
}

impl<K: Send + 'static> LyricsFinder<K> {
    pub fn start(on_result: impl Fn(K, LyricsQuery, Answer) + Send + 'static) -> Self {
        Self::with_transport(Http::new(), INTERVAL, on_result)
    }

    pub fn with_transport<T: Transport>(
        mut transport: T,
        interval: Duration,
        on_result: impl Fn(K, LyricsQuery, Answer) + Send + 'static,
    ) -> Self {
        let (jobs, inbox) = mpsc::channel::<(K, LyricsQuery)>();
        thread::Builder::new()
            .name("vcmp-lyrics-lookup".into())
            .spawn(move || {
                let mut last_request: Option<Instant> = None;
                while let Ok(mut job) = inbox.recv() {
                    while let Ok(later) = inbox.try_recv() {
                        job = later;
                    }
                    if let Some(wait) = last_request
                        .map(|at| interval.saturating_sub(at.elapsed()))
                        .filter(|wait| !wait.is_zero())
                    {
                        thread::sleep(wait);
                        while let Ok(later) = inbox.try_recv() {
                            job = later;
                        }
                    }
                    let (key, query) = job;
                    last_request = Some(Instant::now());
                    let answer = look_up(&mut transport, &query);
                    on_result(key, query, answer);
                }
            })
            .expect("spawning the lyrics lookup thread");
        Self { jobs }
    }

    pub fn find(&self, key: K, query: LyricsQuery) {
        let _ = self.jobs.send((key, query));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A request's query parameters.
    type Params = Vec<(String, String)>;

    /// Answers every request with the next response, recording what was asked.
    #[derive(Clone, Default)]
    struct Fake {
        asked: Arc<Mutex<Vec<Params>>>,
        responses: Arc<Mutex<Vec<Result<Response, String>>>>,
    }

    impl Transport for Fake {
        fn get(&mut self, url: &str, query: &[(&str, &str)]) -> Result<Response, String> {
            assert_eq!(url, GET_URL);
            self.asked.lock().unwrap().push(
                query
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            );
            self.responses.lock().unwrap().remove(0)
        }
    }

    fn ok(status: u16, body: &str) -> Result<Response, String> {
        Ok(Response {
            status,
            body: body.into(),
            retry_after: None,
        })
    }

    fn query() -> LyricsQuery {
        LyricsQuery {
            title: "Song".into(),
            artist: "Band".into(),
            album: Some("Record".into()),
            duration: Duration::from_millis(181_600),
        }
    }

    fn answer(response: Result<Response, String>) -> Answer {
        let mut fake = Fake::default();
        fake.responses.lock().unwrap().push(response);
        look_up(&mut fake, &query())
    }

    #[test]
    fn asks_for_the_exact_track() {
        let mut fake = Fake::default();
        fake.responses.lock().unwrap().push(ok(404, "{}"));
        look_up(&mut fake, &query());
        let without_album = LyricsQuery {
            album: None,
            duration: Duration::ZERO,
            ..query()
        };
        fake.responses.lock().unwrap().push(ok(404, "{}"));
        look_up(&mut fake, &without_album);
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect()
        };
        assert_eq!(
            *fake.asked.lock().unwrap(),
            [
                pairs(&[
                    ("track_name", "Song"),
                    ("artist_name", "Band"),
                    ("album_name", "Record"),
                    ("duration", "182"),
                ]),
                pairs(&[("track_name", "Song"), ("artist_name", "Band")]),
            ]
        );
    }

    #[test]
    fn reads_found_missing_and_instrumental_tracks() {
        assert_eq!(
            answer(ok(
                200,
                r#"{"id":1,"instrumental":false,"plainLyrics":"Hi","syncedLyrics":"[00:01.00]Hi"}"#
            )),
            Answer::Found {
                synced: Some("[00:01.00]Hi".into()),
                plain: Some("Hi".into()),
            }
        );
        assert_eq!(
            answer(ok(200, r#"{"plainLyrics":"Hi","syncedLyrics":null}"#)),
            Answer::Found {
                synced: None,
                plain: Some("Hi".into()),
            }
        );
        assert_eq!(
            answer(ok(
                200,
                r#"{"instrumental":true,"plainLyrics":null,"syncedLyrics":null}"#
            )),
            Answer::NotFound
        );
        assert_eq!(
            answer(ok(404, r#"{"name":"TrackNotFound"}"#)),
            Answer::NotFound
        );
    }

    #[test]
    fn failures_say_when_to_try_again() {
        assert_eq!(
            answer(Ok(Response {
                status: 429,
                body: String::new(),
                retry_after: Some(Duration::from_secs(30)),
            })),
            Answer::Failed {
                error: "LRCLIB answered 429".into(),
                retry_after: Some(Duration::from_secs(30)),
            }
        );
        assert!(matches!(
            answer(Err("offline".into())),
            Answer::Failed {
                retry_after: None,
                ..
            }
        ));
        assert!(matches!(answer(ok(200, "not json")), Answer::Failed { .. }));
    }

    #[test]
    fn the_finder_answers_on_its_thread() {
        let fake = Fake::default();
        fake.responses.lock().unwrap().push(ok(404, "{}"));
        let (sender, results) = mpsc::channel();
        let finder =
            LyricsFinder::with_transport(fake, Duration::ZERO, move |key, query, answer| {
                sender.send((key, query, answer)).unwrap();
            });
        finder.find(1, query());
        assert_eq!(
            results.recv_timeout(Duration::from_secs(5)).unwrap(),
            (1, query(), Answer::NotFound)
        );
    }
}
