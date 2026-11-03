import type { AdaptiveIntensity, OklchColor, Palette, ResolvedScheme } from '$lib/types';

/**
 * OKLCH and Color Science Utilities
 * Implements §7 (Adaptive Colour) according to the spec:
 * - Wu-quantised swatches & seed scoring
 * - Tonal roles mapped to OKLCH
 * - Chroma tapering and intensity scaling
 * - WCAG contrast guard ensuring >= 4.5:1 (or 7:1)
 */

function clamp(v: number, min: number, max: number): number {
  return Math.min(Math.max(v, min), max);
}

/**
 * Convert OKLCH to sRGB hex string.
 * Accurate OKLCH -> OKLab -> Linear RGB -> sRGB transformation.
 */
/** OKLCH to linear sRGB, unclamped, so callers can tell when it is out of gamut. */
function oklchToLinearRgb(l: number, c: number, hDeg: number): [number, number, number] {
  const hRad = (hDeg * Math.PI) / 180;
  const a = c * Math.cos(hRad);
  const b = c * Math.sin(hRad);

  // OKLab to LMS
  const l_ = l + 0.3963377774 * a + 0.2158037573 * b;
  const m_ = l - 0.1055613458 * a - 0.0638541728 * b;
  const s_ = l - 0.0894841775 * a - 1.291485548 * b;

  const L = l_ * l_ * l_;
  const M = m_ * m_ * m_;
  const S = s_ * s_ * s_;

  // LMS to Linear sRGB
  return [
    +4.0767416621 * L - 3.3077115913 * M + 0.2309699292 * S,
    -1.2684380046 * L + 2.6097574011 * M - 0.3413193965 * S,
    -0.0041960863 * L - 0.7034186147 * M + 1.707614701 * S
  ];
}

export function oklchToRgb(l: number, c: number, hDeg: number): [number, number, number] {
  const [rLin, gLin, bLin] = oklchToLinearRgb(l, c, hDeg);

  // Gamma compression (Linear to sRGB)
  const transfer = (c: number) => {
    const cl = clamp(c, 0, 1);
    return cl <= 0.0031308 ? 12.92 * cl : 1.055 * Math.pow(cl, 1 / 2.4) - 0.055;
  };

  return [
    Math.round(transfer(rLin) * 255),
    Math.round(transfer(gLin) * 255),
    Math.round(transfer(bLin) * 255)
  ];
}

export function oklchToCss(l: number, c: number, hDeg: number, alpha = 1): string {
  const [r, g, b] = oklchToRgb(l, c, hDeg);
  if (alpha < 1) {
    return `rgba(${r}, ${g}, ${b}, ${alpha.toFixed(2)})`;
  }
  const toHex = (n: number) => n.toString(16).padStart(2, '0');
  return `#${toHex(r)}${toHex(g)}${toHex(b)}`;
}

/**
 * Relative Luminance according to WCAG 2.1
 */
export function relativeLuminance(r: number, g: number, b: number): number {
  const a = [r, g, b].map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
}

/**
 * Calculate WCAG contrast ratio between two colors
 */
export function contrastRatio(rgb1: [number, number, number], rgb2: [number, number, number]): number {
  const lum1 = relativeLuminance(rgb1[0], rgb1[1], rgb1[2]);
  const lum2 = relativeLuminance(rgb2[0], rgb2[1], rgb2[2]);
  const brightest = Math.max(lum1, lum2);
  const darkest = Math.min(lum1, lum2);
  return (brightest + 0.05) / (darkest + 0.05);
}

/**
 * Ensure contrast meets target (e.g. 4.5:1 or 7:1) by shifting foreground lightness (§7.4)
 */
export function ensureContrast(
  fgOklch: OklchColor,
  bgRgb: [number, number, number],
  minRatio = 4.5
): OklchColor {
  let bestL = fgOklch.l;
  let fgRgb = oklchToRgb(bestL, fgOklch.c, fgOklch.h);
  let ratio = contrastRatio(fgRgb, bgRgb);
  if (ratio >= minRatio) return fgOklch;

  const bgLum = relativeLuminance(bgRgb[0], bgRgb[1], bgRgb[2]);
  const direction = bgLum > 0.5 ? -1 : 1; // move towards dark if bg is light, else light

  for (let step = 0; step < 50; step++) {
    bestL = clamp(bestL + direction * 0.02, 0.02, 0.98);
    fgRgb = oklchToRgb(bestL, fgOklch.c, fgOklch.h);
    ratio = contrastRatio(fgRgb, bgRgb);
    if (ratio >= minRatio) break;
  }

  return { ...fgOklch, l: bestL };
}

