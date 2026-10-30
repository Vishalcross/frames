import type {
  AdaptiveIntensity,
  Appearance,
  ColourSource,
  GlassClarity,
  GlassTint,
  OklchColor,
  Palette,
  ResolvedScheme,
  Scheme
} from '$lib/types';
import {
  computeTonalRoles,
  selectSeedAndSecondary,
  oklchToCss
} from './palette';
import { detectGlassTier, resolveGlassForeground, setForceGovernorLevel } from './glass';

/**
 * Theme Engine (§5.4)
 * Manages reactive theme state, asset focus priority, palette derivation,
 * and DOM attribute synchronization.
 */

const STORAGE_KEY = 'frames_theme_config';

interface StoredConfig {
  appearance: Appearance;
  scheme: Scheme;
  accent: string;
  intensity: AdaptiveIntensity;
  colourSource: ColourSource;
  glassTint: GlassTint;
  glassClarity: GlassClarity;
  reduceTransparency: boolean;
}

const defaultConfig: StoredConfig = {
  appearance: 'standard',
  scheme: 'system',
  accent: 'Blue',
  intensity: 'balanced',
  colourSource: 'both',
  glassTint: 'neutral',
  glassClarity: 'regular',
  reduceTransparency: false
};

function loadStoredConfig(): StoredConfig {
  if (typeof window === 'undefined') return defaultConfig;
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      return { ...defaultConfig, ...JSON.parse(raw) };
    }
  } catch {
    // fallback
  }
  return defaultConfig;
}

const initial = loadStoredConfig();

export const theme = $state({
  appearance: initial.appearance,
  scheme: initial.scheme,
  accent: initial.accent,
  intensity: initial.intensity,
  colourSource: initial.colourSource,
  glassTint: initial.glassTint,
  glassClarity: initial.glassClarity,
  reduceTransparency: initial.reduceTransparency
});

export const resolved = $state<{
  scheme: ResolvedScheme;
  tier: 1 | 2;
  palette: Palette | null;
}>({
  scheme: 'light',
  tier: 1,
  palette: null
});

// Cache for asset palettes provided by AssetRepo or viewer
const assetPaletteCache = new Map<string, { swatches: OklchColor[]; lum: number }>();

export function registerAssetPalette(id: string, swatches: OklchColor[], lum: number) {
  assetPaletteCache.set(id, { swatches, lum });
}

let activeViewerAssetId: string | null = null;
let activeDetailAssetId: string | null = null;
let hoverDwellTimer: any = null;
let lastScrollSeedTimestamp = 0;
let lastSeedHue = 250;

/**
 * Focus asset for adaptive theming priority (§7.6)
 */
export function focusAsset(
  id: string | null,
  opts: { source: 'viewer' | 'hover' | 'scroll' | 'collection'; swatches?: OklchColor[]; lum?: number } = { source: 'viewer' }
): void {
  if (opts.swatches) {
    registerAssetPalette(id || 'temp', opts.swatches, opts.lum ?? 0.5);
  }

  if (opts.source === 'viewer') {
    activeViewerAssetId = id;
    applyFocusedAsset(id);
  } else if (opts.source === 'collection') {
    activeDetailAssetId = id;
    if (!activeViewerAssetId) {
      applyFocusedAsset(id);
    }
  } else if (opts.source === 'hover') {
    if (activeViewerAssetId) return; // viewer has top priority
    if (hoverDwellTimer) clearTimeout(hoverDwellTimer);
    if (!id) return;

    // 400 ms dwell requirement (§7.6 #3)
    hoverDwellTimer = setTimeout(() => {
      applyFocusedAsset(id);
    }, 400);
  } else if (opts.source === 'scroll') {
    if (activeViewerAssetId) return;
    const now = performance.now();
    // Sample every 250 ms, hold seed for at least 600 ms (§7.6 #4)
    if (now - lastScrollSeedTimestamp < 250) return;

    const data = id ? assetPaletteCache.get(id) : null;
    if (data && data.swatches.length > 0) {
      const { seed } = selectSeedAndSecondary(data.swatches);
      const hueDiff = Math.abs(seed.h - lastSeedHue);
      const shortestHueDiff = Math.min(hueDiff, 360 - hueDiff);

      if (shortestHueDiff > 14 && now - lastScrollSeedTimestamp >= 600) {
        lastScrollSeedTimestamp = now;
        lastSeedHue = seed.h;
        applyFocusedAsset(id);
      }
    }
  }
}

