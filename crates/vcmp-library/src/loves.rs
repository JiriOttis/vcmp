//! Loves and unloves waiting to be sent to Last.fm. They stay in the database until
//! Last.fm has taken them, so they survive a restart and time offline. A track has at
//! most one entry: queueing it again replaces the one waiting.

use rusqlite::params_from_iter;

use crate::{Library, Result};

/// A love (`love`) or unlove of a track, with its artist and title as they were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Love {
    pub artist: String,
    pub title: String,
    pub love: bool,
}

/// A love in the queue, with the id [`Library::remove_loves`] takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedLove {
    pub id: i64,
    pub love: Love,
}

impl Library {
    /// Adds loves to the end of the queue, each replacing the one waiting for the
    /// same artist and title.
    pub fn queue_loves(&self, loves: &[Love]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for love in loves {
            self.conn
                .prepare_cached(
                    "INSERT OR REPLACE INTO love_queue (artist, title, love) VALUES (?1, ?2, ?3)",
                )?
                .execute(rusqlite::params![love.artist, love.title, love.love])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The oldest `limit` loves in the queue, oldest first.
    pub fn queued_loves(&self, limit: usize) -> Result<Vec<QueuedLove>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, artist, title, love FROM love_queue ORDER BY id LIMIT ?1")?;
        let queued = statement
            .query_map([limit as i64], |row| {
                Ok(QueuedLove {
                    id: row.get(0)?,
                    love: Love {
                        artist: row.get(1)?,
                        title: row.get(2)?,
                        love: row.get(3)?,
                    },
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(queued)
    }

    /// How many loves and unloves wait to be sent.
    pub fn queued_love_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM love_queue", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Takes loves out of the queue, once Last.fm has them. Unknown ids are skipped.
    pub fn remove_loves(&self, ids: &[i64]) -> Result<()> {
        for chunk in ids.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(", ");
            self.conn.execute(
                &format!("DELETE FROM love_queue WHERE id IN ({marks})"),
                params_from_iter(chunk),
            )?;
        }
        Ok(())
    }

    /// Empties the queue.
    pub fn clear_loves(&self) -> Result<()> {
        self.conn.execute("DELETE FROM love_queue", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(dir: &std::path::Path) -> Library {
        Library::open(&dir.join("library.sqlite3"), &dir.join("cache")).unwrap()
    }

    fn love(title: &str, love: bool) -> Love {
        Love {
            artist: "Band".into(),
            title: title.into(),
            love,
        }
    }

    fn waiting(library: &Library) -> Vec<Love> {
        library
            .queued_loves(50)
            .unwrap()
            .into_iter()
            .map(|queued| queued.love)
            .collect()
    }

    #[test]
    fn loves_survive_reopening_the_database() {
        let dir = tempfile::tempdir().unwrap();
        open(dir.path())
            .queue_loves(&[love("Opening", true), love("Closing", false)])
            .unwrap();
        let library = open(dir.path());
        assert_eq!(
            waiting(&library),
            [love("Opening", true), love("Closing", false)]
        );
        assert_eq!(library.queued_love_count().unwrap(), 2);
    }

    #[test]
    fn a_newer_love_replaces_the_one_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let library = open(dir.path());
        library
            .queue_loves(&[love("Opening", true), love("Closing", true)])
            .unwrap();
        let sending = library.queued_loves(1).unwrap()[0].id;
        library.queue_loves(&[love("Opening", false)]).unwrap();
        assert_eq!(
            waiting(&library),
            [love("Closing", true), love("Opening", false)],
            "the unlove replaced the love and went to the end"
        );
        // The love that was being sent as it was replaced leaves nothing out.
        library.remove_loves(&[sending]).unwrap();
        assert_eq!(library.queued_love_count().unwrap(), 2);

        library.clear_loves().unwrap();
        assert!(waiting(&library).is_empty());
    }
}