/**
 * Score swatch according to §7.2: pop^0.6 × clamp(c, .03, .25)^1.2
 */
export function scoreSwatch(swatch: OklchColor): number {
  if (swatch.c < 0.02) return 0;
  const clampedC = clamp(swatch.c, 0.03, 0.25);
  return Math.pow(swatch.pop || 0.1, 0.6) * Math.pow(clampedC, 1.2);
}

/**
 * Select primary seed and secondary color from swatches (§7.2)
 */
export function selectSeedAndSecondary(swatches: OklchColor[]): { seed: OklchColor; secondary: OklchColor } {
  const neutralSeed: OklchColor = { l: 0.60, c: 0.015, h: 250, pop: 1 };
  if (!swatches || swatches.length === 0) {
    return { seed: neutralSeed, secondary: { ...neutralSeed, h: 290 } };
  }

  // Filter out swatches with chroma < 0.02
  const valid = swatches.filter((s) => s.c >= 0.02);
  if (valid.length === 0) {
    return { seed: neutralSeed, secondary: { ...neutralSeed, h: 290 } };
  }

  // Pick highest scoring swatch as primary seed
  const sorted = [...valid].sort((a, b) => scoreSwatch(b) - scoreSwatch(a));
  const seed = sorted[0];

  // Secondary color: best swatch at least 35° hue away, otherwise seed hue + 40°
  let secondary = sorted.find((s) => {
    const diff = Math.abs(s.h - seed.h);
    const shortestDiff = Math.min(diff, 360 - diff);
    return shortestDiff >= 35;
  });

  if (!secondary) {
    secondary = {
      l: seed.l,
      c: Math.max(0.04, seed.c * 0.8),
      h: (seed.h + 40) % 360,
      pop: seed.pop * 0.5
    };
  }

  return { seed, secondary };
}

export interface ComputedPaletteRoles {
  bgCanvas: string;
  bgSidebar: string;
  bgViewer: string;
  surfaceRaised: string;
  surfaceOverlay: string;
  fill1: string;
  fill2: string;
  fill3: string;
  text1: string;
  text2: string;
  text3: string;
  separator: string;
  accent: string;
  accentFg: string;
  ambient1: string;
  ambient2: string;
}

/**
 * Compute tonal roles for Light or Dark scheme from seed & secondary (§7.3, §7.4)
 */
