//! Automatic album palette generation by hue clustering.

use rusqlite::Connection;
use crate::color::{from_hex, rgb_to_hsl, to_hex};
use crate::palette::{fallback_palette, get_palette_from_db, normalise_roles, Palette, Tone};

#[derive(Clone, Debug)]
pub struct PhotoPaletteMember {
    pub accent_rgb: [u8; 3],
    pub bg_rgb: [u8; 3],
    pub sidebar_rgb: [u8; 3],
}

fn avg_rgb_of(colors: &[[u8; 3]]) -> [u8; 3] {
    if colors.is_empty() {
        return [0, 0, 0];
    }
    let mut sr = 0u64;
    let mut sg = 0u64;
    let mut sb = 0u64;
    for c in colors {
        sr += c[0] as u64;
        sg += c[1] as u64;
        sb += c[2] as u64;
    }
    let n = colors.len() as f64;
    [
        (sr as f64 / n).round().clamp(0.0, 255.0) as u8,
        (sg as f64 / n).round().clamp(0.0, 255.0) as u8,
        (sb as f64 / n).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Derive the album palette from a slice of photo palettes according to the spec algorithm.
pub fn derive_album_palette(palettes: &[Palette], tone: Tone) -> Palette {
    if palettes.is_empty() {
        return fallback_palette(tone);
    }

    let mut hue_buckets: Vec<Vec<PhotoPaletteMember>> = vec![Vec::new(); 12];
    let mut all_members: Vec<PhotoPaletteMember> = Vec::new();

    for p in palettes {
        let acc_rgb = match from_hex(&p.accent) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let bg_rgb = from_hex(&p.bg).unwrap_or([20, 20, 22]);
        let sb_rgb = from_hex(&p.sidebar).unwrap_or([30, 30, 34]);

        let member = PhotoPaletteMember {
            accent_rgb: acc_rgb,
            bg_rgb,
            sidebar_rgb: sb_rgb,
        };
        all_members.push(member.clone());

        // Ignore accents with saturation below 0.35
        let hsl = rgb_to_hsl(acc_rgb);
        if hsl[1] >= 0.35 {
            // Split hue circle [0, 360) into 12 buckets of 30 degrees
            let h = (hsl[0] % 360.0 + 360.0) % 360.0;
            let b_idx = (h / 30.0).floor() as usize % 12;
            hue_buckets[b_idx].push(member);
        }
    }

    if all_members.is_empty() {
        return fallback_palette(tone);
    }

    // Find bucket with the most members
    let mut max_count = 0;
    let mut winning_idx: Option<usize> = None;

    for (idx, b) in hue_buckets.iter().enumerate() {
        if b.len() > max_count {
            max_count = b.len();
            winning_idx = Some(idx);
        }
    }

    let (accent_rgb, winning_members): ([u8; 3], &[PhotoPaletteMember]) = match winning_idx {
        Some(idx) if !hue_buckets[idx].is_empty() => {
            let members = &hue_buckets[idx];
            let acc_colors: Vec<[u8; 3]> = members.iter().map(|m| m.accent_rgb).collect();
            (avg_rgb_of(&acc_colors), members.as_slice())
        }
        _ => ([10, 132, 255], [].as_slice()), // Fallback #0A84FF
    };

    let (bg_colors, sb_colors): (Vec<[u8; 3]>, Vec<[u8; 3]>) = if winning_members.len() >= 3 {
        (
            winning_members.iter().map(|m| m.bg_rgb).collect(),
            winning_members.iter().map(|m| m.sidebar_rgb).collect(),
        )
    } else {
        (
            all_members.iter().map(|m| m.bg_rgb).collect(),
            all_members.iter().map(|m| m.sidebar_rgb).collect(),
        )
    };

    let bg_rgb = avg_rgb_of(&bg_colors);
    let sb_rgb = avg_rgb_of(&sb_colors);

    // Pass through normalise_roles
    normalise_roles(&to_hex(bg_rgb), &to_hex(sb_rgb), &to_hex(accent_rgb), tone)
}

/// Query album assets, sample up to 60, extract/cache their palettes,
/// generate the automatic album palette and persist it into the auto_* columns.
pub fn generate_and_save_album_palette(conn: &Connection, album_id: &str) -> rusqlite::Result<Palette> {
    // 1. Take the album's assets sorted by date.
    let mut stmt = conn.prepare(
        "SELECT a.id FROM assets a
         WHERE (a.album_id = ?1
            OR a.id IN (SELECT ai.asset_id FROM album_items ai WHERE ai.album_id = ?1 AND ai.in_album = 1))
           AND a.trashed_at IS NULL AND a.purged_at IS NULL
         ORDER BY a.taken_at ASC",
    )?;
    let asset_ids: Vec<String> = stmt
        .query_map([album_id], |r| r.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    // Pick up to 60, evenly spaced
    let total = asset_ids.len();
    let picked_ids: Vec<String> = if total <= 60 {
        asset_ids
    } else {
        (0..60)
            .map(|i| {
                let idx = (i * (total - 1)) / (60 - 1);
                asset_ids[idx].clone()
            })
            .collect()
    };

    // Get palette for each, skipping ones without a thumbnail
    let mut picked_palettes = Vec::new();
    for id in picked_ids {
        if let Ok(Some(pal)) = get_palette_from_db(conn, &id) {
            picked_palettes.push(pal);
        } else {
            let thumb = crate::thumbs::thumb_path(&id, crate::thumbs::ThumbSize::Small);
            if thumb.exists() {
                if let Ok(pal) = crate::palette::compute_and_store_palette(conn, &id) {
                    picked_palettes.push(pal);
                }
            }
        }
    }

    let pal = derive_album_palette(&picked_palettes, Tone::Dark);

    // Save to the auto_* columns
    conn.execute(
        "INSERT INTO album_theme (album_id, auto_bg, auto_sidebar, auto_accent, auto_on_bg, auto_on_sidebar)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(album_id) DO UPDATE SET
             auto_bg = excluded.auto_bg,
             auto_sidebar = excluded.auto_sidebar,
             auto_accent = excluded.auto_accent,
             auto_on_bg = excluded.auto_on_bg,
             auto_on_sidebar = excluded.auto_on_sidebar",
        rusqlite::params![
            album_id,
            pal.bg,
            pal.sidebar,
            pal.accent,
            pal.on_bg,
            pal.on_sidebar,
        ],
    )?;

    Ok(pal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_fake_palettes_dominated_by_blue_gives_blue_accent() {
        let mut palettes = Vec::new();
        // 8 blue palettes (hue ~ 220-230, blue bucket)
        for i in 0..8 {
            palettes.push(Palette {
                bg: "#101520".to_string(),
                sidebar: "#182030".to_string(),
                accent: format!("#{:02X}{:02X}FF", 10 + i * 5, 80 + i * 5), // Vivid blue
                on_bg: "#FFFFFF".to_string(),
                on_sidebar: "#FFFFFF".to_string(),
            });
        }
        // 2 orange palettes
        for _ in 0..2 {
            palettes.push(Palette {
                bg: "#201510".to_string(),
                sidebar: "#302018".to_string(),
                accent: "#FF8800".to_string(),
                on_bg: "#FFFFFF".to_string(),
                on_sidebar: "#FFFFFF".to_string(),
            });
        }

        let album_pal = derive_album_palette(&palettes, Tone::Dark);
        let acc_rgb = from_hex(&album_pal.accent).unwrap();
        let hsl = rgb_to_hsl(acc_rgb);

        // Blue hue is around 200..260 degrees
        assert!(
            hsl[0] >= 190.0 && hsl[0] <= 260.0,
            "expected blue accent hue (190..260), got {} for {:?}",
            hsl[0],
            acc_rgb
        );
    }

    #[test]
    fn album_with_no_thumbnails_gives_fallback_palette_without_crashing() {
        let conn = Connection::open_in_memory().unwrap();
        crate::store::Store::run_migrations(&conn).unwrap();

        // Empty album
        let pal = generate_and_save_album_palette(&conn, "empty_album").expect("must not crash");
        let fallback = fallback_palette(Tone::Dark);
        assert_eq!(pal.accent, fallback.accent);
        assert_eq!(pal.bg, fallback.bg);
        assert_eq!(pal.sidebar, fallback.sidebar);
    }
}
