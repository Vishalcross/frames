import type { Asset, AssetDetail, AssetRepo, OklchColor, Query } from '$lib/types';
import { registerAssetPalette } from '$lib/theme/engine.svelte';

/**
 * High-performance deterministic mock repository serving 100,000 assets (§14, M1)
 * Uses deterministic generation so 100k assets consume negligible memory and query in < 2ms.
 */

const TOTAL_MOCK_ASSETS = 100_000;

// Base timestamps for year 2018 to 2026 (newest at bottom/latest timestamp)
const START_TIMESTAMP = new Date('2018-01-01T00:00:00Z').getTime();
const END_TIMESTAMP = new Date('2026-10-30T12:00:00Z').getTime();
const TIME_SPAN = END_TIMESTAMP - START_TIMESTAMP;

// Diverse palette themes to make the gallery visually stunning
const PALETTE_THEMES: { name: string; swatches: OklchColor[]; lum: number }[] = [
  {
    name: 'Sunset Ocean',
    swatches: [
      { l: 0.68, c: 0.18, h: 45, pop: 0.45 },
      { l: 0.52, c: 0.14, h: 215, pop: 0.30 },
      { l: 0.82, c: 0.12, h: 75, pop: 0.15 },
      { l: 0.35, c: 0.08, h: 250, pop: 0.10 }
    ],
    lum: 0.58
  },
  {
    name: 'Alpine Forest',
    swatches: [
      { l: 0.48, c: 0.15, h: 145, pop: 0.50 },
      { l: 0.65, c: 0.10, h: 120, pop: 0.25 },
      { l: 0.32, c: 0.06, h: 160, pop: 0.15 },
      { l: 0.80, c: 0.04, h: 100, pop: 0.10 }
    ],
    lum: 0.44
  },
  {
    name: 'Golden Hour Architecture',
    swatches: [
      { l: 0.72, c: 0.16, h: 65, pop: 0.40 },
      { l: 0.58, c: 0.12, h: 35, pop: 0.35 },
      { l: 0.42, c: 0.05, h: 260, pop: 0.15 },
      { l: 0.88, c: 0.03, h: 80, pop: 0.10 }
    ],
    lum: 0.64
  },
  {
    name: 'Nordic Blue Winter',
    swatches: [
      { l: 0.78, c: 0.09, h: 220, pop: 0.50 },
      { l: 0.60, c: 0.14, h: 245, pop: 0.25 },
      { l: 0.92, c: 0.02, h: 210, pop: 0.15 },
      { l: 0.38, c: 0.08, h: 260, pop: 0.10 }
    ],
    lum: 0.72
  },
  {
    name: 'Moody Dusk & City Lights',
    swatches: [
      { l: 0.38, c: 0.12, h: 290, pop: 0.45 },
      { l: 0.65, c: 0.20, h: 25, pop: 0.25 },
      { l: 0.22, c: 0.06, h: 270, pop: 0.20 },
      { l: 0.78, c: 0.18, h: 50, pop: 0.10 }
    ],
    lum: 0.35
  },
  {
    name: 'Mediterranean Summer',
    swatches: [
      { l: 0.65, c: 0.19, h: 200, pop: 0.40 },
      { l: 0.85, c: 0.15, h: 90, pop: 0.30 },
      { l: 0.55, c: 0.22, h: 25, pop: 0.20 },
      { l: 0.95, c: 0.02, h: 200, pop: 0.10 }
    ],
    lum: 0.68
  },
  {
    name: 'Desert Red Rock',
    swatches: [
      { l: 0.56, c: 0.17, h: 38, pop: 0.50 },
      { l: 0.42, c: 0.13, h: 20, pop: 0.25 },
      { l: 0.75, c: 0.11, h: 70, pop: 0.15 },
      { l: 0.28, c: 0.08, h: 15, pop: 0.10 }
    ],
    lum: 0.49
  },
  {
    name: 'Minimal Monochrome',
    swatches: [
      { l: 0.88, c: 0.005, h: 240, pop: 0.50 },
      { l: 0.45, c: 0.008, h: 240, pop: 0.30 },
      { l: 0.20, c: 0.005, h: 240, pop: 0.15 },
      { l: 0.65, c: 0.006, h: 240, pop: 0.05 }
    ],
    lum: 0.52
  }
];