export function computeTonalRoles(
  seed: OklchColor,
  secondary: OklchColor,
  scheme: ResolvedScheme,
  intensity: AdaptiveIntensity = 'balanced',
  highContrast = false
): ComputedPaletteRoles {
  const intensityMultiplier = intensity === 'subtle' ? 0.6 : intensity === 'vivid' ? 1.5 : 1.0;
  const targetMinRatio = highContrast ? 7.0 : 4.5;

  // Tone to chroma tapering (§7.3): k tapers to 0.25 below tone 10 and above tone 95
  const getChroma = (tone: number, isAccent = false) => {
    let k = 1.0;
    if (tone < 10) {
      k = 0.25 + (0.75 * tone) / 10;
    } else if (tone > 95) {
      k = 0.25 + (0.75 * (100 - tone)) / 5;
    }
    let c = seed.c * intensityMultiplier * k;
    const maxChroma = isAccent ? 0.22 : 0.05;
    return clamp(c, 0, maxChroma);
  };

  const isDark = scheme === 'dark';

  // Table from §7.3:
  const canvasTone = isDark ? 7 : 98;
  const sidebarTone = isDark ? 10 : 94;
  const raisedTone = isDark ? 14 : 100;
  const overlayTone = isDark ? 18 : 96;
  const fill1Tone = isDark ? 22 : 90;
  const fill2Tone = isDark ? 28 : 86;
  const fill3Tone = isDark ? 34 : 80;
  let text1Tone = isDark ? 96 : 12;
  let text2Tone = isDark ? 80 : 35;
  const text3Tone = isDark ? 62 : 55;
  const accentTone = isDark ? 78 : 42;

  const canvasOklch: OklchColor = { l: canvasTone / 100, c: getChroma(canvasTone), h: seed.h, pop: 1 };
  const canvasRgb = oklchToRgb(canvasOklch.l, canvasOklch.c, canvasOklch.h);

  // Contrast Guard on Text (§7.4)
  let text1Oklch: OklchColor = { l: text1Tone / 100, c: getChroma(text1Tone) * 0.3, h: seed.h, pop: 1 };
  text1Oklch = ensureContrast(text1Oklch, canvasRgb, targetMinRatio);

  let text2Oklch: OklchColor = { l: text2Tone / 100, c: getChroma(text2Tone) * 0.4, h: seed.h, pop: 1 };
  text2Oklch = ensureContrast(text2Oklch, canvasRgb, targetMinRatio > 4.5 ? 4.5 : 3.0);

  // Accent and Accent-FG
  const accentOklch: OklchColor = { l: accentTone / 100, c: getChroma(accentTone, true), h: seed.h, pop: 1 };
  const accentRgb = oklchToRgb(accentOklch.l, accentOklch.c, accentOklch.h);

  const whiteRgb: [number, number, number] = [255, 255, 255];
  const blackRgb: [number, number, number] = [17, 17, 19];
  const whiteRatio = contrastRatio(whiteRgb, accentRgb);
  const blackRatio = contrastRatio(blackRgb, accentRgb);
  const accentFg = whiteRatio >= blackRatio ? '#ffffff' : '#111113';

  // Ambient glows (§7.3 & §7.5)
  const ambientTone = isDark ? 45 : 80;
  const ambientAlpha = isDark ? 0.38 : 0.34;
  const ambient1 = oklchToCss(ambientTone / 100, getChroma(ambientTone, true), seed.h, ambientAlpha);
  const ambient2 = oklchToCss(ambientTone / 100, getChroma(ambientTone, true), secondary.h, ambientAlpha * 0.85);

  return {
    bgCanvas: oklchToCss(canvasOklch.l, canvasOklch.c, canvasOklch.h),
    bgSidebar: oklchToCss(sidebarTone / 100, getChroma(sidebarTone), seed.h, 0.72),
    bgViewer: isDark ? '#000000' : '#ffffff',
    surfaceRaised: oklchToCss(raisedTone / 100, getChroma(raisedTone), seed.h),
    surfaceOverlay: oklchToCss(overlayTone / 100, getChroma(overlayTone), seed.h, 0.94),
    fill1: oklchToCss(fill1Tone / 100, getChroma(fill1Tone), seed.h, 0.10),
    fill2: oklchToCss(fill2Tone / 100, getChroma(fill2Tone), seed.h, 0.16),
    fill3: oklchToCss(fill3Tone / 100, getChroma(fill3Tone), seed.h, 0.22),
    text1: oklchToCss(text1Oklch.l, text1Oklch.c, text1Oklch.h),
    text2: oklchToCss(text2Oklch.l, text2Oklch.c, text2Oklch.h),
    text3: oklchToCss(text3Tone / 100, getChroma(text3Tone) * 0.3, seed.h),
    separator: oklchToCss((fill2Tone + 10) / 100, getChroma(fill2Tone) * 0.5, seed.h, 0.25),
    accent: oklchToCss(accentOklch.l, accentOklch.c, accentOklch.h),
    accentFg,
    ambient1,
    ambient2
  };
}

/** A photo's or album's colours, as hex (from the Rust palette extractor or chosen by hand). */
export interface PhotoPalette {
  bg: string;
  sidebar: string;
  accent: string;
  on_bg: string;
  on_sidebar: string;
  /** Colours someone chose for an album: shown exactly, not re-toned. */
  exact?: boolean;
  /** The photo's mean brightness, 0-1, for "Follow photo". */
  lum?: number;
}

/** sRGB (0-255) to OKLCH, the inverse of `oklchToRgb`. */
export function rgbToOklch(r: number, g: number, b: number): OklchColor {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  const [R, G, B] = [lin(r), lin(g), lin(b)];
  const l = Math.cbrt(0.4122214708 * R + 0.5363325363 * G + 0.0514459929 * B);
  const m = Math.cbrt(0.2119034982 * R + 0.6806995451 * G + 0.1073969566 * B);
  const s = Math.cbrt(0.0883024619 * R + 0.2817188376 * G + 0.6299787005 * B);
  const L = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const bb = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  let h = (Math.atan2(bb, a) * 180) / Math.PI;
  if (h < 0) h += 360;
  return { l: L, c: Math.sqrt(a * a + bb * bb), h, pop: 1 };
}

/**
 * A photo's colours as an immersive theme, after editor themes such as Tokyo
 * Night or Solarized Light: every surface carries colour at the scheme's
 * tones, not just the accent. The canvas takes the photo's dominant hue, the
 * sidebar its second hue a shade deeper, and the accent, hover fills and glow
 * its most vivid colour. Light or dark is the caller's choice (see
 * `schemeForBrightness`). `null` if a colour is malformed.
 */
