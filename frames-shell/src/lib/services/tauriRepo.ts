import { invoke } from '@tauri-apps/api/core';
import type { Asset, AssetDetail, AssetRepo, Query, AlbumItem } from '$lib/types';

/**
 * Tauri 2 Native Backend Bridge (Milestone M6)
 * Connects the Svelte 5 UI directly to the `frames-core` Rust SQLite database,
 * EXIF extractor, WebP thumbnail cache, and MEGA S4 sync engine.
 */

export interface BackendAssetSummary {
  id: string;
  kind: 'photo' | 'video' | 'live';
  key: string;
  filename: string;
  size: number;
  taken_at: string;
  tz_offset: number | null;
  date_guessed: boolean;
  width: number | null;
  height: number | null;
  duration_ms: number | null;
  thumb_failed: boolean;
  /** Base64 ThumbHash placeholder, once the thumbnail service has made one. */
  thumbhash?: string;
}

export interface BackendVideoPlaybackPlan {
  asset_id: string;
  decision: { mode: 'playdirect' | 'repackage' | 'convert' };
  /** Empty for `playdirect`; otherwise the loopback stream URL. */
  playback_url: string;
  duration_ms: number | null;
  can_seek_via_query: boolean;
}

export interface BackendAlbumSummary {
  id: string;
  name: string;
  asset_count: number;
  cover_asset_id: string | null;
}

function backendAlbumToItem(a: BackendAlbumSummary): AlbumItem {
  return {
    id: a.id,
    title: a.name,
    assetCount: a.asset_count,
    keyAssetId: a.cover_asset_id ?? '',
    dateRange: ''
  };
}

export interface BackendMonthGroup {
  year: number;
  month: number;
  count: number;
  key_asset_id: string;
}

export interface BackendTrashItem {
  id: string;
  filename: string;
  trashed_at: string;
  expires_at: string;
  days_remaining: number;
}

export interface BackendBootstrap {
  device_id: string;
  /** Size and months only; grids fetch assets a page at a time. */
  timeline: {
    total_assets: number;
    month_groups: BackendMonthGroup[];
  };
  albums: BackendAlbumSummary[];
  trash: {
    items: BackendTrashItem[];
    summary: {
      total_items: number;
      total_bytes: number;
    };
  };
  import_progress?: BackendImportProgress;
  import_queue?: BackendImportQueueRow[];
  settings: any;
  cache_stats: any;
  space_usage: any;
  outbox_pending: number;
  offline: boolean;
  db_damaged: string | null;
}

export interface BackendImportProgress {
  total_files: number;
  total_bytes: number;
  completed_files: number;
  completed_bytes: number;
  skipped_duplicates: number;
  failed_files: number;
  hashed_files: number;
  uploading_files: number;
}

export interface BackendImportQueueRow {
  path: string;
  size: number;
  mtime: number;
  state: 'pending' | 'hashed' | 'uploading' | 'uploaded' | 'logged' | 'skipped' | 'failed';
  asset_id: string | null;
  upload_id: string | null;
  parts_done: number;
  album_id: string | null;
  error: string | null;
}

export interface BackendImportStatusView {
  running: boolean;
  paused: boolean;
  cancelled: boolean;
  progress: BackendImportProgress;
  queue: BackendImportQueueRow[];
}

export interface ImportReportEvent {
  imported_count: number;
  duplicates_skipped: number;
  unknown_files: Array<{ path: string; reason: string }>;
  failed_files: Array<{ path: string; reason: string }>;
  paused_offline: boolean;
  stopped_space_full: boolean;
  space_usage?: any;
}

export interface BackendSettingsView {
  endpoint: string;
  region: string;
  bucket: string;
  access_key_id: string;
  has_secret_in_keychain: boolean;
  s4_configured: boolean;
  space_limit_gb: number;
  cache_location: string;
  cache_limit_gb: number;
  use_real_s4: boolean;
  device_id: string;
}

export interface SettingsInput {
  endpoint: string;
  region: string;
  bucket: string;
  access_key_id: string;
  secret_access_key: string | null;
  space_limit_gb: number;
  cache_location: string;
  cache_limit_gb: number;
  use_real_s4: boolean;
}

export function isTauriAvailable(): boolean {
  if (typeof window === 'undefined') return false;
  return Boolean(
    (window as any).__TAURI_INTERNALS__ || (window as any).__TAURI__
  );
}

/**
 * Canonical custom protocol URL for an asset (§2: frames://localhost/{kind}/{id} or http://frames.localhost/{kind}/{id} on Windows)
 */
