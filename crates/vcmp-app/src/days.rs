//! Calendar days in local time, for the Songs page's Last played and Added columns.
//! The rows carry the day and its date; `SongDates` in the UI turns them into
//! "Today", "3 days ago" or a date against the day `today` reports.

use std::time::SystemTime;

use chrono::{DateTime, Datelike, Local, NaiveDate};

use crate::DayStamp;

/// 1 January 1970, counted the way chrono counts days: from 1 January of year 1.
const UNIX_EPOCH_DAYS: i32 = 719_163;

/// Today, as a [`DayStamp::day`].
pub fn today() -> i32 {
    day_number(local_date(SystemTime::now()))
}

/// The local day of `time`, or the empty stamp for none.
pub fn stamp(time: Option<SystemTime>) -> DayStamp {
    let Some(time) = time else {
        return DayStamp {
            day: -1,
            ..DayStamp::default()
        };
    };
    let date = local_date(time);
    DayStamp {
        day: day_number(date),
        day_of_month: date.day() as i32,
        month: date.month() as i32,
        year: date.year(),
    }
}

fn local_date(time: SystemTime) -> NaiveDate {
    DateTime::<Local>::from(time).date_naive()
}

fn day_number(date: NaiveDate) -> i32 {
    date.num_days_from_ce() - UNIX_EPOCH_DAYS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn days_count_from_1970_in_local_time() {
        // Mid-January 1970, so no time zone moves it out of the month.
        let start = UNIX_EPOCH + Duration::from_secs(14 * 86_400 + 12 * 3600);
        let first = stamp(Some(start));
        assert!((13..=15).contains(&first.day), "{}", first.day);
        assert_eq!(first.day_of_month, first.day + 1);
        assert_eq!((first.month, first.year), (1, 1970));
        let later = stamp(Some(start + Duration::from_secs(10 * 86_400)));
        assert_eq!(later.day - first.day, 10);
        assert_eq!(stamp(None).day, -1);
        assert!(today() > 20_000, "after 2024");
    }
}
