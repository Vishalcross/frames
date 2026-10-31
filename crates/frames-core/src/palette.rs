//! Image palette extraction, caching, and role normalisation.

use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::color::{contrast_ratio, from_hex, hsl_to_rgb, rgb_to_hsl, to_hex};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    Dark,
    Light,
}

impl Default for Tone {
    fn default() -> Self {
        Tone::Dark
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Palette {
    pub bg: String,
    pub sidebar: String,
    pub accent: String,
    pub on_bg: String,
    pub on_sidebar: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumTheme {
    pub album_id: String,
    pub mode: String,
    pub source: String,
    pub bg: Option<String>,
    pub sidebar: Option<String>,
    pub accent: Option<String>,
    pub on_bg: Option<String>,
    pub on_sidebar: Option<String>,
    pub auto_bg: Option<String>,
    pub auto_sidebar: Option<String>,
    pub auto_accent: Option<String>,
    pub auto_on_bg: Option<String>,
    pub auto_on_sidebar: Option<String>,
}

#[derive(Default, Clone)]
struct Bucket {
    count: u32,
    sum_r: u64,
    sum_g: u64,
    sum_b: u64,
}

impl Bucket {
    fn avg_rgb(&self) -> [u8; 3] {
        if self.count == 0 {
            return [0, 0, 0];
        }
        let r = (self.sum_r as f64 / self.count as f64).round().clamp(0.0, 255.0) as u8;
        let g = (self.sum_g as f64 / self.count as f64).round().clamp(0.0, 255.0) as u8;
        let b = (self.sum_b as f64 / self.count as f64).round().clamp(0.0, 255.0) as u8;
        [r, g, b]
    }
}

struct EvaluatedBucket {
    idx: usize,
    count: u32,
    avg_rgb: [u8; 3],
}

pub fn fallback_palette(tone: Tone) -> Palette {
    match tone {
        Tone::Dark => Palette {
            bg: "#141416".to_string(),
            sidebar: "#1E1E22".to_string(),
            accent: "#0A84FF".to_string(),
            on_bg: "#FFFFFF".to_string(),
            on_sidebar: "#FFFFFF".to_string(),
        },
        Tone::Light => Palette {
            bg: "#F2F2F7".to_string(),
            sidebar: "#E5E5EA".to_string(),
            accent: "#0A84FF".to_string(),
            on_bg: "#111111".to_string(),
            on_sidebar: "#111111".to_string(),
        },
    }
}

/// Calculate Euclidean straight RGB distance between two colours.
pub fn rgb_distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    let dr = a[0] as f64 - b[0] as f64;
    let dg = a[1] as f64 - b[1] as f64;
    let db = a[2] as f64 - b[2] as f64;
    (dr * dr + dg * dg + db * db).sqrt()
}

/// Pick text color (#FFFFFF or #111111) giving contrast >= 4.5 against the surface.
pub fn text_color_for(surface: [u8; 3]) -> String {
    let white = [255, 255, 255];
    let dark = [17, 17, 17];
    let cr_white = contrast_ratio(white, surface);
    let cr_dark = contrast_ratio(dark, surface);

    if cr_white >= 4.5 && cr_dark < 4.5 {
        "#FFFFFF".to_string()
    } else if cr_dark >= 4.5 && cr_white < 4.5 {
        "#111111".to_string()
    } else if cr_white >= cr_dark {
        "#FFFFFF".to_string()
    } else {
        "#111111".to_string()
    }
}

/// Normalise colour roles according to tone constraints, contrast rules, and text colours.
pub fn normalise_roles(bg: &str, sidebar: &str, accent: &str, tone: Tone) -> Palette {
    let bg_rgb = from_hex(bg).unwrap_or([20, 20, 22]);
    let mut sb_rgb = from_hex(sidebar).unwrap_or([30, 30, 34]);
    let mut acc_rgb = from_hex(accent).unwrap_or([10, 132, 255]);

    // 1. If two of the three colours are closer than 40 (straight RGB distance),
    // move the sidebar lightness by 0.05 in the tone's direction until they are 40 apart (at most 10 steps).
    let mut sb_hsl = rgb_to_hsl(sb_rgb);
    let tone_dir = match tone {
        Tone::Dark => 1.0,
        Tone::Light => -1.0,
    };

    for _ in 0..10 {
        let d_bg_sb = rgb_distance(bg_rgb, sb_rgb);
        let d_sb_acc = rgb_distance(sb_rgb, acc_rgb);

        if d_bg_sb < 40.0 || d_sb_acc < 40.0 {
            sb_hsl[2] = (sb_hsl[2] + tone_dir * 0.05).clamp(0.0, 1.0);
            sb_rgb = hsl_to_rgb(sb_hsl);
        } else {
            break;
        }
    }

    // 2. Accent against background must have contrast at least 3.0.
    // If not, change the accent lightness by 0.05 per step, up to 10 steps.
    let mut acc_hsl = rgb_to_hsl(acc_rgb);
    if contrast_ratio(acc_rgb, bg_rgb) < 3.0 {
        let cr_up = {
            let mut h = acc_hsl;
            h[2] = (h[2] + 0.05).clamp(0.0, 1.0);
            contrast_ratio(hsl_to_rgb(h), bg_rgb)
        };
        let cr_down = {
            let mut h = acc_hsl;
            h[2] = (h[2] - 0.05).clamp(0.0, 1.0);
            contrast_ratio(hsl_to_rgb(h), bg_rgb)
        };
        let dir = if cr_up >= cr_down { 1.0 } else { -1.0 };
        for _ in 0..10 {
            if contrast_ratio(acc_rgb, bg_rgb) >= 3.0 {
                break;
            }
            let next_l = (acc_hsl[2] + dir * 0.05).clamp(0.0, 1.0);
            if (next_l - acc_hsl[2]).abs() < 1e-4 {
                break;
            }
            acc_hsl[2] = next_l;
            acc_rgb = hsl_to_rgb(acc_hsl);
        }
        if contrast_ratio(acc_rgb, bg_rgb) < 3.0 {
            let other_dir = -dir;
            for _ in 0..10 {
                if contrast_ratio(acc_rgb, bg_rgb) >= 3.0 {
                    break;
                }
                let next_l = (acc_hsl[2] + other_dir * 0.05).clamp(0.0, 1.0);
                if (next_l - acc_hsl[2]).abs() < 1e-4 {
                    break;
                }
                acc_hsl[2] = next_l;
                acc_rgb = hsl_to_rgb(acc_hsl);
            }
        }
    }

    // 3. Text colours for background and sidebar
    let on_bg = text_color_for(bg_rgb);
    let on_sidebar = text_color_for(sb_rgb);

    Palette {
        bg: to_hex(bg_rgb),
        sidebar: to_hex(sb_rgb),
        accent: to_hex(acc_rgb),
        on_bg,
        on_sidebar,
    }
}

/// Extract an adaptive palette from an image thumbnail path.
pub fn extract_palette(thumb: &Path, tone: Tone) -> Palette {
    let img = match image::open(thumb) {
        Ok(i) => i,
        Err(_) => return fallback_palette(tone),
    };

    // 1. Open the 256 px thumbnail and shrink it to 64x64.
    let rgba = img.to_rgba8();
    let resized = image::imageops::resize(&rgba, 64, 64, image::imageops::FilterType::Triangle);

    // 2. Reduce each pixel to 4 bits per channel (r>>4, g>>4, b>>4).
    // Count pixels per bucket and keep the average real colour of each bucket. Skip alpha < 128.
    let mut buckets = vec![Bucket::default(); 4096];
    let mut total_pixels = 0;

    for pixel in resized.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 128 {
            continue;
        }
        let idx = (((r >> 4) as usize) << 8) | (((g >> 4) as usize) << 4) | ((b >> 4) as usize);
        buckets[idx].count += 1;
        buckets[idx].sum_r += r as u64;
        buckets[idx].sum_g += g as u64;
        buckets[idx].sum_b += b as u64;
        total_pixels += 1;
    }

    if total_pixels == 0 {
        return fallback_palette(tone);
    }

    // 3. Sort buckets by count, largest first. Call the largest dominant.
    let mut sorted_buckets: Vec<EvaluatedBucket> = buckets
        .iter()
        .enumerate()
        .filter(|(_, b)| b.count > 0)
        .map(|(idx, b)| EvaluatedBucket {
            idx,
            count: b.count,
            avg_rgb: b.avg_rgb(),
        })
        .collect();

    sorted_buckets.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.idx.cmp(&b.idx)));

    let dominant = &sorted_buckets[0];

    // 4. Accent: score each bucket as sqrt(count) * saturation, only for buckets
    // with HSL lightness between 0.25 and 0.95. The best score wins.
    // If the winner's saturation is below 0.35, use #0A84FF.
    let mut best_score = -1.0;
    let mut best_bucket: Option<&EvaluatedBucket> = None;
    let mut best_hsl = [0.0, 0.0, 0.0];

    for b in &sorted_buckets {
        let hsl = rgb_to_hsl(b.avg_rgb);
        if hsl[2] >= 0.25 && hsl[2] <= 0.95 {
            let score = (b.count as f64).sqrt() * hsl[1];
            if score > best_score {
                best_score = score;
                best_bucket = Some(b);
                best_hsl = hsl;
            }
        }
    }

    let accent_hex = match best_bucket {
        Some(_) if best_hsl[1] >= 0.35 => {
            let mut acc_hsl = best_hsl;
            acc_hsl[2] = match tone {
                Tone::Dark => acc_hsl[2].clamp(0.45, 0.65),
                Tone::Light => acc_hsl[2].clamp(0.40, 0.55),
            };
            to_hex(hsl_to_rgb(acc_hsl))
        }
        _ => "#0A84FF".to_string(),
    };

    // 5. Background: dominant, with saturation capped at 0.35 and lightness moved into the tone's band.
    let mut dom_hsl = rgb_to_hsl(dominant.avg_rgb);
    dom_hsl[1] = dom_hsl[1].min(0.35);
    dom_hsl[2] = match tone {
        Tone::Dark => dom_hsl[2].clamp(0.10, 0.18),
        Tone::Light => dom_hsl[2].clamp(0.93, 0.97),
    };
    let bg_hex = to_hex(hsl_to_rgb(dom_hsl));

    // 6. Sidebar: the next bucket whose hue differs from the background by at least 20 degrees,
    // with lightness set to the tone's sidebar rule. If none, use the background with the sidebar lightness rule applied.
    let bg_hue = dom_hsl[0];
    let bg_has_hue = dom_hsl[1] >= 0.05;
    let mut sidebar_hex = None;

    if bg_has_hue {
        for b in sorted_buckets.iter().skip(1) {
            let b_hsl = rgb_to_hsl(b.avg_rgb);
            if b_hsl[1] < 0.05 {
                continue;
            }
            let diff = (b_hsl[0] - bg_hue).abs();
            let hue_dist = diff.min(360.0 - diff);
            if hue_dist >= 20.0 {
                let mut sb_hsl = b_hsl;
                sb_hsl[1] = sb_hsl[1].min(0.35);
                sb_hsl[2] = match tone {
                    Tone::Dark => (dom_hsl[2] + 0.04).clamp(0.0, 1.0),
                    Tone::Light => (dom_hsl[2] - 0.04).clamp(0.0, 1.0),
                };
                sidebar_hex = Some(to_hex(hsl_to_rgb(sb_hsl)));
                break;
            }
        }
    }

    let sidebar_hex = sidebar_hex.unwrap_or_else(|| {
        let mut sb_hsl = dom_hsl;
        sb_hsl[2] = match tone {
            Tone::Dark => (dom_hsl[2] + 0.04).clamp(0.0, 1.0),
            Tone::Light => (dom_hsl[2] - 0.04).clamp(0.0, 1.0),
        };
        to_hex(hsl_to_rgb(sb_hsl))
    });

    // 7. Normalise roles
    normalise_roles(&bg_hex, &sidebar_hex, &accent_hex, tone)
}