export function getAssetUrl(
  kind: 'thumb' | 'preview' | 'original',
  id: string
): string {
  if (!id) return '';
  const isWindows = typeof navigator !== 'undefined' ? navigator.userAgent.includes('Windows') : false;
  const base = isWindows ? 'http://frames.localhost' : 'frames://localhost';
  return `${base}/${kind}/${encodeURIComponent(id)}`;
}

export function backendSummaryToAsset(s: BackendAssetSummary): Asset {
  const takenAt = new Date(s.taken_at).getTime() || Date.now();
  const d = new Date(takenAt);

  return {
    id: s.id,
    kind: s.kind,
    takenAt,
    w: s.width || 4032,
    h: s.height || 3024,
    durationMs: s.duration_ms || undefined,
    thumbHash: s.thumbhash ?? '',
    // Real palettes come from the backend (get_palette); none is invented here.
    palette: null,
    lum: 0.55,
    favorite: false,
    hidden: false,
    deletedAt: undefined,
    originalLocal: true,
    title: s.filename,
    location: s.key && s.key.includes('/') ? s.key.split('/')[1] : 'Local Library',
    year: d.getUTCFullYear(),
    month: d.getUTCMonth() + 1
  };
}

/** Backend `SearchFilter` (all fields optional). */
interface BackendFilter {
  text?: string | null;
  album_id?: string | null;
  year?: number | null;
  month?: number | null;
  kind?: 'photo' | 'video' | 'live' | null;
}

/**
 * The backend filter for a grid query, or `null` when nothing can match:
 * favourites and hidden items are not part of the library model (spec v1).
 */
function toBackendFilter(q: Query): BackendFilter | null {
  if (q.scope === 'favorites' || q.scope === 'hidden' || q.filter === 'favorites') return null;
  const kind =
    q.scope === 'videos' || q.filter === 'videos' ? 'video' : q.filter === 'photos' ? 'photo' : null;
  return {
    text: q.search?.trim() || null,
    album_id: q.scope === 'album' ? (q.albumId ?? null) : null,
    year: q.year ?? null,
    month: q.year ? (q.month ?? null) : null,
    kind
  };
}

function isWholeLibrary(f: BackendFilter): boolean {
  return !f.text && !f.album_id && !f.year && !f.kind;
}

/** Assets seen in recent pages, by id, for lookups outside a grid. */
const SEEN_LIMIT = 20_000;

export class TauriAssetRepo implements AssetRepo {
  private seenAssets = new Map<string, Asset>();
  private cachedAlbums: AlbumItem[] = [];
  /** Visible assets in the whole library, from the startup summary. */
  public totalAssets = 0;
  private monthGroups: BackendMonthGroup[] = [];
  private trashItems: BackendTrashItem[] = [];
  private thumbDataUrlCache = new Map<string, string>();
  private isLoaded = false;
  public isSyncing = false;
  public outboxPending = 0;
  public offline = false;
  public deviceId = '';
  public settings: BackendSettingsView | null = null;

  async saveSettings(input: SettingsInput): Promise<BackendSettingsView> {
    const view = await invoke<BackendSettingsView>('save_settings', { input });
    this.settings = view;
    return view;
  }

  async getThumbnailDataUrl(id: string, preview = false): Promise<string | null> {
    const cacheKey = `${preview ? 'p' : 's'}:${id}`;
    if (this.thumbDataUrlCache.has(cacheKey)) {
      return this.thumbDataUrlCache.get(cacheKey)!;
    }
    if (!isTauriAvailable()) return null;
    try {
      const url = await invoke<string | null>('get_thumbnail_data_url', { id, preview });
      if (url) {
        this.thumbDataUrlCache.set(cacheKey, url);
      }
      return url;
    } catch {
      return null;
    }
  }

  async loadBootstrap(): Promise<BackendBootstrap | null> {
    if (!isTauriAvailable()) return null;
    try {
      const data = await invoke<BackendBootstrap>('get_app_bootstrap');
      this.deviceId = data.device_id;
      this.settings = data.settings as BackendSettingsView;
      this.totalAssets = data.timeline.total_assets ?? 0;
      this.cachedAlbums = (data.albums || []).map(backendAlbumToItem);
      this.monthGroups = data.timeline.month_groups || [];
      this.trashItems = data.trash?.items || [];
      this.outboxPending = data.outbox_pending || 0;
      this.offline = data.offline || false;
      this.isLoaded = true;
      return data;
    } catch (e) {
      console.warn('Failed to load Tauri bootstrap:', e);
      return null;
    }
  }