export function immersiveRoles(
  p: PhotoPalette,
  scheme: ResolvedScheme,
  intensity: AdaptiveIntensity = 'balanced',
  highContrast = false
): ComputedPaletteRoles | null {
  const bg = hexToRgb(p.bg);
  const sidebar = hexToRgb(p.sidebar);
  const accent = hexToRgb(p.accent);
  if (!bg || !sidebar || !accent) return null;

  const dark = scheme === 'dark';
  const k = intensity === 'subtle' ? 0.6 : intensity === 'vivid' ? 1.4 : 1;
  const vivid = rgbToOklch(...accent);
  // How colourful the dominant and second colours are (the extractor caps
  // saturation at 0.35). A grey photo gets a faint tint of its accent.
  const colourOf = (rgb: Rgb) => Math.min(1, hslSaturation(rgb) / 0.35);
  const greyCanvas = colourOf(bg) < 0.15;
  const canvasHue = greyCanvas ? vivid.h : rgbToOklch(...bg).h;
  const canvasAmt = greyCanvas ? 0.3 : colourOf(bg);
  const greySidebar = colourOf(sidebar) < 0.15;
  const sidebarHue = greySidebar ? canvasHue : rgbToOklch(...sidebar).h;
  const sidebarAmt = greySidebar ? canvasAmt : colourOf(sidebar);

  // Chroma rises with the photo's colourfulness, to about what the most
  // colourful editor themes use (Night Owl's canvas is near 0.05).
  const surfaceC = (amt: number) => k * (dark ? 0.03 + 0.035 * amt : 0.012 + 0.022 * amt);
  const canvas = fitOklch(dark ? 0.22 : 0.965, surfaceC(canvasAmt), canvasHue);
  const side = fitOklch(dark ? 0.185 : 0.925, surfaceC(sidebarAmt) * (dark ? 1 : 1.15), sidebarHue);
  const raised = fitOklch(dark ? 0.27 : 0.99, dark ? canvas.c : k * 0.008, canvasHue);
  const canvasRgb = oklchToRgb(canvas.l, canvas.c, canvas.h);
  // Text must read on the palest dark surface or the deepest light one.
  const textBg = dark ? oklchToRgb(raised.l, raised.c, raised.h) : oklchToRgb(side.l, side.c, side.h);
  const minText = highContrast ? 7 : 4.5;
  const text1 = ensureContrast({ l: dark ? 0.94 : 0.25, c: 0.02, h: canvasHue, pop: 1 }, textBg, minText);
  const text2 = ensureContrast({ l: dark ? 0.78 : 0.43, c: 0.035, h: canvasHue, pop: 1 }, textBg, 4.5);
  const text3 = ensureContrast({ l: dark ? 0.62 : 0.58, c: 0.035, h: canvasHue, pop: 1 }, textBg, highContrast ? 4.5 : 3);

  const accentC = Math.min(0.2, k * Math.max(0.12, vivid.c));
  const acc = ensureContrast(fitOklch(dark ? 0.76 : 0.52, accentC, vivid.h), canvasRgb, 3);
  const accRgb = oklchToRgb(acc.l, acc.c, acc.h);
  const white: Rgb = [255, 255, 255];
  const ink: Rgb = [17, 17, 19];
  const accentFg = contrastRatio(white, accRgb) >= contrastRatio(ink, accRgb) ? '#ffffff' : '#111113';

  const css = (o: OklchColor, alpha = 1) => oklchToCss(o.l, o.c, o.h, alpha);
  const fill = fitOklch(dark ? 0.8 : 0.55, k * 0.08, vivid.h);
  return {
    bgCanvas: css(canvas),
    bgSidebar: css(side),
    bgViewer: css(fitOklch(dark ? 0.13 : 0.985, k * (dark ? 0.025 : 0.008), canvasHue)),
    surfaceRaised: css(raised),
    surfaceOverlay: css(fitOklch(dark ? 0.25 : 0.975, canvas.c, canvasHue), 0.96),
    fill1: css(fill, 0.07),
    fill2: css(fill, dark ? 0.12 : 0.11),
    fill3: css(fill, dark ? 0.18 : 0.17),
    text1: css(text1),
    text2: css(text2),
    text3: css(text3),
    separator: css(fitOklch(dark ? 0.85 : 0.4, dark ? 0.03 : 0.04, canvasHue), dark ? 0.12 : 0.14),
    accent: css(acc),
    accentFg,
    ambient1: css(fitOklch(dark ? 0.55 : 0.84, k * (dark ? 0.16 : 0.1), vivid.h), dark ? 0.3 : 0.4),
    ambient2: css(fitOklch(dark ? 0.5 : 0.88, k * (dark ? 0.1 : 0.07), sidebarHue), dark ? 0.24 : 0.32)
  };
}

