//! Loves: telling Last.fm which tracks the user loves, and reading the tracks an
//! account loved there so the library can like the same ones.
//!
//! Last.fm knows a track by its artist and title only. The import compares them with
//! case, accents and spacing ignored ([`match_key`]), so "Beyoncé" in the library
//! matches "beyonce" on Last.fm, and likes every library track that matches.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Deserializer};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use super::{API_URL, Client, Error, Transport, count, parse};

/// Loved tracks asked for per `user.getLovedTracks` page.
pub const LOVED_PAGE_SIZE: u32 = 200;

/// A love (`love`) or unlove waiting to be sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Love {
    pub artist: String,
    pub title: String,
    pub love: bool,
}

/// A track the account loved on Last.fm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loved {
    pub artist: String,
    pub title: String,
    /// When it was loved; `None` when Last.fm leaves the date out.
    pub at: Option<SystemTime>,
}

/// One page of an account's loved tracks, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LovedPage {
    pub tracks: Vec<Loved>,
    /// This page, from 1.
    pub page: u32,
    /// How many pages there are; at least 1, even with nothing loved.
    pub pages: u32,
}

/// How an import went: loved tracks that matched at least one library track, and
/// those that matched none.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub matched: usize,
    pub unmatched: usize,
}

impl<T: Transport> Client<T> {
    /// Loves the track on the user's profile.
    pub fn love(&mut self, session_key: &str, artist: &str, title: &str) -> Result<(), Error> {
        self.love_call("track.love", session_key, artist, title)
    }

    /// Takes the track out of the user's loved tracks.
    pub fn unlove(&mut self, session_key: &str, artist: &str, title: &str) -> Result<(), Error> {
        self.love_call("track.unlove", session_key, artist, title)
    }

    fn love_call(
        &mut self,
        method: &str,
        session_key: &str,
        artist: &str,
        title: &str,
    ) -> Result<(), Error> {
        let params = vec![
            ("artist".into(), artist.into()),
            ("track".into(), title.into()),
        ];
        let (status, body) = self.signed_post(method, params, Some(session_key))?;
        // Last.fm answers an empty object, which some of its servers send as nothing.
        if status == 200 && body.trim().is_empty() {
            return Ok(());
        }
        let _: serde::de::IgnoredAny = parse(status, &body)?;
        Ok(())
    }

    /// One page of the tracks `user` loved, newest first. Anyone's loved tracks can
    /// be read, so this needs no session key.
    pub fn loved_tracks(&mut self, user: &str, page: u32) -> Result<LovedPage, Error> {
        let query = vec![
            ("method".to_string(), "user.getLovedTracks".to_string()),
            ("user".to_string(), user.to_string()),
            ("page".to_string(), page.to_string()),
            ("limit".to_string(), LOVED_PAGE_SIZE.to_string()),
            ("api_key".to_string(), self.credentials.api_key.clone()),
            ("format".to_string(), "json".to_string()),
        ];
        let (status, body) = self
            .transport
            .get(API_URL, &query)
            .map_err(Error::Network)?;
        let reply: LovedReply = parse(status, &body)?;
        let list = reply.lovedtracks;
        Ok(LovedPage {
            tracks: list
                .track
                .into_iter()
                .map(|entry| Loved {
                    artist: entry.artist.name,
                    title: entry.name,
                    at: entry
                        .date
                        .map(|date| UNIX_EPOCH + Duration::from_secs(date.uts)),
                })
                .collect(),
            page: list.attr.page.max(1),
            pages: list.attr.total_pages.max(list.attr.page).max(1),
        })
    }
}

/// Artist and title as the import compares them: compatibility-decomposed, without
/// accents and other combining marks, lowercased, with runs of spaces made one.
pub fn match_key(artist: &str, title: &str) -> (String, String) {
    (fold(artist), fold(title))
}

