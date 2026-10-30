//! Date taken (spec section 7, step 3): the fallback chain and the folder a
//! photo is filed under. Pure functions; readers for EXIF and QuickTime live
//! elsewhere and hand their results in as `Candidates`.
//!
//! Stored values: `Taken::utc` is the moment in UTC. `tz_offset_minutes` is
//! minutes east of UTC when known. When no offset is known (EXIF without an
//! offset tag, filenames) the wall-clock time is stored as if it were UTC and
//! the offset stays `None`, so the time shown is the time the camera showed.

use std::sync::OnceLock;

use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use regex::Regex;

/// 1990-01-01T00:00:00Z. Anything earlier is treated as a camera clock reset.
const MIN_PLAUSIBLE_TS: i64 = 631_152_000;
const DAY_SECS: i64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSource {
    Exif,
    QuickTime,
    Filename,
    FileModified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    pub utc: DateTime<Utc>,
    pub tz_offset_minutes: Option<i32>,
    pub source: DateSource,
    /// True only for the file-modified-time fallback (spec: `date_guessed`).
    pub guessed: bool,
}

/// Everything the importer could read about when a file was made.
pub struct Candidates<'a> {
    /// EXIF DateTimeOriginal as written (wall-clock, no zone).
    pub exif_local: Option<NaiveDateTime>,
    /// EXIF OffsetTimeOriginal in minutes east of UTC, if present.
    pub exif_offset_minutes: Option<i32>,
    /// QuickTime creation date (UTC by definition).
    pub quicktime_utc: Option<DateTime<Utc>>,
    pub filename: &'a str,
    pub modified: DateTime<Utc>,
    /// Passed in so the "not in the future" check is testable.
    pub now: DateTime<Utc>,
}

fn filename_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?:^|[^0-9])(?P<y>(?:19|20)\d{2})[-_.]?(?P<mo>0[1-9]|1[0-2])[-_.]?(?P<d>0[1-9]|[12]\d|3[01])(?:[-_. T]?(?P<h>[01]\d|2[0-3])[-_.:]?(?P<mi>[0-5]\d)[-_.:]?(?P<s>[0-5]\d))?",
        )
        .expect("filename date pattern is valid")
    })
}

/// A date (and time, when present) written in a file name, e.g.
/// `IMG_20240512_143022.jpg`, `PXL_20240512_143022123.jpg`,
/// `Screenshot_2024-05-12-14-30-22.png`, `IMG-20240512-WA0003.jpg`
/// (date only: time becomes 00:00:00). Impossible dates are skipped.
pub fn date_from_filename(name: &str) -> Option<NaiveDateTime> {
    for caps in filename_re().captures_iter(name) {
        let num = |k: &str| caps.name(k).and_then(|m| m.as_str().parse::<u32>().ok());
        let (Some(y), Some(mo), Some(d)) = (num("y"), num("mo"), num("d")) else {
            continue;
        };
        let Some(date) = NaiveDate::from_ymd_opt(y as i32, mo, d) else {
            continue;
        };
        let time = match (num("h"), num("mi"), num("s")) {
            (Some(h), Some(mi), Some(s)) => NaiveTime::from_hms_opt(h, mi, s),
            _ => None,
        }
        .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).expect("midnight exists"));
        return Some(date.and_time(time));
    }
    None
}

/// `dt` plus `minutes` (negative moves it earlier).
pub fn shift_utc_minutes(dt: DateTime<Utc>, minutes: i32) -> DateTime<Utc> {
    let secs = dt.timestamp() + i64::from(minutes) * 60;
    DateTime::from_timestamp(secs, dt.timestamp_subsec_nanos()).unwrap_or(dt)
}

fn shift_minutes(dt: DateTime<Utc>, minutes: i32) -> DateTime<Utc> {
    shift_utc_minutes(dt, minutes)
}

fn plausible(dt: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    let ts = dt.timestamp();
    ts >= MIN_PLAUSIBLE_TS && ts <= now.timestamp() + DAY_SECS
}

/// Spec order: EXIF (with offset when known), then QuickTime creation date,
/// then a date in the file name, then the file's modified time (guessed).
/// Dates before 1990 or more than a day in the future are skipped as junk.
pub fn choose_taken(c: &Candidates) -> Taken {
    if let Some(local) = c.exif_local {
        let wall = local.and_utc();
        let utc = match c.exif_offset_minutes {
            Some(m) => shift_minutes(wall, -m),
            None => wall,
        };
        if plausible(utc, c.now) {
            return Taken {
                utc,
                tz_offset_minutes: c.exif_offset_minutes,
                source: DateSource::Exif,
                guessed: false,
            };
        }
    }
    if let Some(q) = c.quicktime_utc {
        if plausible(q, c.now) {
            return Taken { utc: q, tz_offset_minutes: None, source: DateSource::QuickTime, guessed: false };
        }
    }
    if let Some(n) = date_from_filename(c.filename) {
        let utc = n.and_utc();
        if plausible(utc, c.now) {
            return Taken { utc, tz_offset_minutes: None, source: DateSource::Filename, guessed: false };
        }
    }
    Taken { utc: c.modified, tz_offset_minutes: None, source: DateSource::FileModified, guessed: true }
}

