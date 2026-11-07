//! Play history: every play that counted, with when it started and the context it
//! played from. Counts and last played are worked out from the log through its
//! (track, time) index; nothing is cached on the tracks.
//!
//! A play points at a track, and tracks are never deleted, so history follows a
//! moved file and stays with a track whose file is gone, like a playlist entry.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{OptionalExtension, params};

use crate::query::time_from_ms;
use crate::{Library, Result, TrackId};

/// How often a track was played, and when it last was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayStats {
    pub count: u32,
    /// When the latest play started.
    pub last: SystemTime,
}

impl Library {
    /// Records one play of `track` that started at `started`, from the context the
    /// app calls `origin` ("album:12", "playlist:3", "search", ...), or from none.
    pub fn record_play(
        &self,
        track: TrackId,
        started: SystemTime,
        origin: Option<&str>,
    ) -> Result<()> {
        self.conn
            .prepare_cached("INSERT INTO plays (track_id, played_ms, origin) VALUES (?1, ?2, ?3)")?
            .execute(params![track.0, ms(started), origin])?;
        Ok(())
    }

    /// Every played track's count and last play, of the plays that started at
    /// `since` or later, or of all of them.
    pub fn play_stats(&self, since: Option<SystemTime>) -> Result<HashMap<TrackId, PlayStats>> {
        let mut statement = self.conn.prepare(
            "SELECT track_id, COUNT(*), MAX(played_ms) FROM plays
             WHERE played_ms >= ?1 GROUP BY track_id",
        )?;
        let stats = statement
            .query_map([since.map_or(i64::MIN, ms)], |row| {
                Ok((
                    TrackId(row.get(0)?),
                    PlayStats {
                        count: row.get(1)?,
                        last: time_from_ms(row.get(2)?),
                    },
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(stats)
    }

    /// One track's count and last play, or `None` if it was never played.
    pub fn track_play_stats(&self, track: TrackId) -> Result<Option<PlayStats>> {
        Ok(self
            .conn
            .prepare_cached(
                "SELECT COUNT(*), MAX(played_ms) FROM plays WHERE track_id = ?1
                 HAVING COUNT(*) > 0",
            )?
            .query_row([track.0], |row| {
                Ok(PlayStats {
                    count: row.get(0)?,
                    last: time_from_ms(row.get(1)?),
                })
            })
            .optional()?)
    }

    /// Deletes every play. Scrobbles waiting for Last.fm, added dates and
    /// favourites stay. Returns how many plays were deleted.
    pub fn clear_play_history(&self) -> Result<usize> {
        Ok(self.conn.execute("DELETE FROM plays", [])?)
    }
}

fn ms(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}