/// Compute palette from the 256px thumbnail for asset_id and store in photo_palettes.
pub fn compute_and_store_palette(conn: &rusqlite::Connection, asset_id: &str) -> Result<Palette, String> {
    let thumb_file = crate::thumbs::thumb_path(asset_id, crate::thumbs::ThumbSize::Small);
    if !thumb_file.exists() {
        return Err(format!("thumbnail does not exist for asset {asset_id}"));
    }
    let palette = extract_palette(&thumb_file, Tone::Dark);
    let now = chrono::Utc::now().timestamp_millis();
    conn.execute(
        "INSERT INTO photo_palettes (asset_id, bg, sidebar, accent, on_bg, on_sidebar, computed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(asset_id) DO UPDATE SET
             bg = excluded.bg,
             sidebar = excluded.sidebar,
             accent = excluded.accent,
             on_bg = excluded.on_bg,
             on_sidebar = excluded.on_sidebar,
             computed_at = excluded.computed_at",
        rusqlite::params![
            asset_id,
            palette.bg,
            palette.sidebar,
            palette.accent,
            palette.on_bg,
            palette.on_sidebar,
            now,
        ],
    ).map_err(|e| format!("failed to store palette: {e}"))?;

    Ok(palette)
}

/// Retrieve the cached palette for an asset from photo_palettes.
pub fn get_palette_from_db(conn: &rusqlite::Connection, asset_id: &str) -> rusqlite::Result<Option<Palette>> {
    let mut stmt = conn.prepare(
        "SELECT bg, sidebar, accent, on_bg, on_sidebar FROM photo_palettes WHERE asset_id = ?1",
    )?;
    let mut rows = stmt.query([asset_id])?;
    if let Some(r) = rows.next()? {
        Ok(Some(Palette {
            bg: r.get(0)?,
            sidebar: r.get(1)?,
            accent: r.get(2)?,
            on_bg: r.get(3)?,
            on_sidebar: r.get(4)?,
        }))
    } else {
        Ok(None)
    }
}

