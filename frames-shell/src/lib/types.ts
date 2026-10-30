export type Appearance = 'standard' | 'adaptive' | 'glass';
export type Scheme = 'system' | 'light' | 'dark' | 'photo';
export type ResolvedScheme = 'light' | 'dark';
export type GlassTier = 1 | 2;
export type AdaptiveIntensity = 'subtle' | 'balanced' | 'vivid';
export type ColourSource = 'viewer' | 'both';
export type GlassTint = 'neutral' | 'adaptive';
export type GlassClarity = 'regular' | 'clear';

export interface OklchColor {
  l: number; // 0..1
  c: number; // 0..0.4
  h: number; // 0..360
  pop: number; // population weight 0..1
}

export interface Palette {
  seed: OklchColor;
  secondary: OklchColor;
  swatches: OklchColor[];
  lum: number; // 0..1
}

export interface ThemeConfig {
  appearance: Appearance;
  scheme: Scheme;
  accent: string; // for Standard mode: Blue, Purple, Pink, Red, Orange, Yellow, Green, Graphite
  intensity: AdaptiveIntensity;
  colourSource: ColourSource;
  glassTint: GlassTint;
  glassClarity: GlassClarity;
  reduceTransparency: boolean;
}

export interface Asset {
  id: string;
  kind: 'photo' | 'video';
  takenAt: number; // timestamp ms
  w: number;
  h: number;
  durationMs?: number;
  thumbHash: string;
  palette: OklchColor[] | null;
  lum: number | null; // 0..1 mean luminance
  favorite: boolean;
  hidden: boolean;
  deletedAt?: number;
  originalLocal: boolean; // false = still in S4, show cloud badge
  title?: string;
  location?: string;
  year?: number;
  month?: number; // 1..12
}

export interface AssetDetail {
  record: Asset;
  caption: string;
  camera: string;
  lens: string;
  exposure: string; // e.g. "f/1.8 · 1/120 · ISO 64"
  dimensions: string;
  fileSize: string;
  filename: string;
  locationName: string;
  albums: string[];
  keywords: string[];
}

export interface Query {
  scope?: string; // 'all' | 'years' | 'months' | 'favorites' | 'videos' | 'trash' | 'hidden' | 'imports' | 'album' | 'search' | 'people' | 'memories' | 'trips' | 'captured-by-me';
  albumId?: string;
  personId?: string;
  tripId?: string;
  memoryId?: string;
  search?: string;
  year?: number;
  month?: number; // 1..12
  filter?: 'all' | 'favorites' | 'videos' | 'photos';
}

export interface AssetRepo {
  count(q: Query): Promise<number>;
  range(q: Query, from: number, n: number): Promise<Asset[]>;
  detail(id: string): Promise<AssetDetail>;
  updateFavorite(id: string, favorite: boolean): Promise<void>;
  updateDeleted(id: string, deleted: boolean): Promise<void>;
  updateCaption(id: string, caption: string): Promise<void>;
  updateTakenAt(id: string, takenAt: number): Promise<void>;
}

export interface AlbumItem {
  id: string;
  title: string;
  assetCount: number;
  keyAssetId: string;
  dateRange: string;
  isFolder?: boolean;
  children?: AlbumItem[];
}

export interface CollectionCard {
  id: string;
  title: string;
  subtitle?: string;
  count: number;
  keyAssetId: string;
  type: 'days' | 'memories' | 'people' | 'trips' | 'albums' | 'media-types' | 'utilities';
}

export interface SyncQueueItem {
  id: string;
  filename: string;
  progress: number; // 0..100
  totalBytes: number;
  uploadedBytes: number;
  status: 'uploading' | 'queued' | 'paused';
}
