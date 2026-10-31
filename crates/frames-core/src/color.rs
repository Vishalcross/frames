//! Color helper functions: hex conversion, HSL conversion, and WCAG contrast.

/// Parse a hex string `#RRGGBB` (with or without `#`) to RGB bytes.
pub fn from_hex(hex: &str) -> Result<[u8; 3], String> {
    let s = hex.strip_prefix('#').unwrap_or(hex);
    if s.len() != 6 {
        return Err(format!("invalid hex color length: {hex}"));
    }
    let r = u8::from_str_radix(&s[0..2], 16).map_err(|e| format!("invalid hex red: {e}"))?;
    let g = u8::from_str_radix(&s[2..4], 16).map_err(|e| format!("invalid hex green: {e}"))?;
    let b = u8::from_str_radix(&s[4..6], 16).map_err(|e| format!("invalid hex blue: {e}"))?;
    Ok([r, g, b])
}

/// Format RGB bytes to uppercase hex `#RRGGBB`.
pub fn to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// Convert RGB [0..255] to HSL [H in degrees 0..360, S in 0..1, L in 0..1].
pub fn rgb_to_hsl(rgb: [u8; 3]) -> [f64; 3] {
    let r = rgb[0] as f64 / 255.0;
    let g = rgb[1] as f64 / 255.0;
    let b = rgb[2] as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let l = (max + min) / 2.0;

    if delta.abs() < 1e-9 {
        return [0.0, 0.0, l];
    }

    let s = if l > 0.5 {
        delta / (2.0 - max - min)
    } else {
        delta / (max + min)
    };

    let mut h = if (max - r).abs() < 1e-9 {
        let segment = (g - b) / delta;
        segment + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < 1e-9 {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };

    h *= 60.0;
    if h < 0.0 {
        h += 360.0;
    } else if h >= 360.0 {
        h -= 360.0;
    }

    [h, s, l]
}

/// Convert HSL [H in degrees 0..360, S in 0..1, L in 0..1] to RGB [0..255].
pub fn hsl_to_rgb(hsl: [f64; 3]) -> [u8; 3] {
    let [h, s, l] = hsl;
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);

    if s.abs() < 1e-9 {
        let v = (l * 255.0).round().clamp(0.0, 255.0) as u8;
        return [v, v, v];
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    let hk = (h % 360.0 + 360.0) % 360.0 / 360.0;

    let tr = hk + 1.0 / 3.0;
    let tg = hk;
    let tb = hk - 1.0 / 3.0;

    let conv = |mut t: f64| -> u8 {
        if t < 0.0 {
            t += 1.0;
        } else if t > 1.0 {
            t -= 1.0;
        }

        let c = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (c * 255.0).round().clamp(0.0, 255.0) as u8
    };

    [conv(tr), conv(tg), conv(tb)]
}

/// Calculate relative luminance using the WCAG 2.1 formula.
pub fn relative_luminance(rgb: [u8; 3]) -> f64 {
    fn channel(v: u8) -> f64 {
        let s = v as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

/// Calculate the contrast ratio between two colors according to WCAG 2.1.
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f64 {
    let lum_a = relative_luminance(a);
    let lum_b = relative_luminance(b);
    let brightest = lum_a.max(lum_b);
    let darkest = lum_a.min(lum_b);
    (brightest + 0.05) / (darkest + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let hex = "#1A2B3C";
        let rgb = from_hex(hex).expect("valid hex");
        assert_eq!(rgb, [0x1A, 0x2B, 0x3C]);
        assert_eq!(to_hex(rgb), hex);
    }

    #[test]
    fn contrast_ratio_black_and_white() {
        let black = [0, 0, 0];
        let white = [255, 255, 255];
        let ratio = contrast_ratio(black, white);
        assert!(
            (ratio - 21.0).abs() <= 0.01,
            "expected contrast ratio ~21.0, got {ratio}"
        );
        let ratio_rev = contrast_ratio(white, black);
        assert!(
            (ratio_rev - 21.0).abs() <= 0.01,
            "expected contrast ratio ~21.0, got {ratio_rev}"
        );
    }

    #[test]
    fn hsl_to_rgb_round_trip_five_sample_colors() {
        let samples: [[u8; 3]; 5] = [
            [255, 0, 0],     // pure red
            [0, 255, 0],     // pure green
            [0, 0, 255],     // pure blue
            [26, 43, 60],    // #1A2B3C
            [128, 128, 128], // neutral grey
        ];

        for c in samples {
            let hsl = rgb_to_hsl(c);
            let round_trip = hsl_to_rgb(hsl);
            for i in 0..3 {
                let diff = (c[i] as i16 - round_trip[i] as i16).abs();
                assert!(
                    diff <= 1,
                    "channel {i} exceeded tolerance: expected {}, got {} for input {:?}",
                    c[i],
                    round_trip[i],
                    c
                );
            }
        }
    }
}