const LOCATIONS = [
  'Big Sur, California',
  'Kyoto, Japan',
  'Amalfi Coast, Italy',
  'Reykjavík, Iceland',
  'Dolomites, Italy',
  'Banff National Park, Canada',
  'Zion National Park, Utah',
  'Paris, France',
  'Swiss Alps, Zermatt',
  'Santorini, Greece',
  'Copenhagen, Denmark',
  'Yosemite Valley, California',
  'Queenstown, New Zealand',
  'Scottish Highlands',
  'Barcelona, Spain'
];

const CAMERAS = [
  { make: 'Apple', model: 'iPhone 16 Pro', lens: '24mm f/1.78', exp: 'f/1.8 · 1/120 · ISO 64' },
  { make: 'Sony', model: 'α7 IV', lens: 'FE 24-70mm F2.8 GM II', exp: 'f/2.8 · 1/250 · ISO 100' },
  { make: 'Fujifilm', model: 'X-T5', lens: 'XF 23mm f/1.4 R LM WR', exp: 'f/2.0 · 1/500 · ISO 160' },
  { make: 'Leica', model: 'Q3', lens: 'Summilux 28mm f/1.7 ASPH', exp: 'f/1.7 · 1/400 · ISO 100' },
  { make: 'Canon', model: 'EOS R5', lens: 'RF 50mm f/1.2 L USM', exp: 'f/1.4 · 1/1000 · ISO 50' },
  { make: 'Hasselblad', model: 'X2D 100C', lens: 'XCD 38mm f/2.5 V', exp: 'f/4.0 · 1/160 · ISO 64' }
];

// In-memory overrides for user interactions (favorites, deletes, captions)
const userOverrides = new Map<string, Partial<Asset & { caption?: string; albums?: string[] }>>();

// Fast deterministic PRNG for index
function pseudoRandom(seed: number): number {
  const x = Math.sin(seed * 9999 + 1) * 10000;
  return x - Math.floor(x);
}

/**
 * Generate a deterministic Asset object by index
 */
export function getAssetByIndex(index: number): Asset {
  const id = `asset_${index.toString().padStart(6, '0')}`;
  const override = userOverrides.get(id);

  const r1 = pseudoRandom(index * 1.31);
  const r2 = pseudoRandom(index * 2.73);
  const r3 = pseudoRandom(index * 4.19);
  const r4 = pseudoRandom(index * 7.43);

  // Time distribution: sorted chronologically from oldest (0) to newest (TOTAL - 1)
  const takenAt = Math.round(START_TIMESTAMP + (index / (TOTAL_MOCK_ASSETS - 1)) * TIME_SPAN);
  const date = new Date(takenAt);
  const year = date.getUTCFullYear();
  const month = date.getUTCMonth() + 1;

  const isVideo = r1 < 0.14; // ~14% videos
  const isFavorite = r2 < 0.08; // ~8% favorites
  const isCloudOnly = r3 < 0.10; // ~10% cloud-only (in S4)

  // Realistic aspect ratios (4:3, 3:2, 16:9, portrait variants)
  const aspectChoices = [
    { w: 4032, h: 3024 }, // 4:3 landscape
    { w: 3024, h: 4032 }, // 3:4 portrait
    { w: 6000, h: 4000 }, // 3:2 landscape
    { w: 4000, h: 6000 }, // 2:3 portrait
    { w: 3840, h: 2160 }, // 16:9
    { w: 3000, h: 3000 }  // 1:1
  ];
  const aspect = aspectChoices[Math.floor(r4 * aspectChoices.length)];

  const themeIdx = Math.floor(r1 * PALETTE_THEMES.length);
  const theme = PALETTE_THEMES[themeIdx];

  const locIdx = Math.floor(r2 * LOCATIONS.length);
  const location = LOCATIONS[locIdx];

  const durationMs = isVideo ? Math.round(5000 + r3 * 85000) : undefined;

  const baseAsset: Asset = {
    id,
    kind: isVideo ? 'video' : 'photo',
    takenAt,
    w: aspect.w,
    h: aspect.h,
    durationMs,
    thumbHash: `th_${themeIdx}_${(r3 * 1000).toFixed(0)}`,
    palette: theme.swatches,
    lum: theme.lum,
    favorite: isFavorite,
    hidden: false,
    originalLocal: !isCloudOnly,
    title: `IMG_${(index % 9999 + 1).toString().padStart(4, '0')}`,
    location,
    year,
    month
  };

  const asset = override ? { ...baseAsset, ...override } : baseAsset;

  // Register palette with the theme engine
  if (asset.palette) {
    registerAssetPalette(asset.id, asset.palette, asset.lum ?? 0.5);
  }

  return asset;
}