function applyFocusedAsset(id: string | null) {
  const cached = id ? assetPaletteCache.get(id) : null;
  let swatches: OklchColor[];
  let lum = 0.5;

  if (cached && cached.swatches.length > 0) {
    swatches = cached.swatches;
    lum = cached.lum;
  } else {
    // Neutral fallback palette (§7.2)
    swatches = [
      { l: 0.65, c: 0.12, h: 220, pop: 0.4 },
      { l: 0.75, c: 0.08, h: 260, pop: 0.3 },
      { l: 0.50, c: 0.15, h: 190, pop: 0.2 },
      { l: 0.40, c: 0.02, h: 240, pop: 0.1 }
    ];
  }

  const { seed, secondary } = selectSeedAndSecondary(swatches);
  resolved.palette = { seed, secondary, swatches, lum };

  // Scheme "Follow photo" (§7.6): dark when lum < .45, light when > .55
  if (theme.scheme === 'photo' && theme.appearance === 'adaptive') {
    if (lum < 0.45) {
      resolved.scheme = 'dark';
    } else if (lum > 0.55) {
      resolved.scheme = 'light';
    }
  }

  syncDOMTokens();
}

/**
 * Cycle appearance between Standard -> Adaptive -> Glass (⌥⌘S or ⌘⇧L)
 */
export function cycleAppearance(): void {
  if (theme.appearance === 'standard') {
    theme.appearance = 'adaptive';
  } else if (theme.appearance === 'adaptive') {
    theme.appearance = 'glass';
  } else {
    theme.appearance = 'standard';
  }
  syncDOM();
}

/**
 * Synchronize state changes to document attributes and CSS properties
 */
export function syncDOM(): void {
  if (typeof document === 'undefined') return;

  // Resolve system scheme
  if (theme.scheme === 'system') {
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    resolved.scheme = prefersDark ? 'dark' : 'light';
  } else if (theme.scheme === 'light' || theme.scheme === 'dark') {
    resolved.scheme = theme.scheme;
  }

  // Detect Tier
  resolved.tier = detectGlassTier();

  const root = document.documentElement;
  root.setAttribute('data-appearance', theme.appearance);
  root.setAttribute('data-scheme', resolved.scheme);
  root.setAttribute('data-accent', theme.accent);
  root.setAttribute('data-glass-tier', resolved.tier.toString());
  root.setAttribute('data-reduce-transparency', theme.reduceTransparency.toString());

  // Check high contrast
  const prefersMoreContrast = window.matchMedia('(prefers-contrast: more)').matches;
  root.setAttribute('data-contrast', prefersMoreContrast ? 'more' : 'normal');

  if (theme.reduceTransparency) {
    setForceGovernorLevel('L3');
  }

  syncDOMTokens();

  // Persist to storage
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(theme));
  } catch {
    // ignore
  }
}