fn fold(text: &str) -> String {
    let folded: String = text
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Which library tracks the loved tracks match. `tracks` are the library's tracks as
/// (id, artist, title); a loved track likes every one that matches, with its love
/// date. A track loved twice under different spellings is listed with its newest
/// date first, as Last.fm sends the newest first.
pub fn match_loved<K, S>(
    loved: &[Loved],
    tracks: impl IntoIterator<Item = (K, S, S)>,
) -> (Vec<(K, Option<SystemTime>)>, Imported)
where
    K: Copy,
    S: AsRef<str>,
{
    let mut by_key: HashMap<(String, String), Vec<K>> = HashMap::new();
    for (id, artist, title) in tracks {
        by_key
            .entry(match_key(artist.as_ref(), title.as_ref()))
            .or_default()
            .push(id);
    }
    let mut likes = Vec::new();
    let mut imported = Imported::default();
    for track in loved {
        match by_key.get(&match_key(&track.artist, &track.title)) {
            Some(ids) => {
                imported.matched += 1;
                likes.extend(ids.iter().map(|id| (*id, track.at)));
            }
            None => imported.unmatched += 1,
        }
    }
    (likes, imported)
}

#[derive(Deserialize)]
struct LovedReply {
    lovedtracks: LovedList,
}

#[derive(Deserialize)]
struct LovedList {
    #[serde(default, deserialize_with = "one_or_many")]
    track: Vec<LovedEntry>,
    #[serde(rename = "@attr")]
    attr: LovedAttr,
}

#[derive(Deserialize)]
struct LovedEntry {
    name: String,
    artist: LovedArtist,
    #[serde(default)]
    date: Option<LovedDate>,
}

#[derive(Deserialize)]
struct LovedArtist {
    name: String,
}

#[derive(Deserialize)]
struct LovedDate {
    #[serde(deserialize_with = "count")]
    uts: u64,
}

#[derive(Deserialize)]
struct LovedAttr {
    #[serde(deserialize_with = "count")]
    page: u32,
    #[serde(rename = "totalPages", deserialize_with = "count")]
    total_pages: u32,
}

/// A list, which Last.fm's JSON gives as a lone object when it holds one item.
fn one_or_many<'de, D, V>(deserializer: D) -> Result<Vec<V>, D::Error>
where
    D: Deserializer<'de>,
    V: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<V> {
        Many(Vec<V>),
        One(V),
    }
    Ok(match OneOrMany::<V>::deserialize(deserializer)? {
        OneOrMany::Many(items) => items,
        OneOrMany::One(item) => vec![item],
    })
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Scripted, api_error, credentials, ok, value};
    use super::*;

    fn client(
        answers: impl IntoIterator<Item = Result<(u16, String), String>>,
    ) -> (Client<Scripted>, super::super::tests::Requests) {
        let (transport, requests) = Scripted::new(answers);
        (Client::new(transport, credentials()), requests)
    }

    fn loved(artist: &str, title: &str, at: Option<u64>) -> Loved {
        Loved {
            artist: artist.into(),
            title: title.into(),
            at: at.map(|secs| UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    #[test]
    fn love_and_unlove_are_signed_and_name_the_track() {
        let (mut client, requests) = client([ok("{}"), ok("")]);
        client.love("sk-1", "Band", "Opening").unwrap();
        client.unlove("sk-1", "Band", "Opening").unwrap();
        let requests = requests.lock().unwrap();
        assert_eq!(value(&requests[0], "method"), Some("track.love"));
        assert_eq!(value(&requests[1], "method"), Some("track.unlove"));
        for form in requests.iter() {
            assert_eq!(value(form, "sk"), Some("sk-1"));
            assert_eq!(value(form, "artist"), Some("Band"));
            assert_eq!(value(form, "track"), Some("Opening"));
            assert!(value(form, "api_sig").is_some());
            assert_eq!(value(form, "get"), None, "a POST");
        }
    }

    #[test]
    fn a_refused_love_says_so() {
        let (mut client, _) = client([api_error(6)]);
        assert!(matches!(
            client.love("sk", "Band", "A").unwrap_err(),
            Error::Api { code: 6, .. }
        ));
    }

    #[test]
    fn loved_tracks_are_read_with_a_get_page_by_page() {
        let (mut client, requests) = client([ok(r##"{"lovedtracks":{"track":[
                {"name":"Opening","artist":{"name":"Band","mbid":"","url":""},
                 "date":{"uts":"1700000000","#text":"14 Nov 2023, 22:13"}},
                {"name":"Closing","artist":{"name":"Other","mbid":"","url":""}}
            ],"@attr":{"user":"listener","totalPages":"3","page":"2","perPage":"200","total":"402"}}}"##)]);
        let page = client.loved_tracks("listener", 2).unwrap();
        assert_eq!(
            page,
            LovedPage {
                tracks: vec![
                    loved("Band", "Opening", Some(1_700_000_000)),
                    loved("Other", "Closing", None),
                ],
                page: 2,
                pages: 3,
            }
        );
        let form = requests.lock().unwrap()[0].clone();
        assert_eq!(value(&form, "get"), Some(""), "a GET");
        assert_eq!(value(&form, "method"), Some("user.getLovedTracks"));
        assert_eq!(value(&form, "user"), Some("listener"));
        assert_eq!(value(&form, "page"), Some("2"));
        assert_eq!(value(&form, "limit"), Some("200"));
        assert_eq!(value(&form, "api_key"), Some("the-key"));
        assert_eq!(value(&form, "sk"), None);
        assert_eq!(value(&form, "api_sig"), None);
    }

    #[test]
    fn one_loved_track_or_none_still_reads() {
        let (mut client, _) = client([
            ok(
                r#"{"lovedtracks":{"track":{"name":"Only","artist":{"name":"Band"},"date":{"uts":5}},
                "@attr":{"page":1,"totalPages":1}}}"#,
            ),
            ok(r#"{"lovedtracks":{"track":[],"@attr":{"page":"1","totalPages":"0","total":"0"}}}"#),
            ok(r#"{"lovedtracks":{"@attr":{"page":"1","totalPages":"0"}}}"#),
        ]);
        let one = client.loved_tracks("listener", 1).unwrap();
        assert_eq!(one.tracks, [loved("Band", "Only", Some(5))]);
        for _ in 0..2 {
            let none = client.loved_tracks("listener", 1).unwrap();
            assert!(none.tracks.is_empty());
            assert_eq!((none.page, none.pages), (1, 1));
        }
    }

    #[test]
    fn an_unknown_user_is_an_api_error() {
        let (mut client, _) = client([api_error(6)]);
        assert!(matches!(
            client.loved_tracks("nobody", 1).unwrap_err(),
            Error::Api { code: 6, .. }
        ));
    }

    #[test]
    fn matching_ignores_case_accents_and_spacing() {
        assert_eq!(
            match_key("  Beyoncé ", "Déjà  Vu"),
            match_key("BEYONCE", "deja vu")
        );
        assert_eq!(match_key("Ｍｕｓｅ", "Ⅱ"), match_key("muse", "ii"));
        assert_ne!(match_key("Band", "Opening"), match_key("Band", "Opening!"));
        assert_ne!(match_key("Band", "A"), match_key("Other", "A"));
    }

    #[test]
    fn every_matching_track_is_liked_and_the_rest_counted() {
        let tracks = [
            (1, "Band", "Opening"),
            (2, "band", "OPENING"),
            (3, "Band", "Closing"),
            (4, "Sigur Rós", "Hoppípolla"),
        ];
        let (likes, imported) = match_loved(
            &[
                loved("Band", "Opening", Some(10)),
                loved("Sigur Ros", "Hoppipolla", None),
                loved("Nobody", "Nothing", Some(1)),
            ],
            tracks,
        );
        let at = |secs| Some(UNIX_EPOCH + Duration::from_secs(secs));
        assert_eq!(likes, [(1, at(10)), (2, at(10)), (4, None)]);
        assert_eq!(
            imported,
            Imported {
                matched: 2,
                unmatched: 1
            }
        );
    }
}
