//! S4 bucket keys (spec section 4): building, name-clash naming and parsing.

use chrono::{DateTime, Utc};

pub const LOG_PREFIX: &str = "index/log/";
pub const SNAPSHOT_PREFIX: &str = "index/snapshot-";

/// Drop any directory part, so a file name can never add key levels.
fn file_name_only(name: &str) -> &str {
    name.rsplit(|c: char| c == '/' || c == '\\').next().unwrap_or(name)
}

/// `originals/YYYY/MM/<filename>`. Year and month are the date taken, as
/// local wall-clock time (see `dates::folder_year_month`).
pub fn original_key(year: i32, month: u32, filename: &str) -> String {
    format!("originals/{year:04}/{month:02}/{}", file_name_only(filename))
}

/// `<name>-<first 8 hex of id>.<ext>`: used when the target key already holds
/// different content. A name without an extension gets the suffix at the end.
pub fn clash_filename(filename: &str, id: &str) -> String {
    let filename = file_name_only(filename);
    let short: String = id.chars().take(8).collect();
    match filename.rfind('.') {
        Some(i) if i > 0 => format!("{}-{}{}", &filename[..i], short, &filename[i..]),
        _ => format!("{filename}-{short}"),
    }
}

/// `thumbs/<id>/s.webp`: small thumbnail.
pub fn thumb_small_key(id: &str) -> String {
    format!("thumbs/{id}/s.webp")
}

/// `thumbs/<id>/m.webp`: preview.
pub fn thumb_preview_key(id: &str) -> String {
    format!("thumbs/{id}/m.webp")
}

/// Device ids are used in keys, so keep them to a safe alphabet.
pub fn is_valid_device_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `index/log/<device>/` : the folder one device writes its batches into.
pub fn log_prefix(device: &str) -> String {
    format!("{LOG_PREFIX}{device}/")
}

/// `index/log/<device>/<seq, 9 digits>.json`
pub fn log_key(device: &str, seq: u64) -> String {
    format!("{LOG_PREFIX}{device}/{seq:09}.json")
}

/// Split a log key into (device, seq). Returns `None` for anything else.
pub fn parse_log_key(key: &str) -> Option<(&str, u64)> {
    let rest = key.strip_prefix(LOG_PREFIX)?;
    let (device, file) = rest.split_once('/')?;
    let digits = file.strip_suffix(".json")?;
    if device.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((device, digits.parse().ok()?))
}

/// `index/snapshot-<YYYYMMDDTHHMMSSZ>-<device>.json.gz`. The compact timestamp
/// sorts alphabetically in time order, so the newest snapshot is the maximum key.
pub fn snapshot_key(at: DateTime<Utc>, device: &str) -> String {
    format!("{SNAPSHOT_PREFIX}{}-{device}.json.gz", at.format("%Y%m%dT%H%M%SZ"))
}

/// Split a snapshot key into (timestamp, device).
pub fn parse_snapshot_key(key: &str) -> Option<(&str, &str)> {
    let rest = key.strip_prefix(SNAPSHOT_PREFIX)?.strip_suffix(".json.gz")?;
    let (ts, device) = rest.split_once('-')?;
    if ts.len() != 16 || !ts.ends_with('Z') || device.is_empty() {
        return None;
    }
    Some((ts, device))
}

/// The newest snapshot among a bucket listing; non-snapshot keys are ignored.
pub fn newest_snapshot<'a, I: IntoIterator<Item = &'a str>>(keys: I) -> Option<&'a str> {
    keys.into_iter()
        .filter_map(|k| parse_snapshot_key(k).map(|(ts, dev)| (ts, dev, k)))
        .max()
        .map(|(_, _, k)| k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const ID: &str = "ab12cd34ef56ab12cd34ef56ab12cd34ef56ab12cd34ef56ab12cd34ef56ab12";

    #[test]
    fn original_key_pads_month_and_keeps_name() {
        assert_eq!(original_key(2024, 5, "IMG_1234.HEIC"), "originals/2024/05/IMG_1234.HEIC");
        assert_eq!(original_key(2024, 12, "a b.jpg"), "originals/2024/12/a b.jpg");
    }

    #[test]
    fn original_key_never_adds_levels() {
        assert_eq!(original_key(2024, 5, "../x/IMG.jpg"), "originals/2024/05/IMG.jpg");
        assert_eq!(original_key(2024, 5, r"C:\pics\IMG.jpg"), "originals/2024/05/IMG.jpg");
    }

    #[test]
    fn clash_name_goes_before_the_extension() {
        assert_eq!(clash_filename("IMG_1234.HEIC", ID), "IMG_1234-ab12cd34.HEIC");
        assert_eq!(clash_filename("a.b.jpg", ID), "a.b-ab12cd34.jpg");
    }

    #[test]
    fn clash_name_without_extension() {
        assert_eq!(clash_filename("README", ID), "README-ab12cd34");
        assert_eq!(clash_filename(".hidden", ID), ".hidden-ab12cd34");
    }

    #[test]
    fn thumb_keys() {
        assert_eq!(thumb_small_key("abc"), "thumbs/abc/s.webp");
        assert_eq!(thumb_preview_key("abc"), "thumbs/abc/m.webp");
    }

    #[test]
    fn device_ids() {
        assert!(is_valid_device_id("mac-7f3a"));
        assert!(!is_valid_device_id(""));
        assert!(!is_valid_device_id("a/b"));
        assert!(!is_valid_device_id("héllo"));
    }

    #[test]
    fn log_key_round_trip() {
        let key = log_key("mac-7f3a", 123);
        assert_eq!(key, "index/log/mac-7f3a/000000123.json");
        assert_eq!(parse_log_key(&key), Some(("mac-7f3a", 123)));
        assert!(key.starts_with(&log_prefix("mac-7f3a")));
    }

    #[test]
    fn parse_log_key_rejects_other_keys() {
        assert_eq!(parse_log_key("originals/2024/05/IMG.jpg"), None);
        assert_eq!(parse_log_key("index/log/mac/abc.json"), None);
        assert_eq!(parse_log_key("index/log/mac/000000001.txt"), None);
        assert_eq!(parse_log_key("index/log//000000001.json"), None);
        assert_eq!(parse_log_key("index/log/mac"), None);
    }

    #[test]
    fn snapshot_key_round_trip() {
        let at = Utc.with_ymd_and_hms(2026, 11, 2, 3, 14, 15).unwrap();
        let key = snapshot_key(at, "mac-7f3a");
        assert_eq!(key, "index/snapshot-20261102T031415Z-mac-7f3a.json.gz");
        assert_eq!(parse_snapshot_key(&key), Some(("20261102T031415Z", "mac-7f3a")));
    }

    #[test]
    fn newest_snapshot_picks_latest_and_ignores_noise() {
        let keys = [
            "index/snapshot-20261001T000000Z-win-01.json.gz",
            "index/snapshot-20261102T031415Z-mac-7f3a.json.gz",
            "index/snapshot-20260901T000000Z-mac-7f3a.json.gz",
            "index/log/mac-7f3a/000000001.json",
            "index/snapshot-garbage.json.gz",
        ];
        assert_eq!(
            newest_snapshot(keys.iter().copied()),
            Some("index/snapshot-20261102T031415Z-mac-7f3a.json.gz")
        );
        assert_eq!(newest_snapshot(std::iter::empty()), None);
    }
}