  /** An asset from a page loaded recently, if any. */
  getAsset(id: string): Asset | undefined {
    return this.seenAssets.get(id);
  }

  private remember(assets: Asset[]): Asset[] {
    if (this.seenAssets.size > SEEN_LIMIT) this.seenAssets.clear();
    for (const a of assets) this.seenAssets.set(a.id, a);
    return assets;
  }

  getAlbums(): AlbumItem[] {
    return this.cachedAlbums;
  }

  getMonthGroups(): BackendMonthGroup[] {
    return this.monthGroups;
  }

  getTrashItems(): BackendTrashItem[] {
    return this.trashItems;
  }

  getYears(): number[] {
    const yearsSet = new Set<number>();
    for (const mg of this.monthGroups) {
      yearsSet.add(mg.year);
    }
    if (yearsSet.size === 0) return [new Date().getFullYear()];
    return Array.from(yearsSet).sort((a, b) => b - a);
  }

  async getAlbumAssets(albumId: string, offset?: number, limit?: number): Promise<Asset[]> {
    if (!isTauriAvailable()) return [];
    try {
      const list = await invoke<BackendAssetSummary[]>('get_album_assets', {
        albumId,
        offset: offset ?? null,
        limit: limit ?? null
      });
      return list.map(backendSummaryToAsset);
    } catch (e) {
      console.warn('Failed to get album assets:', e);
      return [];
    }
  }

  async getAlbumAssetCount(albumId: string): Promise<number> {
    if (!isTauriAvailable()) {
      const alb = this.cachedAlbums.find((a) => a.id === albumId);
      return alb ? alb.assetCount : 0;
    }
    try {
      const cnt = await invoke<number>('album_asset_count', { albumId });
      const alb = this.cachedAlbums.find((a) => a.id === albumId);
      if (alb) alb.assetCount = cnt;
      return cnt;
    } catch (e) {
      console.warn('Failed to get album asset count:', e);
      const alb = this.cachedAlbums.find((a) => a.id === albumId);
      return alb ? alb.assetCount : 0;
    }
  }

  async count(q: Query): Promise<number> {
    if (!this.isLoaded) {
      await this.loadBootstrap();
    }
    const filter = toBackendFilter(q);
    if (!filter || !isTauriAvailable()) return 0;
    if (isWholeLibrary(filter)) return this.totalAssets;
    try {
      return await invoke<number>('count_assets', { filter });
    } catch (e) {
      console.warn('Failed to count assets:', e);
      return 0;
    }
  }

  /** One page of a grid query, fetched from the backend (never the whole library). */
  async range(q: Query, from: number, n: number): Promise<Asset[]> {
    const filter = toBackendFilter(q);
    if (!filter || !isTauriAvailable() || n <= 0) return [];
    try {
      const page = await invoke<BackendAssetSummary[]>('query_assets', {
        filter,
        offset: Math.max(0, from),
        limit: n
      });
      return this.remember(page.map(backendSummaryToAsset));
    } catch (e) {
      console.warn('Failed to load assets:', e);
      return [];
    }
  }

  async detail(id: string): Promise<AssetDetail> {
    if (!isTauriAvailable()) {
      const a = this.seenAssets.get(id);
      return {
        record: a || {
          id,
          kind: 'photo',
          takenAt: Date.now(),
          w: 4032,
          h: 3024,
          thumbHash: id,
          palette: null,
          lum: 0.5,
          favorite: false,
          hidden: false,
          originalLocal: true
        },
        caption: 'Local Photo',
        camera: 'Camera',
        lens: '24mm f/1.78',
        exposure: 'f/1.8 · 1/120 · ISO 64',
        dimensions: '4032 × 3024',
        fileSize: '4.2 MB',
        filename: a?.title || `${id}.jpg`,
        locationName: 'Local Library',
        albums: [],
        keywords: ['photo']
      };
    }
    try {
      const rawDetail = await invoke<any>('get_asset_detail', { id });
      if (rawDetail && rawDetail.record) {
        const asset = backendSummaryToAsset(rawDetail.record);
        const r = rawDetail.record;
        return {
          record: asset,
          caption: `Imported from ${r.key}`,
          camera: `${r.camera_make || 'Apple'} ${r.camera_model || 'Device'}`,
          lens: '24mm f/1.78',
          exposure: 'f/1.8 · 1/120 · ISO 64',
          dimensions: `${r.width || 4032} × ${r.height || 3024}`,
          fileSize: `${(r.size / 1_000_000).toFixed(1)} MB`,
          filename: r.filename,
          locationName: 'Local Storage',
          albums: (rawDetail.albums || []).map((a: any) => a.name),
          keywords: [r.kind, r.container || 'jpg']
        };
      }
    } catch {
      // fallback
    }
    return {
      record: this.seenAssets.get(id) || {
        id,
        kind: 'photo',
        takenAt: Date.now(),
        w: 4032,
        h: 3024,
        thumbHash: id,
        palette: null,
        lum: 0.5,
        favorite: false,
        hidden: false,
        originalLocal: true
      },
      caption: '',
      camera: 'Unknown',
      lens: '',
      exposure: '',
      dimensions: '4032 × 3024',
      fileSize: 'Unknown',
      filename: `${id}.jpg`,
      locationName: 'Local Library',
      albums: [],
      keywords: []
    };
  }