/// (year, month) for the `originals/YYYY/MM/` folder: the local wall-clock
/// date, so a photo taken at 00:30 on 1 May local time files under May.
pub fn folder_year_month(t: &Taken) -> (i32, u32) {
    let local = match t.tz_offset_minutes {
        Some(m) => shift_minutes(t.utc, m),
        None => t.utc,
    };
    (local.year(), local.month())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, s).unwrap()
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> DateTime<Utc> {
        naive(y, mo, d, h, mi, s).and_utc()
    }

    fn base<'a>() -> Candidates<'a> {
        Candidates {
            exif_local: None,
            exif_offset_minutes: None,
            quicktime_utc: None,
            filename: "IMG_1234.HEIC",
            modified: utc(2025, 1, 1, 12, 0, 0),
            now: utc(2026, 11, 2, 0, 0, 0),
        }
    }

    #[test]
    fn filename_patterns() {
        let want = Some(naive(2024, 5, 12, 14, 30, 22));
        assert_eq!(date_from_filename("IMG_20240512_143022.jpg"), want);
        assert_eq!(date_from_filename("VID_20240512_143022.mp4"), want);
        assert_eq!(date_from_filename("PXL_20240512_143022123.jpg"), want);
        assert_eq!(date_from_filename("Screenshot_2024-05-12-14-30-22.png"), want);
        assert_eq!(date_from_filename("2024-05-12 14.30.22.mov"), want);
        assert_eq!(date_from_filename("20240512143022.jpg"), want);
    }

    #[test]
    fn filename_date_only_is_midnight() {
        assert_eq!(date_from_filename("IMG-20240512-WA0003.jpg"), Some(naive(2024, 5, 12, 0, 0, 0)));
    }

    #[test]
    fn filename_without_a_date() {
        assert_eq!(date_from_filename("IMG_1234.HEIC"), None);
        assert_eq!(date_from_filename("DSC00123.ARW"), None);
        assert_eq!(date_from_filename("IMG_1720240512.jpg"), None);
    }

    #[test]
    fn filename_with_impossible_date() {
        assert_eq!(date_from_filename("IMG_20241340_000000.jpg"), None);
        assert_eq!(date_from_filename("IMG_20240230_120000.jpg"), None);
    }

    #[test]
    fn exif_with_offset_converts_to_utc() {
        let mut c = base();
        c.exif_local = Some(naive(2024, 5, 12, 14, 30, 22));
        c.exif_offset_minutes = Some(330); // +05:30
        let t = choose_taken(&c);
        assert_eq!(t.utc, utc(2024, 5, 12, 9, 0, 22));
        assert_eq!(t.tz_offset_minutes, Some(330));
        assert_eq!(t.source, DateSource::Exif);
        assert!(!t.guessed);
    }

    #[test]
    fn exif_without_offset_keeps_wall_clock() {
        let mut c = base();
        c.exif_local = Some(naive(2024, 5, 12, 14, 30, 22));
        let t = choose_taken(&c);
        assert_eq!(t.utc, utc(2024, 5, 12, 14, 30, 22));
        assert_eq!(t.tz_offset_minutes, None);
    }

    #[test]
    fn exif_beats_quicktime_beats_filename_beats_modified() {
        let mut c = base();
        c.filename = "IMG_20230101_000000.jpg";
        c.quicktime_utc = Some(utc(2023, 6, 1, 8, 0, 0));
        c.exif_local = Some(naive(2024, 5, 12, 14, 30, 22));
        assert_eq!(choose_taken(&c).source, DateSource::Exif);
        c.exif_local = None;
        assert_eq!(choose_taken(&c).source, DateSource::QuickTime);
        c.quicktime_utc = None;
        let t = choose_taken(&c);
        assert_eq!(t.source, DateSource::Filename);
        assert_eq!(t.utc, utc(2023, 1, 1, 0, 0, 0));
        assert!(!t.guessed);
        c.filename = "IMG_1234.HEIC";
        let t = choose_taken(&c);
        assert_eq!(t.source, DateSource::FileModified);
        assert_eq!(t.utc, c.modified);
        assert!(t.guessed);
    }

    #[test]
    fn junk_dates_fall_through() {
        let mut c = base();
        c.filename = "IMG_20230101_000000.jpg";
        c.exif_local = Some(naive(1970, 1, 1, 0, 0, 0)); // camera clock reset
        assert_eq!(choose_taken(&c).source, DateSource::Filename);
        c.exif_local = Some(naive(2030, 1, 1, 0, 0, 0)); // far future
        assert_eq!(choose_taken(&c).source, DateSource::Filename);
    }

    #[test]
    fn folder_uses_local_wall_clock() {
        let mut c = base();
        c.exif_local = Some(naive(2024, 5, 1, 0, 30, 0));
        c.exif_offset_minutes = Some(330);
        let t = choose_taken(&c);
        assert_eq!(t.utc, utc(2024, 4, 30, 19, 0, 0)); // April in UTC...
        assert_eq!(folder_year_month(&t), (2024, 5)); // ...but filed under May
    }

    #[test]
    fn folder_without_offset_uses_stored_time() {
        let mut c = base();
        c.exif_local = Some(naive(2023, 12, 31, 23, 59, 59));
        assert_eq!(folder_year_month(&choose_taken(&c)), (2023, 12));
    }
}