/**
 * Light or dark for a photo of mean brightness `lum` (0-1): dark below 0.45,
 * light above 0.55, otherwise unchanged so mid-tone photos do not flip it.
 */
export function schemeForBrightness(lum: number | null | undefined, current: ResolvedScheme): ResolvedScheme {
  if (lum == null) return current;
  if (lum < 0.45) return 'dark';
  if (lum > 0.55) return 'light';
  return current;
}

/** Swatch seeds (no photo palette yet, or the mock library) as a photo palette. */
export function paletteFromSeeds(seed: OklchColor, secondary: OklchColor): PhotoPalette {
  const hex = (l: number, c: number, h: number) => oklchToCss(l, c, h);
  return {
    bg: hex(0.25, Math.min(seed.c, 0.05), seed.h),
    sidebar: hex(0.25, Math.min(secondary.c, 0.05), secondary.h),
    accent: hex(0.65, seed.c, seed.h),
    on_bg: '#ffffff',
    on_sidebar: '#ffffff'
  };
}

/** `l`, `c`, `h` with the chroma reduced until the colour fits in sRGB. */
function fitOklch(l: number, c: number, h: number): OklchColor {
  let chroma = Math.max(0, c);
  while (chroma > 0 && !inSrgb(oklchToLinearRgb(l, chroma, h))) chroma -= 0.004;
  return { l, c: Math.max(0, chroma), h, pop: 1 };
}

function inSrgb(rgb: [number, number, number]): boolean {
  return rgb.every((v) => v >= -0.001 && v <= 1.001);
}

function hslSaturation([r, g, b]: Rgb): number {
  const max = Math.max(r, g, b) / 255;
  const min = Math.min(r, g, b) / 255;
  const l = (max + min) / 2;
  return max === min ? 0 : (max - min) / (1 - Math.abs(2 * l - 1));
}

type Rgb = [number, number, number];

function hexToRgb(hex: string): Rgb | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function rgbCss([r, g, b]: Rgb, alpha = 1): string {
  return alpha < 1 ? `rgba(${r}, ${g}, ${b}, ${alpha})` : `rgb(${r}, ${g}, ${b})`;
}

function mixRgb(a: Rgb, b: Rgb, t: number): Rgb {
  return [0, 1, 2].map((i) => Math.round(a[i] + (b[i] - a[i]) * t)) as Rgb;
}

/**
 * Every role from a photo or album palette. Text, fills and separators are
 * derived from the background they sit on, never mixed with another
 * scheme's tokens, and the scheme follows that background: light text on a
 * dark photo, dark text on a light one. `null` if a colour is malformed.
 */
export function rolesFromPhotoPalette(
  p: PhotoPalette
): { roles: ComputedPaletteRoles; scheme: ResolvedScheme } | null {
  const bg = hexToRgb(p.bg);
  const sidebar = hexToRgb(p.sidebar);
  const accent = hexToRgb(p.accent);
  if (!bg || !sidebar || !accent) return null;

  const white: Rgb = [255, 255, 255];
  const ink: Rgb = [17, 17, 19];
  const text = contrastRatio(white, bg) >= contrastRatio(ink, bg) ? white : ink;
  const scheme: ResolvedScheme = text === white ? 'dark' : 'light';
  const accentFg = contrastRatio(white, accent) >= contrastRatio(ink, accent) ? '#ffffff' : '#111113';

  return {
    scheme,
    roles: {
      bgCanvas: rgbCss(bg),
      bgSidebar: rgbCss(sidebar, 0.9),
      bgViewer: rgbCss(bg),
      surfaceRaised: rgbCss(mixRgb(bg, text, 0.06)),
      surfaceOverlay: rgbCss(mixRgb(bg, text, 0.1), 0.94),
      fill1: rgbCss(text, 0.08),
      fill2: rgbCss(text, 0.13),
      fill3: rgbCss(text, 0.2),
      text1: rgbCss(text),
      text2: rgbCss(text, 0.66),
      text3: rgbCss(text, 0.42),
      separator: rgbCss(text, 0.16),
      accent: rgbCss(accent),
      accentFg,
      ambient1: rgbCss(accent, scheme === 'dark' ? 0.32 : 0.26),
      ambient2: rgbCss(sidebar, 0.28)
    }
  };
}