function syncDOMTokens(): void {
  if (typeof document === 'undefined') return;
  const root = document.documentElement;

  if (theme.appearance === 'adaptive' && resolved.palette) {
    const isBrowsingOnly = theme.colourSource === 'viewer' && !activeViewerAssetId;
    const intensity = isBrowsingOnly ? 'subtle' : theme.intensity;
    const highContrast = root.getAttribute('data-contrast') === 'more';

    const roles = computeTonalRoles(
      resolved.palette.seed,
      resolved.palette.secondary,
      resolved.scheme,
      intensity,
      highContrast
    );

    // Apply variables to root for smooth CSS transition
    root.style.setProperty('--bg-canvas', roles.bgCanvas);
    root.style.setProperty('--bg-sidebar', roles.bgSidebar);
    root.style.setProperty('--bg-viewer', roles.bgViewer);
    root.style.setProperty('--surface-raised', roles.surfaceRaised);
    root.style.setProperty('--surface-overlay', roles.surfaceOverlay);
    root.style.setProperty('--fill-1', roles.fill1);
    root.style.setProperty('--fill-2', roles.fill2);
    root.style.setProperty('--fill-3', roles.fill3);
    root.style.setProperty('--text-1', roles.text1);
    root.style.setProperty('--text-2', roles.text2);
    root.style.setProperty('--text-3', roles.text3);
    root.style.setProperty('--separator', roles.separator);
    root.style.setProperty('--accent', roles.accent);
    root.style.setProperty('--accent-fg', roles.accentFg);
    root.style.setProperty('--ad-ambient-1', roles.ambient1);
    root.style.setProperty('--ad-ambient-2', roles.ambient2);
  } else if (theme.appearance === 'glass') {
    // Glass Tint = Adaptive (§8.4)
    if (theme.glassTint === 'adaptive' && resolved.palette) {
      const tintColor = oklchToCss(
        resolved.palette.seed.l,
        Math.min(0.12, resolved.palette.seed.c),
        resolved.palette.seed.h
      );
      root.style.setProperty('--glass-tint', tintColor);
      root.style.setProperty('--glass-tint-amt', '12%');
    } else {
      root.style.removeProperty('--glass-tint');
      root.style.removeProperty('--glass-tint-amt');
    }

    // Adaptive foreground (§8.4)
    const glassFg = resolveGlassForeground(resolved.palette?.lum ?? null);
    root.style.setProperty('--glass-fg', glassFg === 'dark' ? '#111113' : '#ffffff');
  } else {
    // In Standard mode, reset inline overrides so standard.css tokens apply cleanly
    root.style.removeProperty('--bg-canvas');
    root.style.removeProperty('--bg-sidebar');
    root.style.removeProperty('--bg-viewer');
    root.style.removeProperty('--surface-raised');
    root.style.removeProperty('--surface-overlay');
    root.style.removeProperty('--fill-1');
    root.style.removeProperty('--fill-2');
    root.style.removeProperty('--fill-3');
    root.style.removeProperty('--text-1');
    root.style.removeProperty('--text-2');
    root.style.removeProperty('--text-3');
    root.style.removeProperty('--separator');
    root.style.removeProperty('--accent');
    root.style.removeProperty('--accent-fg');
    root.style.removeProperty('--ad-ambient-1');
    root.style.removeProperty('--ad-ambient-2');
  }
}

/**
 * Initialize theme listeners on startup
 */
export function initThemeEngine(): () => void {
  if (typeof window === 'undefined') return () => {};

  syncDOM();

  const colorSchemeQuery = window.matchMedia('(prefers-color-scheme: dark)');
  const contrastQuery = window.matchMedia('(prefers-contrast: more)');

  const handleSystemChange = () => {
    if (theme.scheme === 'system') {
      syncDOM();
    }
  };

  const handleContrastChange = () => {
    syncDOM();
  };

  colorSchemeQuery.addEventListener('change', handleSystemChange);
  contrastQuery.addEventListener('change', handleContrastChange);

  // Keyboard shortcut ⌘⇧L to cycle appearance (§5.4)
  const handleKeydown = (e: KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === 'l') {
      e.preventDefault();
      cycleAppearance();
    }
  };

  window.addEventListener('keydown', handleKeydown);

  // Initial seed
  applyFocusedAsset(null);

  return () => {
    colorSchemeQuery.removeEventListener('change', handleSystemChange);
    contrastQuery.removeEventListener('change', handleContrastChange);
    window.removeEventListener('keydown', handleKeydown);
  };
}