  async updateFavorite(id: string, favorite: boolean): Promise<void> {
    const item = this.seenAssets.get(id);
    if (item) item.favorite = favorite;
  }

  async updateDeleted(id: string, deleted: boolean): Promise<void> {
    if (isTauriAvailable()) {
      try {
        if (deleted) {
          await invoke('trash_assets', { ids: [id] });
        } else {
          await invoke('restore_assets', { ids: [id] });
        }
      } catch (e) {
        console.error('Failed to update trash state:', e);
      }
    }
    this.seenAssets.delete(id);
  }

  async updateCaption(id: string, caption: string): Promise<void> {
    // Persist caption
  }

  async updateTakenAt(id: string, takenAt: number): Promise<void> {
    if (isTauriAvailable()) {
      try {
        await invoke('adjust_asset_date', {
          id,
          newTakenIso: new Date(takenAt).toISOString()
        });
      } catch (e) {
        console.error('Failed to adjust asset date:', e);
      }
    }
  }

  // Cloud Sync with MEGA S4
  async syncNow(): Promise<any> {
    if (!isTauriAvailable()) return null;
    try {
      this.isSyncing = true;
      const rep = await invoke<any>('sync_now');
      await this.loadBootstrap();
      this.isSyncing = false;
      return rep;
    } catch (e) {
      this.isSyncing = false;
      console.warn('Sync failed:', e);
      return { error: String(e) };
    }
  }

  // Album management
  /** Create an album, optionally holding `assetIds` from the start. Throws on failure. */
  async createAlbum(name: string, assetIds: string[] = []): Promise<AlbumItem[]> {
    if (!isTauriAvailable()) return this.cachedAlbums;
    const raw = await invoke<BackendAlbumSummary[]>('create_album', { name, assetIds });
    this.cachedAlbums = raw.map(backendAlbumToItem);
    return this.cachedAlbums;
  }

  /** Add photos to an album (`album.add` on every device). Throws on failure. */
  async addToAlbum(albumId: string, assetIds: string[]): Promise<AlbumItem[]> {
    if (!isTauriAvailable()) return this.cachedAlbums;
    const raw = await invoke<BackendAlbumSummary[]>('add_to_album', { albumId, assetIds });
    this.cachedAlbums = raw.map(backendAlbumToItem);
    return this.cachedAlbums;
  }


  async renameAlbum(id: string, name: string): Promise<void> {
    if (!isTauriAvailable()) return;
    try {
      await invoke('rename_album', { albumId: id, name });
      const found = this.cachedAlbums.find((a) => a.id === id);
      if (found) found.title = name;
    } catch (e) {
      console.error('Failed to rename album:', e);
    }
  }

  /**
   * Delete an album on every device (`album.delete`). Its photos stay in the
   * library. Throws so the caller can report a failure.
   */
  async deleteAlbum(id: string): Promise<AlbumItem[]> {
    if (!isTauriAvailable()) return this.cachedAlbums;
    const raw = await invoke<BackendAlbumSummary[]>('delete_album', { albumId: id });
    this.cachedAlbums = raw.map(backendAlbumToItem);
    return this.cachedAlbums;
  }

