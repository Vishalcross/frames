//! S4 bucket keys (spec section 4): building, Unicode NFC filename normalization,
//! name-clash naming, and parsing.

use chrono::{DateTime, Utc};

pub const LOG_PREFIX: &str = "index/log/";
pub const SNAPSHOT_PREFIX: &str = "index/snapshot-";

/// Normalize a filename or string to Unicode NFC (composed form) so macOS (APFS/HFS+
/// decomposed NFD) and Windows (NTFS composed NFC) always produce identical S4 keys
/// and index filenames.
pub fn normalize_nfc(input: &str) -> String {
    if input.is_ascii() {
        return input.to_string();
    }
    let mut out: Vec<char> = Vec::with_capacity(input.len());
    for ch in input.chars() {
        if let Some(&last) = out.last() {
            if let Some(composed) = compose_canonical_pair(last, ch) {
                *out.last_mut().unwrap() = composed;
                continue;
            }
        }
        out.push(ch);
    }
    out.into_iter().collect()
}

/// Canonical composition of a starter/base character and combining mark (or Hangul Jamo
/// L+V / LV+T per Unicode Standard Annex #15).
fn compose_canonical_pair(base: char, mark: char) -> Option<char> {
    // 1. Algorithmic Hangul Jamo composition (UAX #15)
    const S_BASE: u32 = 0xAC00;
    const L_BASE: u32 = 0x1100;
    const V_BASE: u32 = 0x1161;
    const T_BASE: u32 = 0x11A7;
    const L_COUNT: u32 = 19;
    const V_COUNT: u32 = 21;
    const T_COUNT: u32 = 28;
    const N_COUNT: u32 = V_COUNT * T_COUNT; // 588
    const S_COUNT: u32 = L_COUNT * N_COUNT; // 11172

    let b_u32 = base as u32;
    let m_u32 = mark as u32;
    if (L_BASE..L_BASE + L_COUNT).contains(&b_u32) && (V_BASE..V_BASE + V_COUNT).contains(&m_u32) {
        let l_idx = b_u32 - L_BASE;
        let v_idx = m_u32 - V_BASE;
        let s = S_BASE + (l_idx * V_COUNT + v_idx) * T_COUNT;
        return char::from_u32(s);
    }
    if (S_BASE..S_BASE + S_COUNT).contains(&b_u32)
        && (b_u32 - S_BASE).is_multiple_of(T_COUNT)
        && (T_BASE + 1..T_BASE + T_COUNT).contains(&m_u32)
    {
        let t_idx = m_u32 - T_BASE;
        return char::from_u32(b_u32 + t_idx);
    }

    // 2. Common canonical precomposed Latin, Greek, Cyrillic, and Kana pairs
    match (base, mark) {
        // Grave (U+0300)
        ('A', '\u{0300}') => Some('\u{00C0}'),
        ('E', '\u{0300}') => Some('\u{00C8}'),
        ('I', '\u{0300}') => Some('\u{00CC}'),
        ('O', '\u{0300}') => Some('\u{00D2}'),
        ('U', '\u{0300}') => Some('\u{00D9}'),
        ('a', '\u{0300}') => Some('\u{00E0}'),
        ('e', '\u{0300}') => Some('\u{00E8}'),
        ('i', '\u{0300}') => Some('\u{00EC}'),
        ('o', '\u{0300}') => Some('\u{00F2}'),
        ('u', '\u{0300}') => Some('\u{00F9}'),
        // Acute (U+0301)
        ('A', '\u{0301}') => Some('\u{00C1}'),
        ('C', '\u{0301}') => Some('\u{0106}'),
        ('E', '\u{0301}') => Some('\u{00C9}'),
        ('I', '\u{0301}') => Some('\u{00CD}'),
        ('N', '\u{0301}') => Some('\u{0143}'),
        ('O', '\u{0301}') => Some('\u{00D3}'),
        ('S', '\u{0301}') => Some('\u{015A}'),
        ('U', '\u{0301}') => Some('\u{00DA}'),
        ('Y', '\u{0301}') => Some('\u{00DD}'),
        ('Z', '\u{0301}') => Some('\u{0179}'),
        ('a', '\u{0301}') => Some('\u{00E1}'),
        ('c', '\u{0301}') => Some('\u{0107}'),
        ('e', '\u{0301}') => Some('\u{00E9}'),
        ('i', '\u{0301}') => Some('\u{00ED}'),
        ('n', '\u{0301}') => Some('\u{0144}'),
        ('o', '\u{0301}') => Some('\u{00F3}'),
        ('s', '\u{0301}') => Some('\u{015B}'),
        ('u', '\u{0301}') => Some('\u{00FA}'),
        ('y', '\u{0301}') => Some('\u{00FD}'),
        ('z', '\u{0301}') => Some('\u{017A}'),
        // Circumflex (U+0302)
        ('A', '\u{0302}') => Some('\u{00C2}'),
        ('E', '\u{0302}') => Some('\u{00CA}'),
        ('I', '\u{0302}') => Some('\u{00CE}'),
        ('O', '\u{0302}') => Some('\u{00D4}'),
        ('U', '\u{0302}') => Some('\u{00DB}'),
        ('a', '\u{0302}') => Some('\u{00E2}'),
        ('e', '\u{0302}') => Some('\u{00EA}'),
        ('i', '\u{0302}') => Some('\u{00EE}'),
        ('o', '\u{0302}') => Some('\u{00F4}'),
        ('u', '\u{0302}') => Some('\u{00FB}'),
        // Tilde (U+0303)
        ('A', '\u{0303}') => Some('\u{00C3}'),
        ('N', '\u{0303}') => Some('\u{00D1}'),
        ('O', '\u{0303}') => Some('\u{00D5}'),
        ('a', '\u{0303}') => Some('\u{00E3}'),
        ('n', '\u{0303}') => Some('\u{00F1}'),
        ('o', '\u{0303}') => Some('\u{00F5}'),
        // Macron (U+0304)
        ('A', '\u{0304}') => Some('\u{0100}'),
        ('E', '\u{0304}') => Some('\u{0112}'),
        ('I', '\u{0304}') => Some('\u{012A}'),
        ('O', '\u{0304}') => Some('\u{014C}'),
        ('U', '\u{0304}') => Some('\u{016A}'),
        ('a', '\u{0304}') => Some('\u{0101}'),
        ('e', '\u{0304}') => Some('\u{0113}'),
        ('i', '\u{0304}') => Some('\u{012B}'),
        ('o', '\u{0304}') => Some('\u{014D}'),
        ('u', '\u{0304}') => Some('\u{016B}'),
        // Breve (U+0306)
        ('A', '\u{0306}') => Some('\u{0102}'),
        ('G', '\u{0306}') => Some('\u{011E}'),
        ('a', '\u{0306}') => Some('\u{0103}'),
        ('g', '\u{0306}') => Some('\u{011F}'),
        ('И', '\u{0306}') => Some('Й'),
        ('и', '\u{0306}') => Some('й'),
        // Dot above (U+0307)
        ('Z', '\u{0307}') => Some('\u{017B}'),
        ('z', '\u{0307}') => Some('\u{017C}'),
        // Diaeresis / Umlaut (U+0308)
        ('A', '\u{0308}') => Some('\u{00C4}'),
        ('E', '\u{0308}') => Some('\u{00CB}'),
        ('I', '\u{0308}') => Some('\u{00CF}'),
        ('O', '\u{0308}') => Some('\u{00D6}'),
        ('U', '\u{0308}') => Some('\u{00DC}'),
        ('Y', '\u{0308}') => Some('\u{0178}'),
        ('a', '\u{0308}') => Some('\u{00E4}'),
        ('e', '\u{0308}') => Some('\u{00EB}'),
        ('i', '\u{0308}') => Some('\u{00EF}'),
        ('o', '\u{0308}') => Some('\u{00F6}'),
        ('u', '\u{0308}') => Some('\u{00FC}'),
        ('y', '\u{0308}') => Some('\u{00FF}'),
        ('Е', '\u{0308}') => Some('Ё'),
        ('е', '\u{0308}') => Some('ё'),
        // Ring above (U+030A)
        ('A', '\u{030A}') => Some('\u{00C5}'),
        ('U', '\u{030A}') => Some('\u{016E}'),
        ('a', '\u{030A}') => Some('\u{00E5}'),
        ('u', '\u{030A}') => Some('\u{016F}'),
        // Caron (U+030C)
        ('C', '\u{030C}') => Some('\u{010C}'),
        ('D', '\u{030C}') => Some('\u{010E}'),
        ('E', '\u{030C}') => Some('\u{011A}'),
        ('N', '\u{030C}') => Some('\u{0147}'),
        ('R', '\u{030C}') => Some('\u{0158}'),
        ('S', '\u{030C}') => Some('\u{0160}'),
        ('T', '\u{030C}') => Some('\u{0164}'),
        ('Z', '\u{030C}') => Some('\u{017D}'),
        ('c', '\u{030C}') => Some('\u{010D}'),
        ('d', '\u{030C}') => Some('\u{010F}'),
        ('e', '\u{030C}') => Some('\u{011B}'),
        ('n', '\u{030C}') => Some('\u{0148}'),
        ('r', '\u{030C}') => Some('\u{0159}'),
        ('s', '\u{030C}') => Some('\u{0161}'),
        ('t', '\u{030C}') => Some('\u{0165}'),
        ('z', '\u{030C}') => Some('\u{017E}'),
        // Cedilla (U+0327) & Ogonek (U+0328)
        ('C', '\u{0327}') => Some('\u{00C7}'),
        ('S', '\u{0327}') => Some('\u{015E}'),
        ('T', '\u{0327}') => Some('\u{0162}'),
        ('c', '\u{0327}') => Some('\u{00E7}'),
        ('s', '\u{0327}') => Some('\u{015F}'),
        ('t', '\u{0327}') => Some('\u{0163}'),
        ('A', '\u{0328}') => Some('\u{0104}'),
        ('E', '\u{0328}') => Some('\u{0118}'),
        ('a', '\u{0328}') => Some('\u{0105}'),
        ('e', '\u{0328}') => Some('\u{0119}'),
        // Japanese Kana Dakuten (U+3099) & Handakuten (U+309A)
        ('か', '\u{3099}') => Some('が'),
        ('き', '\u{3099}') => Some('ぎ'),
        ('く', '\u{3099}') => Some('ぐ'),
        ('け', '\u{3099}') => Some('げ'),
        ('こ', '\u{3099}') => Some('ご'),
        ('さ', '\u{3099}') => Some('ざ'),
        ('し', '\u{3099}') => Some('じ'),
        ('す', '\u{3099}') => Some('ず'),
        ('せ', '\u{3099}') => Some('ぜ'),
        ('そ', '\u{3099}') => Some('ぞ'),
        ('た', '\u{3099}') => Some('だ'),
        ('ち', '\u{3099}') => Some('ぢ'),
        ('つ', '\u{3099}') => Some('づ'),
        ('て', '\u{3099}') => Some('で'),
        ('と', '\u{3099}') => Some('ど'),
        ('は', '\u{3099}') => Some('ば'),
        ('ひ', '\u{3099}') => Some('び'),
        ('ふ', '\u{3099}') => Some('ぶ'),
        ('へ', '\u{3099}') => Some('べ'),
        ('ほ', '\u{3099}') => Some('ぼ'),
        ('は', '\u{309A}') => Some('ぱ'),
        ('ひ', '\u{309A}') => Some('ぴ'),
        ('ふ', '\u{309A}') => Some('ぷ'),
        ('へ', '\u{309A}') => Some('ぺ'),
        ('ほ', '\u{309A}') => Some('ぽ'),
        ('カ', '\u{3099}') => Some('ガ'),
        ('キ', '\u{3099}') => Some('ギ'),
        ('ク', '\u{3099}') => Some('グ'),
        ('ケ', '\u{3099}') => Some('ゲ'),
        ('コ', '\u{3099}') => Some('ゴ'),
        ('サ', '\u{3099}') => Some('ザ'),
        ('シ', '\u{3099}') => Some('ジ'),
        ('ス', '\u{3099}') => Some('ズ'),
        ('セ', '\u{3099}') => Some('ゼ'),
        ('ソ', '\u{3099}') => Some('ゾ'),
        ('タ', '\u{3099}') => Some('ダ'),
        ('チ', '\u{3099}') => Some('ヂ'),
        ('ツ', '\u{3099}') => Some('ヅ'),
        ('テ', '\u{3099}') => Some('デ'),
        ('ト', '\u{3099}') => Some('ド'),
        ('ハ', '\u{3099}') => Some('バ'),
        ('ヒ', '\u{3099}') => Some('ビ'),
        ('フ', '\u{3099}') => Some('ブ'),
        ('ヘ', '\u{3099}') => Some('ベ'),
        ('ホ', '\u{3099}') => Some('ボ'),
        ('ハ', '\u{309A}') => Some('パ'),
        ('ヒ', '\u{309A}') => Some('ピ'),
        ('フ', '\u{309A}') => Some('プ'),
        ('ヘ', '\u{309A}') => Some('ペ'),
        ('ホ', '\u{309A}') => Some('ポ'),
        _ => None,
    }
}