/**
 * Generate deterministic thumbnail SVG/CSS background
 */
export function getThumbnailDataUrl(asset: Asset, size: 'thumb' | 'preview' | 'original' = 'thumb'): string {
  const p = asset.palette || PALETTE_THEMES[0].swatches;
  const s1 = p[0] || { l: 0.6, c: 0.15, h: 220, pop: 1 };
  const s2 = p[1] || { l: 0.4, c: 0.12, h: 40, pop: 0.5 };
  const s3 = p[2] || { l: 0.7, c: 0.10, h: 140, pop: 0.3 };

  const c1 = `oklch(${(s1.l * 100).toFixed(0)}% ${s1.c.toFixed(2)} ${s1.h.toFixed(0)})`;
  const c2 = `oklch(${(s2.l * 100).toFixed(0)}% ${s2.c.toFixed(2)} ${s2.h.toFixed(0)})`;
  const c3 = `oklch(${(s3.l * 100).toFixed(0)}% ${s3.c.toFixed(2)} ${s3.h.toFixed(0)})`;

  const w = size === 'thumb' ? 256 : size === 'preview' ? 1024 : asset.w;
  const h = Math.round((w * asset.h) / asset.w);

  // High quality SVG image that renders immediately without network or IPC lag
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="100%" height="100%">
    <defs>
      <linearGradient id="g_${asset.id}" x1="0%" y1="0%" x2="100%" y2="100%">
        <stop offset="0%" stop-color="${c1}" />
        <stop offset="55%" stop-color="${c2}" />
        <stop offset="100%" stop-color="${c3}" />
      </linearGradient>
      <radialGradient id="r_${asset.id}" cx="60%" cy="40%" r="50%">
        <stop offset="0%" stop-color="${c1}" stop-opacity="0.85" />
        <stop offset="100%" stop-color="${c2}" stop-opacity="0" />
      </radialGradient>
    </defs>
    <rect width="${w}" height="${h}" fill="url(#g_${asset.id})" />
    <circle cx="${Math.round(w * 0.6)}" cy="${Math.round(h * 0.4)}" r="${Math.round(Math.min(w, h) * 0.45)}" fill="url(#r_${asset.id})" />
    <path d="M 0 ${Math.round(h * 0.75)} Q ${Math.round(w * 0.3)} ${Math.round(h * 0.65)}, ${Math.round(w * 0.6)} ${Math.round(h * 0.8)} T ${w} ${Math.round(h * 0.7)} L ${w} ${h} L 0 ${h} Z" fill="${c2}" opacity="0.65" />
    <path d="M 0 ${Math.round(h * 0.85)} Q ${Math.round(w * 0.4)} ${Math.round(h * 0.78)}, ${Math.round(w * 0.7)} ${Math.round(h * 0.9)} T ${w} ${Math.round(h * 0.82)} L ${w} ${h} L 0 ${h} Z" fill="${c3}" opacity="0.8" />
  </svg>`;

  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

/**
 * Filter matching helper
 */
function matchesQuery(asset: Asset, q: Query): boolean {
  if (asset.deletedAt) {
    return q.scope === 'trash';
  }
  if (q.scope === 'trash') {
    return !!asset.deletedAt;
  }
  if (q.scope === 'hidden') {
    return asset.hidden;
  }
  if (asset.hidden && q.scope !== 'hidden') {
    return false;
  }

  if (q.filter === 'favorites' || q.scope === 'favorites') {
    if (!asset.favorite) return false;
  }
  if (q.filter === 'videos' || q.scope === 'videos') {
    if (asset.kind !== 'video') return false;
  }
  if (q.filter === 'photos') {
    if (asset.kind !== 'photo') return false;
  }
  if (q.year && asset.year !== q.year) {
    return false;
  }
  if (q.month && asset.month !== q.month) {
    return false;
  }
  if (q.search) {
    const s = q.search.toLowerCase();
    const matchName = asset.title?.toLowerCase().includes(s);
    const matchLoc = asset.location?.toLowerCase().includes(s);
    const matchYear = asset.year?.toString().includes(s);
    if (!matchName && !matchLoc && !matchYear) return false;
  }
  return true;
}

/**
 * The Mock Asset Repository Implementation (§14)
 */
export class MockAssetRepo implements AssetRepo {
  private total = TOTAL_MOCK_ASSETS;

  async count(q: Query): Promise<number> {
    if (!q.filter && !q.scope && !q.year && !q.month && !q.search) {
      return this.total;
    }
    if (q.scope === 'all' || !q.scope) {
      if (q.year && !q.month && !q.filter && !q.search) {
        // Approximate division by 9 years
        return Math.round(this.total / 9);
      }
      if (q.year && q.month && !q.filter && !q.search) {
        return Math.round(this.total / (9 * 12));
      }
    }

    // For filtered scopes, sample or count up to 10k items
    let matches = 0;
    const sampleLimit = Math.min(this.total, 6000);
    for (let i = 0; i < sampleLimit; i++) {
      const asset = getAssetByIndex(i);
      if (matchesQuery(asset, q)) matches++;
    }
    return Math.round((matches / sampleLimit) * this.total);
  }

  async range(q: Query, from: number, n: number): Promise<Asset[]> {
    const results: Asset[] = [];
    const isDefaultScope = (!q.scope || q.scope === 'all') && !q.filter && !q.year && !q.month && !q.search;

    if (isDefaultScope) {
      const start = Math.max(0, from);
      const end = Math.min(this.total, start + n);
      for (let i = start; i < end; i++) {
        results.push(getAssetByIndex(i));
      }
      return results;
    }

    // Query with filter/scope
    let index = 0;
    let found = 0;
    while (index < this.total && results.length < n) {
      const asset = getAssetByIndex(index);
      if (matchesQuery(asset, q)) {
        if (found >= from) {
          results.push(asset);
        }
        found++;
      }
      index++;
    }

    return results;
  }

  async detail(id: string): Promise<AssetDetail> {
    const indexNum = parseInt(id.replace('asset_', ''), 10) || 0;
    const asset = getAssetByIndex(indexNum);
    const camIdx = indexNum % CAMERAS.length;
    const cam = CAMERAS[camIdx];

    const sizeMb = asset.kind === 'video' ? (24 + (indexNum % 80)).toFixed(1) : (2.4 + (indexNum % 9)).toFixed(1);

    return {
      record: asset,
      caption: (asset as any).caption || `Captured in ${asset.location}`,
      camera: `${cam.make} ${cam.model}`,
      lens: cam.lens,
      exposure: cam.exp,
      dimensions: `${asset.w} × ${asset.h}`,
      fileSize: `${sizeMb} MB`,
      filename: `${asset.title}.${asset.kind === 'video' ? 'MOV' : 'HEIC'}`,
      locationName: asset.location || 'Unknown location',
      albums: ['Summer Trip', 'Favorites', 'Wallpapers'],
      keywords: [asset.kind, 'Travel', 'Nature', 'Landscape', 'Tahoe']
    };
  }

  async updateFavorite(id: string, favorite: boolean): Promise<void> {
    const existing = userOverrides.get(id) || {};
    userOverrides.set(id, { ...existing, favorite });
  }

  async updateDeleted(id: string, deleted: boolean): Promise<void> {
    const existing = userOverrides.get(id) || {};
    userOverrides.set(id, { ...existing, deletedAt: deleted ? Date.now() : undefined });
  }

  async updateCaption(id: string, caption: string): Promise<void> {
    const existing = userOverrides.get(id) || {};
    userOverrides.set(id, { ...existing, caption });
  }

  async updateTakenAt(id: string, takenAt: number): Promise<void> {
    const existing = userOverrides.get(id) || {};
    const d = new Date(takenAt);
    userOverrides.set(id, { ...existing, takenAt, year: d.getUTCFullYear(), month: d.getUTCMonth() + 1 });
  }
}

export const mockRepo = new MockAssetRepo();
