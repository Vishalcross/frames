/**
 * Adaptive colour theming engine (Phase 4).
 * Coordinates album theme settings, photo palettes, DOM application,
 * and debounced transitions.
 */
import { invoke } from '@tauri-apps/api/core';
import { onBrowsingFocus, setPhotoPalette } from '$lib/theme/engine.svelte';

export interface AdaptivePalette {
  bg: string;
  sidebar: string;
  accent: string;
  on_bg: string;
  on_sidebar: string;
  /** Chosen by hand for an album, so shown exactly rather than re-toned. */
  exact?: boolean;
  /** The photo's mean brightness, 0-1 (photos only), for "Follow photo". */
  lum?: number;
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
// The album page being browsed, if any (set when no photo is open).
let pageAlbumId: string | null = null;

// "Viewer and browsing": the photo under the pointer or centred in the scroll
// colours the app like an open one, under the same album rules. Only an open
// photo decides light or dark, so hovering never flips the whole app.
onBrowsingFocus((assetId) => handleViewedPhotoChange(assetId, pageAlbumId, { browsing: true }));

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
          on_sidebar: theme.on_sidebar || '#FFFFFF',
          exact: true
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
 * Use this palette for the Adaptive appearance. The theme engine derives every
 * token from it (Task 4-h #1); it is kept, not shown, in other appearances.
 */
export function applyPalette(p: AdaptivePalette): void {
  if (!p) return;
  currentAppliedPalette = p;
  setPhotoPalette(p);
}

/**
 * Back to the neutral Adaptive palette (Task 4-h #6).
 */
export function clearAdaptivePalette(): void {
  currentAppliedPalette = null;
  setPhotoPalette(null);
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
  albumId?: string | null,
  opts: { browsing?: boolean } = {}
): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer);
  }
  if (!assetId) pageAlbumId = albumId ?? null;

  const reqId = ++requestCounter;
  debounceTimer = setTimeout(async () => {
    try {
      const palette = await resolvePalette(assetId, albumId);
      // Ignore any result that is not from the newest request
      if (reqId !== requestCounter) return;

      // If the result is null and viewing a photo, do nothing (the previous colours stay).
      // If closing the viewer with no active album palette, restore expected grid colours.
      if (palette) {
        applyPalette(opts.browsing ? { ...palette, lum: undefined } : palette);
      } else if (!assetId) {
        clearAdaptivePalette();
      }
    } catch {
      // ignore
    }
  }, 120);
}
