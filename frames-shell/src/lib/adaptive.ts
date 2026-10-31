/**
 * Adaptive colour theming engine (Phase 4).
 * Coordinates album theme settings, photo palettes, DOM application,
 * and debounced transitions.
 */
import { invoke } from '@tauri-apps/api/core';

export interface AdaptivePalette {
  bg: string;
  sidebar: string;
  accent: string;
  on_bg: string;
  on_sidebar: string;
}

export interface BackendAlbumTheme {
  album_id: string;
  mode: 'dynamic' | 'album';
  source: 'auto' | 'manual';
  bg?: string | null;
  sidebar?: string | null;
  accent?: string | null;
  on_bg?: string | null;
  on_sidebar?: string | null;
  auto_bg?: string | null;
  auto_sidebar?: string | null;
  auto_accent?: string | null;
  auto_on_bg?: string | null;
  auto_on_sidebar?: string | null;
}

export type CommandInvoker = (cmd: string, args?: Record<string, unknown>) => Promise<any>;

let currentInvoker: CommandInvoker = async (cmd: string, args?: Record<string, unknown>) => {
  try {
    return await invoke(cmd, args);
  } catch (err) {
    console.warn(`[adaptive] invoke error for ${cmd}:`, err);
    return null;
  }
};

export function setCommandInvoker(invoker: CommandInvoker) {
  currentInvoker = invoker;
}

let activeAlbumTheme: BackendAlbumTheme | null = null;
let currentAppliedPalette: AdaptivePalette | null = null;
let requestCounter = 0;
let debounceTimer: any = null;
let isListeningToPaletteReady = false;

/**
 * Setup Tauri event listener for album-palette-ready
 */
export async function initAdaptiveListeners(): Promise<void> {
  if (isListeningToPaletteReady || typeof window === 'undefined') return;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    await listen<{ album_id: string }>('album-palette-ready', async (event) => {
      const albId = event.payload?.album_id;
      if (activeAlbumTheme && activeAlbumTheme.album_id === albId) {
        // Refresh album theme
        const updatedTheme = await currentInvoker('get_album_theme', { albumId: albId });
        if (updatedTheme) {
          activeAlbumTheme = updatedTheme;
          if (updatedTheme.auto_bg && updatedTheme.auto_sidebar && updatedTheme.auto_accent) {
            const pal: AdaptivePalette = {
              bg: updatedTheme.auto_bg,
              sidebar: updatedTheme.auto_sidebar,
              accent: updatedTheme.auto_accent,
              on_bg: updatedTheme.auto_on_bg || '#FFFFFF',
              on_sidebar: updatedTheme.auto_on_sidebar || '#FFFFFF'
            };
            applyPalette(pal);
          }
        }
      }
    });
    isListeningToPaletteReady = true;
  } catch {
    // not in Tauri
  }
}

/**
 * Decide which palette applies for an asset and album (Task 4-g).
 */
export async function resolvePalette(
  assetId: string | null,
  albumId?: string | null
): Promise<AdaptivePalette | null> {
  let targetAlbumId = albumId;

  // A photo always belongs to one album, so opening a photo from All Photos,
  // Years or Months uses that album's setting.
  if (!targetAlbumId && assetId) {
    try {
      const foundId = await currentInvoker('get_asset_album_id', { assetId });
      if (foundId) {
        targetAlbumId = foundId;
      }
    } catch {
      // ignore
    }
  }

  let theme: BackendAlbumTheme | null = null;

  if (targetAlbumId) {
    theme = await currentInvoker('get_album_theme', { albumId: targetAlbumId });
    if (theme) {
      activeAlbumTheme = theme;
    }
  }

  // 1. If album mode is active
  if (theme && theme.mode === 'album') {
    // 2. If source = manual: return manual colours with text colours
    if (theme.source === 'manual') {
      if (theme.bg && theme.sidebar && theme.accent) {
        return {
          bg: theme.bg,
          sidebar: theme.sidebar,
          accent: theme.accent,
          on_bg: theme.on_bg || '#FFFFFF',
          on_sidebar: theme.on_sidebar || '#FFFFFF'
        };
      }
      return null;
    }

    // 3. If source = auto: return auto_* colours
    if (theme.source === 'auto') {
      if (theme.auto_bg && theme.auto_sidebar && theme.auto_accent) {
        return {
          bg: theme.auto_bg,
          sidebar: theme.auto_sidebar,
          accent: theme.auto_accent,
          on_bg: theme.auto_on_bg || '#FFFFFF',
          on_sidebar: theme.auto_on_sidebar || '#FFFFFF'
        };
      }
      // If empty, call regenerate_album_palette once and keep showing previous palette
      currentInvoker('regenerate_album_palette', { albumId: targetAlbumId }).catch(() => {});
      return null;
    }
  }

  // 4. If mode = dynamic (or no album theme): return get_palette(assetId)
  if (assetId) {
    const pal = await currentInvoker('get_palette', { assetId });
    if (pal && pal.bg && pal.sidebar && pal.accent) {
      return pal as AdaptivePalette;
    }
  }

  // 5. If result is null, return null (caller keeps previous palette)
  return null;
}