/// Drop any directory part and normalize to Unicode NFC, so a file name can never
/// add key levels or diverge between macOS NFD and Windows NFC.
pub fn clean_filename(name: &str) -> String {
    let leaf = name.rsplit(['/', '\\']).next().unwrap_or(name);
    normalize_nfc(leaf)
}

/// `originals/YYYY/MM/<filename>`. Year and month are the date taken, as
/// local wall-clock time (see `dates::folder_year_month`).
pub fn original_key(year: i32, month: u32, filename: &str) -> String {
    format!("originals/{year:04}/{month:02}/{}", clean_filename(filename))
}

/// `<name>-<first 8 hex of id>.<ext>`: used when the target key already holds
/// different content, or when exporting two files with the same name to a flat folder.
/// A name without an extension gets the suffix at the end.
pub fn clash_filename(filename: &str, id: &str) -> String {
    let filename = clean_filename(filename);
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

    #[test]
    fn normalizes_nfd_and_nfc_filenames_to_identical_keys() {
        let mac_nfd = "Vacances_e\u{0301}te\u{0301}_\u{1100}\u{1161}.JPG";
        let win_nfc = "Vacances_\u{00e9}t\u{00e9}_\u{AC00}.JPG";
        assert_ne!(mac_nfd.as_bytes(), win_nfc.as_bytes());
        assert_eq!(
            original_key(2024, 7, mac_nfd),
            original_key(2024, 7, win_nfc)
        );
        assert_eq!(
            clash_filename(mac_nfd, ID),
            clash_filename(win_nfc, ID)
        );
    }
}