fn is_valid_hex_color(s: &str) -> bool {
    if s.len() != 7 || !s.starts_with('#') {
        return false;
    }
    s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Retrieve the album theme settings, returning default values if not configured.
pub fn get_album_theme(conn: &rusqlite::Connection, album_id: &str) -> rusqlite::Result<AlbumTheme> {
    let mut stmt = conn.prepare(
        "SELECT mode, source, bg, sidebar, accent, auto_bg, auto_sidebar, auto_accent, auto_on_bg, auto_on_sidebar
         FROM album_theme WHERE album_id = ?1",
    )?;
    let mut rows = stmt.query([album_id])?;
    if let Some(r) = rows.next()? {
        let mode: String = r.get(0)?;
        let source: String = r.get(1)?;
        let bg: Option<String> = r.get(2)?;
        let sidebar: Option<String> = r.get(3)?;
        let accent: Option<String> = r.get(4)?;
        let auto_bg: Option<String> = r.get(5)?;
        let auto_sidebar: Option<String> = r.get(6)?;
        let auto_accent: Option<String> = r.get(7)?;
        let auto_on_bg: Option<String> = r.get(8)?;
        let auto_on_sidebar: Option<String> = r.get(9)?;

        let (on_bg, on_sidebar) = if source == "manual" {
            let ob = bg.as_deref().and_then(|h| from_hex(h).ok()).map(text_color_for);
            let os = sidebar.as_deref().and_then(|h| from_hex(h).ok()).map(text_color_for);
            (ob, os)
        } else {
            (None, None)
        };

        Ok(AlbumTheme {
            album_id: album_id.to_string(),
            mode,
            source,
            bg,
            sidebar,
            accent,
            on_bg,
            on_sidebar,
            auto_bg,
            auto_sidebar,
            auto_accent,
            auto_on_bg,
            auto_on_sidebar,
        })
    } else {
        Ok(AlbumTheme {
            album_id: album_id.to_string(),
            mode: "dynamic".to_string(),
            source: "auto".to_string(),
            bg: None,
            sidebar: None,
            accent: None,
            on_bg: None,
            on_sidebar: None,
            auto_bg: None,
            auto_sidebar: None,
            auto_accent: None,
            auto_on_bg: None,
            auto_on_sidebar: None,
        })
    }
}

/// Set the album theme settings with strict validation.
pub fn set_album_theme(
    conn: &rusqlite::Connection,
    album_id: &str,
    mode: &str,
    source: &str,
    bg: Option<String>,
    sidebar: Option<String>,
    accent: Option<String>,
) -> Result<AlbumTheme, String> {
    if mode != "dynamic" && mode != "album" {
        return Err(format!("invalid mode '{mode}', expected 'dynamic' or 'album'"));
    }
    if source != "auto" && source != "manual" {
        return Err(format!("invalid source '{source}', expected 'auto' or 'manual'"));
    }

    if source == "manual" {
        if bg.is_none() || sidebar.is_none() || accent.is_none() {
            return Err("source = manual requires all three colours (bg, sidebar, accent)".into());
        }
    }

    for (label, col) in [("bg", &bg), ("sidebar", &sidebar), ("accent", &accent)] {
        if let Some(c) = col {
            if !is_valid_hex_color(c) {
                return Err(format!("invalid hex colour for {label}: '{c}', expected #RRGGBB"));
            }
        }
    }

    conn.execute(
        "INSERT INTO album_theme (album_id, mode, source, bg, sidebar, accent)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(album_id) DO UPDATE SET
             mode = excluded.mode,
             source = excluded.source,
             bg = excluded.bg,
             sidebar = excluded.sidebar,
             accent = excluded.accent",
        rusqlite::params![album_id, mode, source, bg, sidebar, accent],
    ).map_err(|e| format!("failed to save album theme: {e}"))?;

    get_album_theme(conn, album_id).map_err(|e| format!("failed to read back album theme: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn make_test_image(w: u32, h: u32, painter: impl Fn(u32, u32) -> Rgba<u8>) -> std::path::PathBuf {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(x, y, painter(x, y));
            }
        }
        let tmp = std::env::temp_dir().join(format!("test_pal_{}.png", fastrand::u64(..)));
        img.save(&tmp).expect("save test png");
        tmp
    }

    #[test]
    fn solid_red_gives_accent_near_red() {
        let path = make_test_image(256, 256, |_, _| Rgba([255, 0, 0, 255]));
        let p = extract_palette(&path, Tone::Dark);
        let _ = std::fs::remove_file(&path);

        let acc_rgb = from_hex(&p.accent).expect("valid hex");
        assert!(
            acc_rgb[0] > 200 && acc_rgb[1] < 60 && acc_rgb[2] < 60,
            "expected accent near red, got {:?}",
            acc_rgb
        );
    }

    #[test]
    fn half_blue_half_orange_gives_three_different_colours() {
        let path = make_test_image(256, 256, |_, y| {
            if y < 128 {
                Rgba([0, 80, 255, 255])
            } else {
                Rgba([255, 140, 0, 255])
            }
        });
        let p = extract_palette(&path, Tone::Dark);
        let _ = std::fs::remove_file(&path);

        assert_ne!(p.bg, p.sidebar, "bg and sidebar must differ");
        assert_ne!(p.sidebar, p.accent, "sidebar and accent must differ");
        assert_ne!(p.bg, p.accent, "bg and accent must differ");
    }

    #[test]
    fn grey_image_gives_accent_0a84ff() {
        let path = make_test_image(256, 256, |_, _| Rgba([128, 128, 128, 255]));
        let p = extract_palette(&path, Tone::Dark);
        let _ = std::fs::remove_file(&path);

        assert_eq!(p.accent, "#0A84FF");
    }

    #[test]
    fn white_and_black_images_both_give_text_contrast_at_least_4_5() {
        let white_path = make_test_image(256, 256, |_, _| Rgba([255, 255, 255, 255]));
        let white_pal = extract_palette(&white_path, Tone::Light);
        let _ = std::fs::remove_file(&white_path);

        let bg_rgb = from_hex(&white_pal.bg).unwrap();
        let sb_rgb = from_hex(&white_pal.sidebar).unwrap();
        let on_bg_rgb = from_hex(&white_pal.on_bg).unwrap();
        let on_sb_rgb = from_hex(&white_pal.on_sidebar).unwrap();

        let cr_bg = contrast_ratio(on_bg_rgb, bg_rgb);
        let cr_sb = contrast_ratio(on_sb_rgb, sb_rgb);
        assert!(cr_bg >= 4.5, "white image bg text contrast {cr_bg} < 4.5");
        assert!(cr_sb >= 4.5, "white image sidebar text contrast {cr_sb} < 4.5");

        let black_path = make_test_image(256, 256, |_, _| Rgba([0, 0, 0, 255]));
        let black_pal = extract_palette(&black_path, Tone::Dark);
        let _ = std::fs::remove_file(&black_path);

        let bg_rgb = from_hex(&black_pal.bg).unwrap();
        let sb_rgb = from_hex(&black_pal.sidebar).unwrap();
        let on_bg_rgb = from_hex(&black_pal.on_bg).unwrap();
        let on_sb_rgb = from_hex(&black_pal.on_sidebar).unwrap();

        let cr_bg = contrast_ratio(on_bg_rgb, bg_rgb);
        let cr_sb = contrast_ratio(on_sb_rgb, sb_rgb);
        assert!(cr_bg >= 4.5, "black image bg text contrast {cr_bg} < 4.5");
        assert!(cr_sb >= 4.5, "black image sidebar text contrast {cr_sb} < 4.5");
    }

    #[test]
    fn album_theme_defaults_for_unknown_album() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::store::Store::run_migrations(&conn).unwrap();

        let theme = get_album_theme(&conn, "unknown-album").unwrap();
        assert_eq!(theme.mode, "dynamic");
        assert_eq!(theme.source, "auto");
        assert!(theme.bg.is_none());
        assert!(theme.sidebar.is_none());
        assert!(theme.accent.is_none());
    }

    #[test]
    fn album_theme_manual_save_with_two_colours_is_rejected() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::store::Store::run_migrations(&conn).unwrap();

        let res = set_album_theme(
            &conn,
            "alb1",
            "album",
            "manual",
            Some("#112233".into()),
            Some("#445566".into()),
            None, // Missing accent!
        );
        assert!(res.is_err(), "manual save with only two colours must be rejected");
    }

    #[test]
    fn album_theme_valid_save_round_trips() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::store::Store::run_migrations(&conn).unwrap();

        let saved = set_album_theme(
            &conn,
            "alb1",
            "album",
            "manual",
            Some("#1A2B3C".into()),
            Some("#2B3C4D".into()),
            Some("#FF5500".into()),
        ).expect("valid save");

        assert_eq!(saved.mode, "album");
        assert_eq!(saved.source, "manual");
        assert_eq!(saved.bg.as_deref(), Some("#1A2B3C"));
        assert_eq!(saved.sidebar.as_deref(), Some("#2B3C4D"));
        assert_eq!(saved.accent.as_deref(), Some("#FF5500"));
        // Text colours computed automatically without altering manual colours
        assert!(saved.on_bg.is_some());
        assert!(saved.on_sidebar.is_some());

        let loaded = get_album_theme(&conn, "alb1").expect("load saved");
        assert_eq!(loaded, saved);
    }

    #[test]
    fn palette_derivation_is_fast_under_100ms() {
        let path = make_test_image(256, 256, |x, y| {
            Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
        });
        let start = std::time::Instant::now();
        let _pal = extract_palette(&path, Tone::Dark);
        let elapsed = start.elapsed();
        let _ = std::fs::remove_file(&path);
        println!("palette derivation on 256x256 took: {:?}", elapsed);
        assert!(elapsed.as_millis() < 100, "derivation took {}ms, must be < 100ms", elapsed.as_millis());
    }

    #[test]
    fn normalise_roles_increases_contrast_when_accent_is_close_to_bg() {
        // bg is midtone grey, accent is slightly darker grey with contrast < 3.0
        let bg = "#333333";
        let sidebar = "#444444";
        let accent = "#282828";
        let pal = normalise_roles(bg, sidebar, accent, Tone::Dark);
        let cr = contrast_ratio(from_hex(&pal.accent).unwrap(), from_hex(&pal.bg).unwrap());
        assert!(
            cr >= 3.0,
            "expected contrast ratio >= 3.0, got {cr} (accent: {}, bg: {})",
            pal.accent,
            pal.bg
        );
    }
}