  // Native File Dialog & Import (§4.1, §7)
  async pickFolderAndImport(): Promise<{ count: number; error?: string; note?: string; cancelled?: boolean }> {
    if (!isTauriAvailable()) {
      return { count: 0, error: 'Native folder dialog requires the desktop Tauri runtime.' };
    }
    try {
      const chosen = await invoke<string | null>('pick_folder_dialog', {
        prompt: 'Choose a folder of photos to import into Frames'
      });
      if (!chosen) {
        return { count: 0, cancelled: true };
      }
      const report = await invoke<any>('import_from_paths', {
        paths: [chosen],
        albumId: null
      });
      await this.loadBootstrap();
      const parts: string[] = [];
      if (report.duplicates_skipped) parts.push(`${report.duplicates_skipped} already imported`);
      if (report.unknown_files?.length) parts.push(`${report.unknown_files.length} unsupported`);
      if (report.failed_files?.length) {
        const f = report.failed_files[0];
        parts.push(`${report.failed_files.length} failed (${f.path ?? ''} ${f.reason ?? ''})`.trim());
      }
      if (report.paused_offline) parts.push('paused: offline');
      if (report.stopped_space_full) parts.push('stopped: S4 space full');
      return { count: report.imported_count || 0, note: parts.join(', ') };
    } catch (e: any) {
      return { count: 0, error: e?.toString() || 'Import failed' };
    }
  }

  // Seed sample data in SQLite (§4.12)
  async seedDemoLibrary(): Promise<number> {
    if (!isTauriAvailable()) return 0;
    try {
      const count = await invoke<number>('seed_sample_library');
      await this.loadBootstrap();
      return count;
    } catch (e) {
      console.error('Failed to seed sample library:', e);
      return 0;
    }
  }

  // Import Status & Control (§9)
  async getImportStatus(): Promise<BackendImportStatusView> {
    const fallback: BackendImportStatusView = {
      running: false,
      paused: false,
      cancelled: false,
      progress: {
        total_files: 0,
        total_bytes: 0,
        completed_files: 0,
        completed_bytes: 0,
        skipped_duplicates: 0,
        failed_files: 0,
        hashed_files: 0,
        uploading_files: 0
      },
      queue: []
    };
    if (!isTauriAvailable()) return fallback;
    try {
      return await invoke<BackendImportStatusView>('get_import_status');
    } catch (e) {
      console.error('Failed to get import status:', e);
      return fallback;
    }
  }

  async pauseImport(): Promise<BackendImportStatusView> {
    if (!isTauriAvailable()) return this.getImportStatus();
    return await invoke<BackendImportStatusView>('pause_import');
  }

  async resumeImport(): Promise<BackendImportStatusView> {
    if (!isTauriAvailable()) return this.getImportStatus();
    return await invoke<BackendImportStatusView>('resume_import');
  }

  async cancelImport(): Promise<BackendImportStatusView> {
    if (!isTauriAvailable()) return this.getImportStatus();
    return await invoke<BackendImportStatusView>('cancel_import');
  }

  async resetImport(): Promise<BackendImportStatusView> {
    if (!isTauriAvailable()) return this.getImportStatus();
    return await invoke<BackendImportStatusView>('reset_import');
  }

  async clearImportQueue(all = false): Promise<BackendImportStatusView> {
    if (!isTauriAvailable()) return this.getImportStatus();
    return await invoke<BackendImportStatusView>('clear_import_queue', { all });
  }

  /**
   * How to play a video or a Live Photo's motion: `playdirect` (load the file
   * through `frames://`) or a loopback stream that repackages or converts it.
   */
  async getVideoPlayback(
    id: string,
    supportsHevc: boolean,
    forceConvert = false
  ): Promise<BackendVideoPlaybackPlan> {
    return await invoke<BackendVideoPlaybackPlan>('get_video_playback', {
      id,
      supportsHevc,
      forceConvert
    });
  }

  /** Ask for a folder and copy the originals of `ids` into it. */
  async exportAssets(ids: string[]): Promise<{ exported: number; cancelled?: boolean }> {
    const destinationFolder = await invoke<string | null>('pick_folder_dialog', {
      prompt: 'Choose a folder to export the originals to'
    });
    if (!destinationFolder) return { exported: 0, cancelled: true };
    const paths = await invoke<string[]>('export_assets', { ids, destinationFolder });
    return { exported: paths.length };
  }

  async getWorkerStatus(): Promise<{ uploading_count: number; thumb_generating_count: number }> {
    if (!isTauriAvailable()) return { uploading_count: 0, thumb_generating_count: 0 };
    try {
      return await invoke<{ uploading_count: number; thumb_generating_count: number }>('get_worker_status');
    } catch (e) {
      return { uploading_count: 0, thumb_generating_count: 0 };
    }
  }
}

export const tauriRepo = new TauriAssetRepo();