/**
 * Determine contrast text colour (#FFFFFF or #111111) for accent.
 */
export function getContrastTextColor(hex: string): string {
  try {
    const clean = hex.startsWith('#') ? hex.slice(1) : hex;
    const r = parseInt(clean.slice(0, 2), 16) / 255;
    const g = parseInt(clean.slice(2, 4), 16) / 255;
    const b = parseInt(clean.slice(4, 6), 16) / 255;
    const toLinear = (c: number) => (c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
    const L = 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
    const crWhite = (1.0 + 0.05) / (L + 0.05);
    const crBlack = (L + 0.05) / 0.05;
    return crBlack > crWhite ? '#111111' : '#FFFFFF';
  } catch {
    return '#FFFFFF';
  }
}

/**
 * Apply the 5 CSS variables to document.documentElement (Task 4-h #1).
 */
export function applyPalette(p: AdaptivePalette): void {
  if (typeof document === 'undefined' || !p) return;
  currentAppliedPalette = p;
  const root = document.documentElement;

  root.style.setProperty('--adaptive-bg', p.bg);
  root.style.setProperty('--adaptive-sidebar', p.sidebar);
  root.style.setProperty('--adaptive-accent', p.accent);
  root.style.setProperty('--adaptive-on-bg', p.on_bg);
  root.style.setProperty('--adaptive-on-sidebar', p.on_sidebar);
  root.style.setProperty('--adaptive-on-accent', getContrastTextColor(p.accent));
}

/**
 * Clear the 5 CSS variables from document.documentElement (Task 4-h #6).
 */
export function clearAdaptivePalette(): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;
  root.style.removeProperty('--adaptive-bg');
  root.style.removeProperty('--adaptive-sidebar');
  root.style.removeProperty('--adaptive-accent');
  root.style.removeProperty('--adaptive-on-bg');
  root.style.removeProperty('--adaptive-on-sidebar');
  root.style.removeProperty('--adaptive-on-accent');
}

/**
 * Restore current palette when switching back to Adaptive (Task 4-h #6).
 */
export function restoreAdaptivePalette(): void {
  if (currentAppliedPalette) {
    applyPalette(currentAppliedPalette);
  }
}

/**
 * Get currently applied adaptive palette.
 */
export function getCurrentPalette(): AdaptivePalette | null {
  return currentAppliedPalette;
}

/**
 * Handle viewed photo change with 120ms debounce and request race guard (Task 4-h #4).
 */
export function handleViewedPhotoChange(
  assetId: string | null,
  albumId?: string | null
): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
  }

  const reqId = ++requestCounter;
  debounceTimer = setTimeout(async () => {
    try {
      const palette = await resolvePalette(assetId, albumId);
      // Ignore any result that is not from the newest request
      if (reqId !== requestCounter) return;

      // If the result is null and viewing a photo, do nothing (the previous colours stay).
      // If closing the viewer with no active album palette, restore expected grid colours.
      if (palette) {
        applyPalette(palette);
      } else if (!assetId) {
        clearAdaptivePalette();
      }
    } catch {
      // ignore
    }
  }, 120);
}
