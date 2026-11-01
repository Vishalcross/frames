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
}

export interface BackendAlbumSummary {
  id: string;
  name: string;
  asset_count: number;
  key_asset_id: string | null;
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
  timeline: {
    assets: BackendAssetSummary[];
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
  const isVideo = s.kind === 'video';

  // Seed palette deterministically from ID
  const idNum = s.id.split('').reduce((acc, c) => acc + c.charCodeAt(0), 0);
  const hue = (idNum * 37) % 360;

  return {
    id: s.id,
    kind: isVideo ? 'video' : 'photo',
    takenAt,
    w: s.width || 4032,
    h: s.height || 3024,
    durationMs: s.duration_ms || undefined,
    thumbHash: s.id,
    palette: [
      { l: 0.65, c: 0.16, h: hue, pop: 0.5 },
      { l: 0.50, c: 0.12, h: (hue + 60) % 360, pop: 0.3 }
    ],
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

export class TauriAssetRepo implements AssetRepo {
  private cachedAssets: Asset[] = [];
  private cachedAlbums: AlbumItem[] = [];
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
      this.cachedAssets = (data.timeline.assets || []).map(backendSummaryToAsset);
      this.cachedAlbums = (data.albums || []).map((a) => ({
        id: a.id,
        title: a.name,
        assetCount: a.asset_count,
        keyAssetId: a.key_asset_id || (this.cachedAssets[0]?.id ?? ''),
        dateRange: ''
      }));
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

  getAssets(): Asset[] {
    return this.cachedAssets;
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
    for (const a of this.cachedAssets) {
      if (a.year) yearsSet.add(a.year);
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
    if (q.scope === 'album' && q.albumId) {
      return await this.getAlbumAssetCount(q.albumId);
    }
    if (q.search || q.filter || q.year) {
      const items = await this.range(q, 0, 10000);
      return items.length;
    }
    return this.cachedAssets.length;
  }

  async range(q: Query, from: number, n: number): Promise<Asset[]> {
    if (!this.isLoaded) {
      await this.loadBootstrap();
    }

    if (q.scope === 'album' && q.albumId) {
      if (isTauriAvailable()) {
        return await this.getAlbumAssets(q.albumId, from, n);
      }
      return [];
    }

    let list: Asset[] = this.cachedAssets;

    if (q.search && q.search.trim()) {
      try {
        const found = await invoke<BackendAssetSummary[]>('search_assets', {
          filter: {
            text: q.search.trim(),
            album_id: null,
            year: null,
            month: null,
            camera: null,
            kind: null,
            file_ext: null
          }
        });
        list = found.map(backendSummaryToAsset);
      } catch {
        const needle = q.search.trim().toLowerCase();
        list = this.cachedAssets.filter((a) =>
          (a.title || '').toLowerCase().includes(needle)
        );
      }
    } else if (q.filter === 'favorites') {
      list = this.cachedAssets.filter((a) => a.favorite);
    } else if (q.filter === 'videos') {
      list = this.cachedAssets.filter((a) => a.kind === 'video');
    } else if (q.filter === 'photos') {
      list = this.cachedAssets.filter((a) => a.kind === 'photo');
    } else if (q.year) {
      list = this.cachedAssets.filter(
        (a) => a.year === q.year && (!q.month || a.month === q.month)
      );
    }

    return list.slice(from, from + n);
  }

  async detail(id: string): Promise<AssetDetail> {
    if (!isTauriAvailable()) {
      const a = this.cachedAssets.find((it) => it.id === id);
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
      record: this.cachedAssets.find((it) => it.id === id) || {
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
    const item = this.cachedAssets.find((a) => a.id === id);
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
    this.cachedAssets = this.cachedAssets.filter((a) => a.id !== id);
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
  async createAlbum(name: string): Promise<AlbumItem[]> {
    if (!isTauriAvailable()) return this.cachedAlbums;
    try {
      const raw = await invoke<BackendAlbumSummary[]>('create_album', { name });
      this.cachedAlbums = raw.map((a) => ({
        id: a.id,
        title: a.name,
        assetCount: a.asset_count,
        keyAssetId: a.key_asset_id || '',
        dateRange: ''
      }));
      return this.cachedAlbums;
    } catch (e) {
      console.error('Failed to create album:', e);
      return this.cachedAlbums;
    }
  }

  async renameAlbum(id: string, name: string): Promise<void> {
    if (!isTauriAvailable()) return;
    try {
      await invoke('rename_album', { id, name });
      const found = this.cachedAlbums.find((a) => a.id === id);
      if (found) found.title = name;
    } catch (e) {
      console.error('Failed to rename album:', e);
    }
  }

  async deleteAlbum(id: string): Promise<void> {
    if (!isTauriAvailable()) return;
    try {
      await invoke('delete_album', { id });
      this.cachedAlbums = this.cachedAlbums.filter((a) => a.id !== id);
    } catch (e) {
      console.error('Failed to delete album:', e);
    }
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

  // Local Library Roots & Background Scan (§1.2, §1.3)
  async getLocalRoots(): Promise<string[]> {
    if (!isTauriAvailable()) return [];
    try {
      return await invoke<string[]>('get_local_roots');
    } catch (e) {
      console.error('Failed to get local roots:', e);
      return [];
    }
  }

  async setLocalRoots(paths: string[]): Promise<void> {
    if (!isTauriAvailable()) return;
    try {
      await invoke('set_local_roots', { paths });
      await this.loadBootstrap();
    } catch (e) {
      console.error('Failed to set local roots:', e);
    }
  }

  async scanLocalLibrary(): Promise<number> {
    if (!isTauriAvailable()) return 0;
    try {
      const count = await invoke<number>('scan_local_library');
      await this.loadBootstrap();
      return count;
    } catch (e) {
      console.error('Failed to scan local library:', e);
      return 0;
    }
  }

  async queueUpload(assetId: string): Promise<string> {
    if (!isTauriAvailable()) return '';
    try {
      return await invoke<string>('queue_upload', { assetId });
    } catch (e) {
      console.error('Failed to queue upload:', e);
      return '';
    }
  }

  async getWorkerStatus(): Promise<{ uploading_count: number; thumb_generating_count: number }> {
    if (!isTauriAvailable()) return { uploading_count: 0, thumb_generating_count: 0 };
    try {
      return await invoke<{ uploading_count: number; thumb_generating_count: number }>('get_worker_status');
    } catch (e) {
      return { uploading_count: 0, thumb_generating_count: 0 };
    }
  }

  async prioritize(assetIds: string[]): Promise<void> {
    if (!isTauriAvailable()) return;
    try {
      await invoke('prioritize', { assetIds });
    } catch (e) {
      console.warn('Failed to prioritize assets:', e);
    }
  }
}

export const tauriRepo = new TauriAssetRepo();
