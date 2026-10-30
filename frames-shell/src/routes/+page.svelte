<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  type AssetKind = "photo" | "video" | "live";

  interface AssetSummary {
    id: string;
    kind: AssetKind;
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

  interface AlbumRef {
    id: string;
    name: string;
  }

  interface AssetRecord extends AssetSummary {
    container: string | null;
    video_codec: string | null;
    audio_codec: string | null;
    camera_make: string | null;
    camera_model: string | null;
    motion_id: string | null;
    motion_key: string | null;
    motion_size: number | null;
  }

  interface AssetDetail {
    record: AssetRecord;
    trashed_at: string | null;
    albums: AlbumRef[];
  }

  interface MonthGroup {
    year: number;
    month: number;
    count: number;
  }

  interface AlbumSummary {
    id: string;
    name: string;
    created_at: string;
    asset_count: number;
    cover_asset_id: string | null;
  }

  interface TrashItem {
    asset: AssetSummary;
    trashed_at: string;
    days_left: number;
    motion_size: number | null;
  }

  interface TrashSummary {
    item_count: number;
    total_bytes: number;
  }

  interface SearchSuggestions {
    albums: AlbumRef[];
    dates: string[];
    cameras: string[];
    file_types: string[];
    filenames: string[];
  }

  interface SearchChip {
    kind: "album" | "year" | "camera" | "type" | "ext" | "text";
    label: string;
    value: string;
  }

  interface VideoPlaybackPlan {
    asset_id: string;
    decision: { mode: string; copy_video?: boolean; copy_audio?: boolean };
    playback_url: string;
    duration_ms: number | null;
    can_seek_via_query: boolean;
  }

  interface SettingsView {
    endpoint: string;
    region: string;
    bucket: string;
    access_key_id: string;
    has_secret_in_keychain: boolean;
    space_limit_gb: number;
    cache_location: string;
    cache_limit_gb: number;
    use_real_s4: boolean;
    device_id: string;
  }

  interface CacheStats {
    small_thumb_count: number;
    small_thumb_bytes: number;
    lru_entry_count: number;
    lru_used_bytes: number;
    lru_limit_bytes: number;
  }

  interface SpaceUsage {
    used_bytes: number;
    limit_bytes: number;
    warn_90_percent: boolean;
    is_full: boolean;
  }

  interface ImportProgress {
    total_files: number;
    total_bytes: number;
    completed_files: number;
    completed_bytes: number;
    skipped_duplicates: number;
    failed_files: number;
  }

  // Navigation & Primary State
  let activeNav = $state<"library" | "album" | "imports" | "trash" | "settings">("library");
  let activeAlbumId = $state<string | null>(null);
  let deviceId = $state("");
  let offline = $state(false);
  let outboxPending = $state(0);
  let statusNotice = $state<string | null>(null);

  // Data collections
  let timelineAssets = $state<AssetSummary[]>([]);
  let displayedAssets = $state<AssetSummary[]>([]);
  let albums = $state<AlbumSummary[]>([]);
  let trashItems = $state<TrashItem[]>([]);
  let trashSum = $state<TrashSummary>({ item_count: 0, total_bytes: 0 });
  let importProgress = $state<ImportProgress>({
    total_files: 0,
    total_bytes: 0,
    completed_files: 0,
    completed_bytes: 0,
    skipped_duplicates: 0,
    failed_files: 0
  });
  let lastImportReport = $state<any>(null);
  let settings = $state<SettingsView>({
    endpoint: "https://s3.eu-central-1.s4.mega.io",
    region: "eu-central-1",
    bucket: "frames-library",
    access_key_id: "",
    has_secret_in_keychain: false,
    space_limit_gb: 750,
    cache_location: "",
    cache_limit_gb: 10,
    use_real_s4: false,
    device_id: ""
  });
  let secretInput = $state("");
  let cacheStats = $state<CacheStats>({
    small_thumb_count: 0,
    small_thumb_bytes: 0,
    lru_entry_count: 0,
    lru_used_bytes: 0,
    lru_limit_bytes: 10_000_000_000
  });
  let spaceUsage = $state<SpaceUsage>({
    used_bytes: 0,
    limit_bytes: 750_000_000_000,
    warn_90_percent: false,
    is_full: false
  });
  let checkReport = $state<any>(null);

  // Grid & Virtualization State
  let zoomColumns = $state(6); // Spec: 3–15 columns
  let thumbUrls = $state<Record<string, string>>({});
  let scrollTop = $state(0);
  let viewportHeight = $state(800);

  // Selection & Drag-and-Drop State
  let selectedIds = $state<Set<string>>(new Set());
  let lastClickedIndex = $state<number | null>(null);
  let draggingIds = $state<string[]>([]);
  let dragOverAlbumId = $state<string | null>(null);

  // Search State
  let searchInput = $state("");
  let searchChips = $state<SearchChip[]>([]);
  let suggestions = $state<SearchSuggestions | null>(null);
  let showSuggestions = $state(false);

  // Album Modals / Inline Inputs
  let newAlbumName = $state("");
  let renamingAlbumId = $state<string | null>(null);
  let renameAlbumValue = $state("");
  let targetAlbumPick = $state("");

  // Import & Export Inputs
  let importFolderPath = $state("");
  let importTargetAlbum = $state("");
  let exportFolderPath = $state("/tmp/frames-export");

  // Trash Confirmation Modal
  let confirmPurgeModal = $state<{ mode: "selected" | "empty"; ids: string[]; count: number; bytes: number } | null>(null);

  // Viewer State
  let viewerIndex = $state<number | null>(null);
  let viewerDetail = $state<AssetDetail | null>(null);
  let viewerZoom = $state(1);
  let showInfoPanel = $state(true);
  let adjustDateValue = $state("");
  let livePlaying = $state(false);
  let videoPlan = $state<VideoPlaybackPlan | null>(null);
  let videoCurrentUrl = $state("");
  let videoSeekSeconds = $state(0);
  let videoError = $state(false);

  const MONTH_NAMES = [
    "",
    "January", "February", "March", "April", "May", "June",
    "July", "August", "June", "October", "November", "December"
  ];
  MONTH_NAMES[9] = "September";

  function notify(msg: string) {
    statusNotice = msg;
    setTimeout(() => {
      if (statusNotice === msg) statusNotice = null;
    }, 4000);
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function formatDuration(ms: number | null): string {
    if (!ms) return "0:00";
    const totalSec = Math.round(ms / 1000);
    const min = Math.floor(totalSec / 60);
    const sec = totalSec % 60;
    return `${min}:${sec.toString().padStart(2, "0")}`;
  }

  function localYearMonth(asset: AssetSummary): { year: number; month: number; key: string; label: string } {
    const dt = new Date(asset.taken_at);
    const shifted = asset.tz_offset != null
      ? new Date(dt.getTime() + asset.tz_offset * 60 * 1000)
      : dt;
    const year = shifted.getUTCFullYear();
    const month = shifted.getUTCMonth() + 1;
    return {
      year,
      month,
      key: `${year}-${month.toString().padStart(2, "0")}`,
      label: `${MONTH_NAMES[month]} ${year}`
    };
  }

  // Group displayedAssets by Month for Sticky Headers
  let groupedByMonth = $derived.by(() => {
    const groups: { key: string; label: string; year: number; month: number; items: { asset: AssetSummary; globalIdx: number }[] }[] = [];
    displayedAssets.forEach((asset, globalIdx) => {
      const ym = localYearMonth(asset);
      const last = groups[groups.length - 1];
      if (last && last.key === ym.key) {
        last.items.push({ asset, globalIdx });
      } else {
        groups.push({
          key: ym.key,
          label: ym.label,
          year: ym.year,
          month: ym.month,
          items: [{ asset, globalIdx }]
        });
      }
    });
    return groups;
  });

  // Distinct years for the right-hand Date Scrubber
  let distinctYears = $derived.by(() => {
    const years: number[] = [];
    for (const g of groupedByMonth) {
      if (!years.includes(g.year)) years.push(g.year);
    }
    return years;
  });

  async function loadBootstrap() {
    try {
      const data: any = await invoke("get_app_bootstrap");
      deviceId = data.device_id;
      timelineAssets = data.timeline.assets;
      albums = data.albums;
      trashItems = data.trash.items;
      trashSum = data.trash.summary;
      importProgress = data.import_progress;
      settings = data.settings;
      cacheStats = data.cache_stats;
      spaceUsage = data.space_usage;
      outboxPending = data.outbox_pending;
      offline = data.offline;
      await refreshDisplayedAssets();
    } catch (e) {
      notify(`Failed to load library: ${e}`);
    }
  }

  async function refreshDisplayedAssets() {
    if (searchChips.length > 0 || searchInput.trim().length > 0) {
      const filter: any = {
        text: searchInput.trim() || null,
        album_id: activeNav === "album" ? activeAlbumId : null,
        year: null,
        month: null,
        camera: null,
        kind: null,
        file_ext: null
      };
      for (const chip of searchChips) {
        if (chip.kind === "album") filter.album_id = chip.value;
        if (chip.kind === "year") filter.year = parseInt(chip.value.slice(0, 4), 10);
        if (chip.kind === "camera") filter.camera = chip.value;
        if (chip.kind === "type") {
          if (["photo", "video", "live"].includes(chip.value)) {
            filter.kind = chip.value;
          } else {
            filter.file_ext = chip.value;
          }
        }
        if (chip.kind === "text") {
          filter.text = [filter.text, chip.value].filter(Boolean).join(" ");
        }
      }
      displayedAssets = await invoke("search_assets", { filter });
    } else if (activeNav === "album" && activeAlbumId) {
      displayedAssets = await invoke("get_album_assets", { albumId: activeAlbumId });
    } else {
      displayedAssets = timelineAssets;
    }
    prefetchVisibleThumbnails();
  }

  async function prefetchVisibleThumbnails() {
    for (const a of displayedAssets.slice(0, 120)) {
      if (!thumbUrls[a.id]) {
        invoke<string | null>("get_thumbnail_data_url", { id: a.id, preview: false }).then((url) => {
          if (url) thumbUrls[a.id] = url;
        });
      }
    }
  }

  async function handleSeedSampleLibrary() {
    const count: number = await invoke("seed_sample_library");
    await loadBootstrap();
    notify(`Loaded ${count} sample photos, RAWs, Live Photos & videos into library`);
  }

  // Selection logic: Click, Shift-click, Cmd/Ctrl-click, Month select
  function handleAssetClick(e: MouseEvent, idx: number, id: string) {
    const next = new Set(selectedIds);
    if (e.shiftKey && lastClickedIndex !== null) {
      const start = Math.min(lastClickedIndex, idx);
      const end = Math.max(lastClickedIndex, idx);
      if (!e.metaKey && !e.ctrlKey) next.clear();
      for (let i = start; i <= end; i++) {
        next.add(displayedAssets[i].id);
      }
    } else if (e.metaKey || e.ctrlKey) {
      if (next.has(id)) next.delete(id);
      else next.add(id);
      lastClickedIndex = idx;
    } else {
      next.clear();
      next.add(id);
      lastClickedIndex = idx;
    }
    selectedIds = next;
  }

  function selectWholeMonth(items: { asset: AssetSummary; globalIdx: number }[]) {
    const next = new Set(selectedIds);
    const allSelected = items.every((it) => next.has(it.asset.id));
    for (const it of items) {
      if (allSelected) next.delete(it.asset.id);
      else next.add(it.asset.id);
    }
    selectedIds = next;
  }

  // Viewer navigation & neighbour prefetch (Spec Section 9)
  async function openViewer(idx: number) {
    if (idx < 0 || idx >= displayedAssets.length) return;
    viewerIndex = idx;
    viewerZoom = 1;
    livePlaying = false;
    videoError = false;
    videoPlan = null;
    const target = displayedAssets[idx];
    viewerDetail = await invoke("get_asset_detail", { id: target.id });
    if (viewerDetail) {
      adjustDateValue = viewerDetail.record.taken_at.slice(0, 19) + "Z";
    }
    // Load high-res preview
    invoke<string | null>("get_thumbnail_data_url", { id: target.id, preview: true }).then((url) => {
      if (url) thumbUrls[target.id] = url;
    });
    // Prepare video or Live Photo motion playback plan
    if (target.kind === "video" || target.kind === "live") {
      try {
        const vEl = document.createElement("video");
        const canHevc = vEl.canPlayType('video/mp4; codecs="hvc1"') !== "" || vEl.canPlayType('video/mp4; codecs="hev1"') !== "";
        const plan: VideoPlaybackPlan = await invoke("get_video_playback", {
          id: target.id,
          supportsHevc: canHevc
        });
        videoPlan = plan;
        videoCurrentUrl = plan.playback_url;
        videoSeekSeconds = 0;
      } catch {
        videoError = true;
      }
    }
    // Prefetch neighbours (idx - 1 and idx + 1)
    for (const nIdx of [idx - 1, idx + 1]) {
      if (nIdx >= 0 && nIdx < displayedAssets.length) {
        const nid = displayedAssets[nIdx].id;
        invoke<string | null>("get_thumbnail_data_url", { id: nid, preview: true }).then((url) => {
          if (url) thumbUrls[nid] = url;
        });
      }
    }
  }

  async function handleSeekConvertedVideo(newSeconds: number) {
    if (!videoPlan) return;
    videoSeekSeconds = newSeconds;
    if (videoPlan.can_seek_via_query) {
      videoCurrentUrl = await invoke("get_video_seek_url", {
        baseUrl: videoPlan.playback_url,
        seekSeconds: newSeconds
      });
    }
  }

  async function saveAdjustedDate() {
    if (!viewerDetail) return;
    try {
      const updated: AssetDetail | null = await invoke("adjust_asset_date", {
        id: viewerDetail.record.id,
        newTakenIso: adjustDateValue
      });
      if (updated) viewerDetail = updated;
      await loadBootstrap();
      notify("Updated date taken in index");
    } catch (e) {
      notify(`Invalid date format: ${e}`);
    }
  }

  async function handleExportIds(ids: string[]) {
    if (ids.length === 0) return;
    try {
      const out: string[] = await invoke("export_assets", {
        ids,
        destinationFolder: exportFolderPath
      });
      notify(`Exported ${out.length} original file(s) to ${exportFolderPath}`);
    } catch (e) {
      notify(`Export failed: ${e}`);
    }
  }

  // Search Suggestions & Chips
  async function onSearchInputChange() {
    if (searchInput.trim().length === 0) {
      suggestions = null;
      await refreshDisplayedAssets();
      return;
    }
    suggestions = await invoke("get_search_suggestions", { query: searchInput });
    showSuggestions = true;
    await refreshDisplayedAssets();
  }

  function addSearchChip(chip: SearchChip) {
    searchChips = [...searchChips, chip];
    searchInput = "";
    suggestions = null;
    showSuggestions = false;
    refreshDisplayedAssets();
  }

  function removeSearchChip(idx: number) {
    searchChips = searchChips.filter((_, i) => i !== idx);
    refreshDisplayedAssets();
  }

  // Album actions
  async function handleCreateAlbum(e: Event) {
    e.preventDefault();
    if (!newAlbumName.trim()) return;
    albums = await invoke("create_album", { name: newAlbumName });
    newAlbumName = "";
    await loadBootstrap();
  }

  async function handleRenameAlbum(albumId: string) {
    if (!renameAlbumValue.trim()) return;
    albums = await invoke("rename_album", { albumId, name: renameAlbumValue });
    renamingAlbumId = null;
    await loadBootstrap();
  }

  async function handleDeleteAlbum(albumId: string, name: string) {
    albums = await invoke("delete_album", { albumId });
    if (activeAlbumId === albumId) {
      activeNav = "library";
      activeAlbumId = null;
    }
    await loadBootstrap();
    notify(`Deleted album "${name}" (photos kept in Library)`);
  }

  async function handleBulkAddToAlbum(albumId: string, ids: string[]) {
    if (!albumId || ids.length === 0) return;
    albums = await invoke("add_to_album", { albumId, assetIds: ids });
    await loadBootstrap();
    notify(`Added ${ids.length} item(s) to album`);
  }

  async function handleBulkMoveToAlbum(toAlbumId: string) {
    if (!activeAlbumId || !toAlbumId || selectedIds.size === 0) return;
    const ids = Array.from(selectedIds);
    await invoke("move_to_album", {
      fromAlbumId: activeAlbumId,
      toAlbumId,
      assetIds: ids
    });
    selectedIds = new Set();
    await loadBootstrap();
    notify(`Moved ${ids.length} item(s) to album`);
  }

  async function handleBulkRemoveFromAlbum() {
    if (!activeAlbumId || selectedIds.size === 0) return;
    const ids = Array.from(selectedIds);
    await invoke("remove_from_album", { albumId: activeAlbumId, assetIds: ids });
    selectedIds = new Set();
    await loadBootstrap();
    notify(`Removed ${ids.length} item(s) from album`);
  }

  async function handleMoveSelectedToTrash(ids: string[]) {
    if (ids.length === 0) return;
    await invoke("trash_assets", { ids });
    selectedIds = new Set();
    viewerIndex = null;
    await loadBootstrap();
    notify(`Moved ${ids.length} item(s) to Trash (30-day auto-delete)`);
  }

  // Trash actions
  async function handleRestoreTrash(ids: string[]) {
    await invoke("restore_assets", { ids });
    await loadBootstrap();
    notify(`Restored ${ids.length} item(s) from Trash`);
  }

  function promptDeleteTrashNow(ids: string[]) {
    let bytes = 0;
    for (const item of trashItems) {
      if (ids.includes(item.asset.id)) {
        bytes += item.asset.size + (item.motion_size ?? 0);
      }
    }
    confirmPurgeModal = { mode: "selected", ids, count: ids.length, bytes };
  }

  function promptEmptyTrash() {
    confirmPurgeModal = {
      mode: "empty",
      ids: trashItems.map((i) => i.asset.id),
      count: trashSum.item_count,
      bytes: trashSum.total_bytes
    };
  }

  async function executeConfirmedPurge() {
    if (!confirmPurgeModal) return;
    const modal = confirmPurgeModal;
    confirmPurgeModal = null;
    if (modal.mode === "empty") {
      const rep: any = await invoke("empty_all_trash");
      notify(`Permanently deleted ${rep.purged_asset_ids.length} item(s) from S4`);
    } else {
      const rep: any = await invoke("delete_trash_now", { ids: modal.ids });
      notify(`Permanently deleted ${rep.purged_asset_ids.length} item(s) from S4`);
    }
    await loadBootstrap();
  }

  // Import actions
  async function handleRunImport(e: Event) {
    e.preventDefault();
    if (!importFolderPath.trim()) return;
    try {
      const rep: any = await invoke("import_from_paths", {
        paths: [importFolderPath.trim()],
        albumId: importTargetAlbum || null
      });
      lastImportReport = rep;
      await loadBootstrap();
      notify(`Imported ${rep.imported_count} item(s) (${rep.duplicates_skipped} duplicates skipped)`);
    } catch (err) {
      notify(`Import error: ${err}`);
    }
  }

  // Settings & Sync actions
  async function handleSyncNow() {
    const rep: any = await invoke("sync_now");
    await loadBootstrap();
    if (rep.offline) {
      notify("Offline — changes safely queued in outbox");
    } else {
      notify(`Synced with S4 (${rep.batches_pushed} pushed, ${rep.batches_pulled} pulled)`);
    }
  }

  async function handleToggleOffline() {
    offline = await invoke("toggle_offline_mode", { offline: !offline });
    await loadBootstrap();
  }

  async function handleSaveSettings(e: Event) {
    e.preventDefault();
    settings = await invoke("save_settings", {
      input: {
        endpoint: settings.endpoint,
        region: settings.region,
        bucket: settings.bucket,
        access_key_id: settings.access_key_id,
        secret_access_key: secretInput.trim() ? secretInput : null,
        space_limit_gb: Number(settings.space_limit_gb),
        cache_location: settings.cache_location,
        cache_limit_gb: Number(settings.cache_limit_gb),
        use_real_s4: settings.use_real_s4
      }
    });
    secretInput = "";
    await loadBootstrap();
    notify("Saved settings (S4 key stored in OS Keychain)");
  }

  async function handleClearCache() {
    const [freed, stats]: [number, CacheStats] = await invoke("clear_local_cache");
    cacheStats = stats;
    notify(`Cleared ${formatBytes(freed)} of cached previews & originals`);
  }

  async function handleCheckLibrary() {
    checkReport = await invoke("run_check_library");
    notify("Completed S4 bucket vs local index check");
  }

  async function handleRebuildDatabase() {
    const rep: any = await invoke("run_rebuild_database");
    await loadBootstrap();
    notify(`Rebuilt local database from S4 (${rep.batches_pulled} log batches applied)`);
  }

  // Keyboard shortcuts (Spec Section 9: Space quick look, arrows, Delete -> Trash, Cmd/Ctrl+A)
  function handleGlobalKeydown(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return;

    if (viewerIndex !== null) {
      if (e.key === "Escape" || e.key === " ") {
        e.preventDefault();
        viewerIndex = null;
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        if (viewerIndex + 1 < displayedAssets.length) openViewer(viewerIndex + 1);
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        if (viewerIndex > 0) openViewer(viewerIndex - 1);
      } else if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        handleMoveSelectedToTrash([displayedAssets[viewerIndex].id]);
      }
      return;
    }

    if (e.key === " " && selectedIds.size > 0) {
      e.preventDefault();
      const firstId = Array.from(selectedIds)[0];
      const idx = displayedAssets.findIndex((a) => a.id === firstId);
      if (idx >= 0) openViewer(idx);
    } else if (e.key === "Enter" && selectedIds.size > 0) {
      e.preventDefault();
      const firstId = Array.from(selectedIds)[0];
      const idx = displayedAssets.findIndex((a) => a.id === firstId);
      if (idx >= 0) openViewer(idx);
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
      if (activeNav === "library" || activeNav === "album") {
        e.preventDefault();
        selectedIds = new Set(displayedAssets.map((a) => a.id));
      }
    } else if ((e.key === "Delete" || e.key === "Backspace") && selectedIds.size > 0) {
      if (activeNav === "library" || activeNav === "album") {
        e.preventDefault();
        handleMoveSelectedToTrash(Array.from(selectedIds));
      }
    }
  }

  function scrollToYear(year: number) {
    const el = document.getElementById(`year-anchor-${year}`);
    if (el) el.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  onMount(() => {
    loadBootstrap();
  });
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div class="app-shell">
  <!-- Translucent Vibrancy / Mica Sidebar (Spec Section 9) -->
  <aside class="sidebar">
    <div class="brand-row">
      <div class="brand-title">Frames</div>
      <span class="device-pill" title="Device ID">{deviceId || "local"}</span>
    </div>

    <nav class="nav-section">
      <button
        class="nav-item"
        class:active={activeNav === "library"}
        onclick={() => {
          activeNav = "library";
          activeAlbumId = null;
          selectedIds = new Set();
          refreshDisplayedAssets();
        }}
      >
        <span>🖼️ Library</span>
        <span class="count-badge">{timelineAssets.length}</span>
      </button>

      <div class="nav-heading">Albums</div>
      {#each albums as alb (alb.id)}
        <div
          class="album-nav-row"
          class:active={activeNav === "album" && activeAlbumId === alb.id}
          class:drag-over={dragOverAlbumId === alb.id}
          role="button"
          tabindex="0"
          ondragover={(e) => {
            e.preventDefault();
            dragOverAlbumId = alb.id;
          }}
          ondragleave={() => {
            if (dragOverAlbumId === alb.id) dragOverAlbumId = null;
          }}
          ondrop={(e) => {
            e.preventDefault();
            dragOverAlbumId = null;
            if (draggingIds.length > 0) {
              handleBulkAddToAlbum(alb.id, draggingIds);
              draggingIds = [];
            }
          }}
          onclick={() => {
            activeNav = "album";
            activeAlbumId = alb.id;
            selectedIds = new Set();
            refreshDisplayedAssets();
          }}
          onkeydown={(e) => {
            if (e.key === "Enter") {
              activeNav = "album";
              activeAlbumId = alb.id;
              refreshDisplayedAssets();
            }
          }}
        >
          <span class="album-name">📁 {alb.name}</span>
          <span class="count-badge">{alb.asset_count}</span>
        </div>
      {/each}

      <form class="new-album-form" onsubmit={handleCreateAlbum}>
        <input
          type="text"
          placeholder="+ New Album..."
          bind:value={newAlbumName}
        />
      </form>

      <div class="nav-divider"></div>

      <button
        class="nav-item"
        class:active={activeNav === "imports"}
        onclick={() => (activeNav = "imports")}
      >
        <span>📥 Imports</span>
        {#if importProgress.total_files > 0}
          <span class="count-badge">{importProgress.completed_files}/{importProgress.total_files}</span>
        {/if}
      </button>

      <button
        class="nav-item"
        class:active={activeNav === "trash"}
        onclick={() => (activeNav = "trash")}
      >
        <span>🗑️ Trash</span>
        <span class="count-badge">{trashSum.item_count}</span>
      </button>

      <button
        class="nav-item"
        class:active={activeNav === "settings"}
        onclick={() => (activeNav = "settings")}
      >
        <span>⚙️ Settings</span>
      </button>
    </nav>

    <div class="sidebar-footer">
      <div class="space-meter">
        <div class="space-label">
          <span>MEGA S4 Storage</span>
          <span>{formatBytes(spaceUsage.used_bytes)} / {settings.space_limit_gb} GB</span>
        </div>
        <div class="progress-track">
          <div
            class="progress-fill"
            class:warn={spaceUsage.warn_90_percent}
            style="width: {Math.min(100, (spaceUsage.used_bytes / Math.max(1, spaceUsage.limit_bytes)) * 100)}%"
          ></div>
        </div>
      </div>

      <div class="sync-controls">
        <button class="btn-secondary small" onclick={handleSyncNow}>
          🔄 Sync {outboxPending > 0 ? `(${outboxPending})` : ""}
        </button>
        <button
          class="btn-secondary small"
          class:offline-btn={offline}
          onclick={handleToggleOffline}
          title="Simulate or toggle offline mode"
        >
          {offline ? "⚡ Offline" : "🟢 Online"}
        </button>
      </div>
    </div>
  </aside>

  <!-- Main Content Area -->
  <div class="main-area">
    <!-- Status & Error Banners (Spec Section 11) -->
    {#if offline}
      <div class="banner offline-banner">
        <strong>Offline</strong> — Browsing, search, and album changes still work. Changes wait in the outbox ({outboxPending} pending); uploads pause until back online.
      </div>
    {/if}
    {#if spaceUsage.warn_90_percent}
      <div class="banner warn-banner">
        <strong>Storage Warning:</strong> Bucket usage is over 90% of your configured {settings.space_limit_gb} GB limit ({formatBytes(spaceUsage.used_bytes)} used).
      </div>
    {/if}
    {#if statusNotice}
      <div class="banner notice-banner">{statusNotice}</div>
    {/if}

    <!-- Top Toolbar: Search Box + Chips + Zoom Slider (Spec Section 9) -->
    <header class="top-toolbar">
      <div class="search-wrapper">
        <span class="search-icon">🔍</span>
        {#each searchChips as chip, i}
          <span class="search-chip">
            {chip.label}
            <button type="button" onclick={() => removeSearchChip(i)}>×</button>
          </span>
        {/each}
        <input
          type="text"
          class="search-input"
          placeholder="Search filename, date, album, camera, type..."
          bind:value={searchInput}
          oninput={onSearchInputChange}
          onfocus={() => {
            if (suggestions) showSuggestions = true;
          }}
          onblur={() => setTimeout(() => (showSuggestions = false), 180)}
        />

        {#if showSuggestions && suggestions}
          <div class="suggestions-dropdown">
            {#if suggestions.albums.length > 0}
              <div class="sugg-group">Albums</div>
              {#each suggestions.albums as alb}
                <button
                  type="button"
                  class="sugg-item"
                  onclick={() =>
                    addSearchChip({ kind: "album", label: `Album: ${alb.name}`, value: alb.id })}
                >
                  📁 {alb.name}
                </button>
              {/each}
            {/if}
            {#if suggestions.dates.length > 0}
              <div class="sugg-group">Dates</div>
              {#each suggestions.dates as d}
                <button
                  type="button"
                  class="sugg-item"
                  onclick={() => addSearchChip({ kind: "year", label: `Date: ${d}`, value: d })}
                >
                  📅 {d}
                </button>
              {/each}
            {/if}
            {#if suggestions.cameras.length > 0}
              <div class="sugg-group">Cameras</div>
              {#each suggestions.cameras as cam}
                <button
                  type="button"
                  class="sugg-item"
                  onclick={() =>
                    addSearchChip({ kind: "camera", label: `Camera: ${cam}`, value: cam })}
                >
                  📷 {cam}
                </button>
              {/each}
            {/if}
            {#if suggestions.file_types.length > 0}
              <div class="sugg-group">Types</div>
              {#each suggestions.file_types as ft}
                <button
                  type="button"
                  class="sugg-item"
                  onclick={() =>
                    addSearchChip({ kind: "type", label: `Type: ${ft.toUpperCase()}`, value: ft })}
                >
                  🎞️ {ft.toUpperCase()}
                </button>
              {/each}
            {/if}
            {#if suggestions.filenames.length > 0}
              <div class="sugg-group">Filenames</div>
              {#each suggestions.filenames as fn}
                <button
                  type="button"
                  class="sugg-item"
                  onclick={() =>
                    addSearchChip({ kind: "text", label: `File: ${fn}`, value: fn })}
                >
                  📄 {fn}
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </div>

      {#if activeNav === "library" || activeNav === "album"}
        <div class="zoom-control" title="Grid columns (3 to 15)">
          <span>zoom</span>
          <input
            type="range"
            min="3"
            max="15"
            step="1"
            bind:value={zoomColumns}
          />
          <span class="zoom-val">{zoomColumns}</span>
        </div>
      {/if}
    </header>

    <!-- Selection Action Bar (Spec Section 9) -->
    {#if selectedIds.size > 0 && (activeNav === "library" || activeNav === "album")}
      <div class="selection-bar">
        <strong>{selectedIds.size} selected</strong>
        <div class="selection-actions">
          <select bind:value={targetAlbumPick}>
            <option value="">Choose album...</option>
            {#each albums as alb}
              <option value={alb.id}>{alb.name}</option>
            {/each}
          </select>
          <button
            class="btn-secondary small"
            disabled={!targetAlbumPick}
            onclick={() => handleBulkAddToAlbum(targetAlbumPick, Array.from(selectedIds))}
          >
            + Add to Album
          </button>
          {#if activeNav === "album" && activeAlbumId}
            <button
              class="btn-secondary small"
              disabled={!targetAlbumPick}
              onclick={() => handleBulkMoveToAlbum(targetAlbumPick)}
            >
              ⇄ Move to Album
            </button>
            <button class="btn-secondary small" onclick={handleBulkRemoveFromAlbum}>
              Remove from Album
            </button>
          {/if}
          <button
            class="btn-secondary small"
            onclick={() => handleExportIds(Array.from(selectedIds))}
          >
            ⬇ Export Originals
          </button>
          <button
            class="btn-danger small"
            onclick={() => handleMoveSelectedToTrash(Array.from(selectedIds))}
          >
            🗑️ Move to Trash
          </button>
          <button class="btn-secondary small" onclick={() => (selectedIds = new Set())}>
            Clear
          </button>
        </div>
      </div>
    {/if}

    <!-- Views -->
    {#if activeNav === "library" || activeNav === "album"}
      {#if activeNav === "album" && activeAlbumId}
        {@const currentAlbum = albums.find((a) => a.id === activeAlbumId)}
        {#if currentAlbum}
          <div class="album-header-bar">
            {#if renamingAlbumId === currentAlbum.id}
              <input
                type="text"
                bind:value={renameAlbumValue}
                onkeydown={(e) => {
                  if (e.key === "Enter") handleRenameAlbum(currentAlbum.id);
                }}
              />
              <button class="btn-primary small" onclick={() => handleRenameAlbum(currentAlbum.id)}>
                Save
              </button>
              <button class="btn-secondary small" onclick={() => (renamingAlbumId = null)}>
                Cancel
              </button>
            {:else}
              <h2>{currentAlbum.name} <span class="sub-count">({currentAlbum.asset_count} items)</span></h2>
              <div class="album-header-actions">
                <button
                  class="btn-secondary small"
                  onclick={() => {
                    renamingAlbumId = currentAlbum.id;
                    renameAlbumValue = currentAlbum.name;
                  }}
                >
                  Rename
                </button>
                <button
                  class="btn-danger small"
                  onclick={() => handleDeleteAlbum(currentAlbum.id, currentAlbum.name)}
                >
                  Delete Album
                </button>
              </div>
            {/if}
          </div>
        {/if}
      {/if}

      <div class="timeline-stage">
        <div
          class="timeline-scroll"
          onscroll={(e) => (scrollTop = (e.currentTarget as HTMLElement).scrollTop)}
          bind:clientHeight={viewportHeight}
        >
          {#if displayedAssets.length === 0}
            <div class="empty-state">
              <h3>No photos or videos here yet</h3>
              <p>Import a folder from your external disk or load the sample library to explore Timeline, Live Photos, RAWs, Albums, and Video playback.</p>
              <div class="empty-actions">
                <button class="btn-primary" onclick={handleSeedSampleLibrary}>
                  ✨ Load Sample Set (HEIC, RAW, Live Photo, MOV, MKV)
                </button>
                <button class="btn-secondary" onclick={() => (activeNav = "imports")}>
                  📥 Open Imports
                </button>
              </div>
            </div>
          {:else}
            {#each groupedByMonth as group (group.key)}
              <section class="month-section" id="year-anchor-{group.year}">
                <div class="sticky-month-header">
                  <span class="month-title">{group.label}</span>
                  <button
                    type="button"
                    class="month-select-btn"
                    onclick={() => selectWholeMonth(group.items)}
                  >
                    Select Month ({group.items.length})
                  </button>
                </div>

                <div
                  class="thumb-grid"
                  style="grid-template-columns: repeat({zoomColumns}, minmax(0, 1fr));"
                >
                  {#each group.items as { asset, globalIdx } (asset.id)}
                    <div
                      class="thumb-cell"
                      class:selected={selectedIds.has(asset.id)}
                      role="button"
                      tabindex="0"
                      draggable="true"
                      ondragstart={() => {
                        draggingIds = selectedIds.has(asset.id)
                          ? Array.from(selectedIds)
                          : [asset.id];
                      }}
                      onclick={(e) => handleAssetClick(e, globalIdx, asset.id)}
                      ondblclick={() => openViewer(globalIdx)}
                      onkeydown={(e) => {
                        if (e.key === "Enter") openViewer(globalIdx);
                      }}
                    >
                      {#if thumbUrls[asset.id]}
                        <img src={thumbUrls[asset.id]} alt={asset.filename} loading="lazy" />
                      {:else}
                        <div class="thumb-placeholder">
                          <span>{asset.filename}</span>
                        </div>
                      {/if}

                      <!-- Badges: video length, LIVE, date guessed, thumb failed (Spec Section 9) -->
                      <div class="thumb-badges-top">
                        {#if asset.kind === "live"}
                          <span class="badge live-badge">● LIVE</span>
                        {/if}
                        {#if asset.date_guessed}
                          <span class="badge guessed-badge" title="Date guessed from file modified time">≈ Date</span>
                        {/if}
                        {#if asset.thumb_failed}
                          <span class="badge warn-badge" title="Placeholder thumbnail">!</span>
                        {/if}
                      </div>

                      <div class="thumb-badges-bottom">
                        <span class="filename-overlay">{asset.filename}</span>
                        {#if asset.kind === "video"}
                          <span class="badge video-badge">▶ {formatDuration(asset.duration_ms)}</span>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              </section>
            {/each}
          {/if}
        </div>

        <!-- Right-hand Year / Date Scrubber (Spec Section 9) -->
        {#if distinctYears.length > 0}
          <div class="year-scrubber">
            {#each distinctYears as yr}
              <button
                type="button"
                class="year-pill"
                onclick={() => scrollToYear(yr)}
              >
                {yr}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    {:else if activeNav === "imports"}
      <div class="panel-view">
        <h2>Import Photos & Videos</h2>
        <p class="muted">
          The source folder is opened strictly read-only. Exact SHA-256 duplicates are skipped automatically, and Live Photos (Photo + MOV) are paired into a single asset.
        </p>

        <form class="card-form" onsubmit={handleRunImport}>
          <div class="form-row">
            <label for="import-path">Source Folder Path (External Disk or Local Folder)</label>
            <input
              id="import-path"
              type="text"
              placeholder="/Volumes/PhotosDisk/DCIM or C:\Photos"
              bind:value={importFolderPath}
            />
          </div>
          <div class="form-row">
            <label for="import-album">Add Imported Items to Album (Optional)</label>
            <select id="import-album" bind:value={importTargetAlbum}>
              <option value="">None (Library only)</option>
              {#each albums as alb}
                <option value={alb.id}>{alb.name}</option>
              {/each}
            </select>
          </div>
          <div class="form-actions">
            <button type="submit" class="btn-primary">Start / Resume Import</button>
            <button type="button" class="btn-secondary" onclick={handleSeedSampleLibrary}>
              ✨ Seed Sample Set
            </button>
          </div>
        </form>

        <div class="stats-grid">
          <div class="stat-card">
            <span class="stat-label">Files Processed</span>
            <strong>{importProgress.completed_files} / {importProgress.total_files}</strong>
          </div>
          <div class="stat-card">
            <span class="stat-label">Data Transferred</span>
            <strong>{formatBytes(importProgress.completed_bytes)} / {formatBytes(importProgress.total_bytes)}</strong>
          </div>
          <div class="stat-card">
            <span class="stat-label">Duplicates Skipped</span>
            <strong>{importProgress.skipped_duplicates}</strong>
          </div>
          <div class="stat-card">
            <span class="stat-label">Errors</span>
            <strong>{importProgress.failed_files}</strong>
          </div>
        </div>

        {#if lastImportReport}
          <div class="report-card">
            <h3>Latest Import Summary</h3>
            <p>
              Imported: <strong>{lastImportReport.imported_count}</strong> ·
              Duplicates skipped: <strong>{lastImportReport.duplicates_skipped}</strong>
            </p>
            {#if lastImportReport.unknown_files?.length > 0}
              <h4>Skipped (Unknown Types)</h4>
              <ul>
                {#each lastImportReport.unknown_files as uf}
                  <li><code>{uf.path}</code> — {uf.reason}</li>
                {/each}
              </ul>
            {/if}
            {#if lastImportReport.failed_files?.length > 0}
              <h4>Unreadable / Failed Files</h4>
              <ul>
                {#each lastImportReport.failed_files as ff}
                  <li><code>{ff.path}</code> — {ff.reason}</li>
                {/each}
              </ul>
            {/if}
          </div>
        {/if}
      </div>
    {:else if activeNav === "trash"}
      <div class="panel-view">
        <div class="trash-header">
          <div>
            <h2>Trash</h2>
            <p class="muted">
              Items in Trash are automatically purged from MEGA S4 after 30 days.
              Currently holding {trashSum.item_count} item(s) ({formatBytes(trashSum.total_bytes)}).
            </p>
          </div>
          {#if trashItems.length > 0}
            <button class="btn-danger" onclick={promptEmptyTrash}>
              Empty Trash ({trashSum.item_count} items · {formatBytes(trashSum.total_bytes)})
            </button>
          {/if}
        </div>

        {#if trashItems.length === 0}
          <div class="empty-state">
            <p>Trash is empty.</p>
          </div>
        {:else}
          <div class="trash-list">
            {#each trashItems as item (item.asset.id)}
              <div class="trash-row">
                <div class="trash-info">
                  <strong>{item.asset.filename}</strong>
                  <span class="muted">
                    {formatBytes(item.asset.size + (item.motion_size ?? 0))} ·
                    {item.days_left} days left before permanent S4 purge
                  </span>
                </div>
                <div class="trash-actions">
                  <button
                    class="btn-secondary small"
                    onclick={() => handleRestoreTrash([item.asset.id])}
                  >
                    Restore
                  </button>
                  <button
                    class="btn-danger small"
                    onclick={() => promptDeleteTrashNow([item.asset.id])}
                  >
                    Delete Now
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {:else if activeNav === "settings"}
      <div class="panel-view">
        <h2>Settings & Library Maintenance</h2>

        <form class="card-form" onsubmit={handleSaveSettings}>
          <h3>MEGA S4 Object Storage</h3>
          <div class="form-row">
            <label for="s4-endpoint">S4 Endpoint URL</label>
            <input id="s4-endpoint" type="text" bind:value={settings.endpoint} />
          </div>
          <div class="form-row two-col">
            <div>
              <label for="s4-region">Region</label>
              <input id="s4-region" type="text" bind:value={settings.region} />
            </div>
            <div>
              <label for="s4-bucket">Bucket Name</label>
              <input id="s4-bucket" type="text" bind:value={settings.bucket} />
            </div>
          </div>
          <div class="form-row two-col">
            <div>
              <label for="s4-ak">Access Key ID</label>
              <input id="s4-ak" type="text" bind:value={settings.access_key_id} />
            </div>
            <div>
              <label for="s4-sk">
                Secret Access Key {settings.has_secret_in_keychain ? "(Stored in OS Keychain ✓)" : "(Stored in OS Keychain)"}
              </label>
              <input
                id="s4-sk"
                type="password"
                placeholder="Enter to update in OS Keychain..."
                bind:value={secretInput}
              />
            </div>
          </div>
          <div class="form-row two-col">
            <div>
              <label for="s4-limit">Configured Space Limit (GB, warns at 90%)</label>
              <input id="s4-limit" type="number" min="1" bind:value={settings.space_limit_gb} />
            </div>
            <div class="checkbox-row">
              <label>
                <input type="checkbox" bind:checked={settings.use_real_s4} />
                Connect directly to remote S4 endpoint (uncheck for local sandbox bucket)
              </label>
            </div>
          </div>

          <h3>Local Thumbnail & Preview Cache</h3>
          <div class="form-row two-col">
            <div>
              <label for="cache-loc">Cache Directory</label>
              <input id="cache-loc" type="text" bind:value={settings.cache_location} />
            </div>
            <div>
              <label for="cache-cap">Preview & Original LRU Cap (GB)</label>
              <input id="cache-cap" type="number" min="1" bind:value={settings.cache_limit_gb} />
            </div>
          </div>
          <div class="form-row">
            <label for="export-loc">Default Export Originals Folder</label>
            <input id="export-loc" type="text" bind:value={exportFolderPath} />
          </div>

          <div class="form-actions">
            <button type="submit" class="btn-primary">Save Settings</button>
          </div>
        </form>

        <div class="maintenance-card">
          <h3>Cache & Integrity Tools</h3>
          <p class="muted">
            Small thumbnails ({cacheStats.small_thumb_count} files, {formatBytes(cacheStats.small_thumb_bytes)}) are always kept locally.
            LRU Preview/Original cache: {cacheStats.lru_entry_count} files ({formatBytes(cacheStats.lru_used_bytes)} / {formatBytes(cacheStats.lru_limit_bytes)}).
          </p>
          <div class="form-actions">
            <button type="button" class="btn-secondary" onclick={handleClearCache}>
              🧹 Clear Cache
            </button>
            <button type="button" class="btn-secondary" onclick={handleCheckLibrary}>
              🔍 Check Library
            </button>
            <button type="button" class="btn-secondary" onclick={handleRebuildDatabase}>
              ♻️ Rebuild Local Database
            </button>
          </div>

          {#if checkReport}
            <div class="report-card">
              <h4>Check Library Report</h4>
              <p>
                Missing originals in S4: <strong>{checkReport.missing_originals.length}</strong> ·
                Unknown files in S4: <strong>{checkReport.unknown_s4_keys.length}</strong> ·
                Crashed multipart uploads tidied: <strong>{checkReport.tidied_multipart_uploads}</strong>
              </p>
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<!-- Confirm Delete Now / Empty Trash Modal (Spec Section 9) -->
{#if confirmPurgeModal}
  <div class="modal-backdrop">
    <div class="modal-card">
      <h3>Permanently Delete from MEGA S4?</h3>
      <p>
        You are about to permanently delete <strong>{confirmPurgeModal.count} item(s)</strong>
        totalling <strong>{formatBytes(confirmPurgeModal.bytes)}</strong> from your S4 bucket
        (originals, motion files, and thumbnails). This cannot be undone.
      </p>
      <div class="form-actions">
        <button class="btn-danger" onclick={executeConfirmedPurge}>
          Delete {confirmPurgeModal.count} Item(s) ({formatBytes(confirmPurgeModal.bytes)})
        </button>
        <button class="btn-secondary" onclick={() => (confirmPurgeModal = null)}>
          Cancel
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Full-Screen Photo / Video / Live Photo Viewer (Spec Section 8 & 9) -->
{#if viewerIndex !== null && displayedAssets[viewerIndex]}
  {@const current = displayedAssets[viewerIndex]}
  <div class="viewer-overlay">
    <div class="viewer-topbar">
      <div class="viewer-title">
        <strong>{current.filename}</strong>
        {#if current.kind === "live"}
          <button
            type="button"
            class="live-trigger-badge"
            class:active={livePlaying}
            onmouseenter={() => (livePlaying = true)}
            onmouseleave={() => (livePlaying = false)}
            onmousedown={() => (livePlaying = true)}
            onmouseup={() => (livePlaying = false)}
            title="Hover or hold to play Live Photo motion"
          >
            ● LIVE {livePlaying ? "(Playing Motion)" : "(Hover / Hold)"}
          </button>
        {/if}
        {#if videoPlan && (current.kind === "video" || livePlaying)}
          <span class="playback-mode-pill">
            Mode: {videoPlan.decision.mode.toUpperCase()}
          </span>
        {/if}
      </div>

      <div class="viewer-controls">
        {#if current.kind !== "video"}
          <button
            class="btn-secondary small"
            onclick={() => (viewerZoom = Math.max(1, viewerZoom - 0.5))}
          >
            −
          </button>
          <span class="zoom-readout">{Math.round(viewerZoom * 100)}%</span>
          <button
            class="btn-secondary small"
            onclick={() => (viewerZoom = Math.min(4, viewerZoom + 0.5))}
          >
            +
          </button>
        {/if}
        <button class="btn-secondary small" onclick={() => (showInfoPanel = !showInfoPanel)}>
          ℹ️ Info
        </button>
        <button class="btn-secondary small" onclick={() => handleExportIds([current.id])}>
          ⬇ Export
        </button>
        <button class="btn-danger small" onclick={() => handleMoveSelectedToTrash([current.id])}>
          🗑️ Trash
        </button>
        <button class="btn-secondary small" onclick={() => (viewerIndex = null)}>
          ✕ Close
        </button>
      </div>
    </div>

    <div class="viewer-body">
      <button
        class="nav-arrow left"
        disabled={viewerIndex === 0}
        onclick={() => openViewer((viewerIndex ?? 1) - 1)}
      >
        ‹
      </button>

      <div
        class="viewer-stage"
        onwheel={(e) => {
          if (current.kind !== "video") {
            e.preventDefault();
            const delta = e.deltaY < 0 ? 0.25 : -0.25;
            viewerZoom = Math.min(4, Math.max(1, viewerZoom + delta));
          }
        }}
      >
        {#if current.kind === "video" || (current.kind === "live" && livePlaying)}
          {#if videoError}
            <div class="cant-play-card">
              <h3>Can't play here</h3>
              <p>This video codec could not be converted on the fly.</p>
              <button class="btn-primary" onclick={() => handleExportIds([current.id])}>
                ⬇ Export Original ({current.filename})
              </button>
            </div>
          {:else}
            <div class="video-player-container">
              <!-- svelte-ignore a11y_media_has_caption -->
              <video
                src={videoCurrentUrl}
                controls
                autoplay
                poster={thumbUrls[current.id]}
                onerror={() => {
                  // Only trigger fallback if real remote/loopback stream failed outside demo sample
                  if (!current.id.startsWith("a10")) videoError = true;
                }}
              ></video>

              {#if current.duration_ms}
                <div class="scrubber-bar">
                  <span>Seek ({formatDuration(videoSeekSeconds * 1000)} / {formatDuration(current.duration_ms)})</span>
                  <input
                    type="range"
                    min="0"
                    max={Math.ceil(current.duration_ms / 1000)}
                    step="1"
                    bind:value={videoSeekSeconds}
                    onchange={() => handleSeekConvertedVideo(videoSeekSeconds)}
                  />
                </div>
              {/if}
            </div>
          {/if}
        {:else if thumbUrls[current.id]}
          <img
            src={thumbUrls[current.id]}
            alt={current.filename}
            class="viewer-img"
            style="transform: scale({viewerZoom});"
          />
        {/if}
      </div>

      <button
        class="nav-arrow right"
        disabled={viewerIndex + 1 >= displayedAssets.length}
        onclick={() => openViewer((viewerIndex ?? 0) + 1)}
      >
        ›
      </button>

      {#if showInfoPanel && viewerDetail}
        <aside class="info-panel">
          <h3>Info</h3>
          <dl class="meta-list">
            <dt>Filename</dt>
            <dd>{viewerDetail.record.filename}</dd>

            <dt>Date Taken (UTC)</dt>
            <dd>
              {viewerDetail.record.taken_at}
              {#if viewerDetail.record.date_guessed}
                <span class="badge guessed-badge">Guessed from file mtime</span>
              {/if}
            </dd>

            <dt>Camera</dt>
            <dd>
              {[viewerDetail.record.camera_make, viewerDetail.record.camera_model]
                .filter(Boolean)
                .join(" ") || "—"}
            </dd>

            <dt>Dimensions & Size</dt>
            <dd>
              {viewerDetail.record.width ?? "?"} × {viewerDetail.record.height ?? "?"} ·
              {formatBytes(viewerDetail.record.size)}
            </dd>

            {#if viewerDetail.record.video_codec || viewerDetail.record.container}
              <dt>Format / Codecs</dt>
              <dd>
                {[
                  viewerDetail.record.container?.toUpperCase(),
                  viewerDetail.record.video_codec?.toUpperCase(),
                  viewerDetail.record.audio_codec?.toUpperCase()
                ]
                  .filter(Boolean)
                  .join(" / ")}
              </dd>
            {/if}

            <dt>S4 Bucket Key</dt>
            <dd><code>{viewerDetail.record.key}</code></dd>

            {#if viewerDetail.record.motion_key}
              <dt>Live Photo Motion Key</dt>
              <dd><code>{viewerDetail.record.motion_key}</code></dd>
            {/if}

            <dt>Albums</dt>
            <dd>
              {#if viewerDetail.albums.length === 0}
                <span class="muted">None</span>
              {:else}
                {#each viewerDetail.albums as alb}
                  <span class="album-tag">{alb.name}</span>
                {/each}
              {/if}
            </dd>
          </dl>

          <div class="adjust-date-box">
            <h4>Adjust Date (Index Only)</h4>
            <input type="text" bind:value={adjustDateValue} />
            <button class="btn-primary small" onclick={saveAdjustedDate}>
              Update Date
            </button>
          </div>
        </aside>
      {/if}
    </div>
  </div>
{/if}

<style>
  :root {
    --bg-main: #f5f5f7;
    --bg-sidebar: rgba(235, 235, 240, 0.78);
    --bg-card: #ffffff;
    --text-primary: #1d1d1f;
    --text-muted: #6e6e73;
    --border-color: rgba(0, 0, 0, 0.1);
    --accent: #0071e3;
    --danger: #d93025;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", Roboto, sans-serif;
    color: var(--text-primary);
    background-color: var(--bg-main);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --bg-main: #121214;
      --bg-sidebar: rgba(30, 30, 34, 0.78);
      --bg-card: #1c1c1f;
      --text-primary: #f5f5f7;
      --text-muted: #98989d;
      --border-color: rgba(255, 255, 255, 0.12);
      --accent: #0a84ff;
      --danger: #ff453a;
    }
  }

  * {
    box-sizing: border-box;
  }

  :global(body) {
    margin: 0;
    overflow: hidden;
    user-select: none;
  }

  .app-shell {
    display: grid;
    grid-template-columns: 240px 1fr;
    height: 100vh;
    width: 100vw;
  }

  .sidebar {
    background: var(--bg-sidebar);
    backdrop-filter: blur(22px);
    -webkit-backdrop-filter: blur(22px);
    border-right: 1px solid var(--border-color);
    display: flex;
    flex-direction: column;
    padding: 14px 12px;
    gap: 12px;
  }

  .brand-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 8px;
  }

  .brand-title {
    font-size: 18px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  .device-pill {
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 6px;
    background: rgba(128, 128, 128, 0.18);
    color: var(--text-muted);
    font-family: monospace;
  }

  .nav-section {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .nav-heading {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--text-muted);
    padding: 10px 8px 4px;
  }

  .nav-item,
  .album-nav-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    border: none;
    background: transparent;
    color: var(--text-primary);
    padding: 7px 10px;
    border-radius: 8px;
    font-size: 13.5px;
    cursor: pointer;
    text-align: left;
  }

  .nav-item:hover,
  .album-nav-row:hover {
    background: rgba(128, 128, 128, 0.12);
  }

  .nav-item.active,
  .album-nav-row.active {
    background: var(--accent);
    color: #fff;
    font-weight: 500;
  }

  .album-nav-row.drag-over {
    outline: 2px dashed var(--accent);
    background: rgba(10, 132, 255, 0.18);
  }

  .count-badge {
    font-size: 11.5px;
    opacity: 0.75;
  }

  .new-album-form input {
    width: 100%;
    padding: 6px 8px;
    margin-top: 4px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 12.5px;
  }

  .nav-divider {
    height: 1px;
    background: var(--border-color);
    margin: 8px 0;
  }

  .sidebar-footer {
    border-top: 1px solid var(--border-color);
    padding-top: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .space-label {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .progress-track {
    height: 6px;
    border-radius: 3px;
    background: rgba(128, 128, 128, 0.2);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent);
  }

  .progress-fill.warn {
    background: var(--danger);
  }

  .sync-controls {
    display: flex;
    gap: 6px;
  }

  .main-area {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
  }

  .banner {
    padding: 8px 16px;
    font-size: 13px;
    border-bottom: 1px solid var(--border-color);
  }

  .offline-banner {
    background: rgba(255, 159, 10, 0.2);
  }

  .warn-banner {
    background: rgba(255, 69, 58, 0.2);
  }

  .notice-banner {
    background: rgba(10, 132, 255, 0.16);
  }

  .top-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 18px;
    border-bottom: 1px solid var(--border-color);
    background: var(--bg-card);
    gap: 16px;
  }

  .search-wrapper {
    position: relative;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    flex: 1;
    max-width: 620px;
    background: var(--bg-main);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 4px 10px;
  }

  .search-input {
    border: none;
    background: transparent;
    color: var(--text-primary);
    flex: 1;
    min-width: 160px;
    font-size: 13.5px;
    outline: none;
  }

  .search-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--accent);
    color: #fff;
    font-size: 12px;
    padding: 2px 8px;
    border-radius: 12px;
  }

  .search-chip button {
    border: none;
    background: transparent;
    color: #fff;
    cursor: pointer;
    padding: 0 2px;
    font-size: 13px;
  }

  .suggestions-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 10px;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.22);
    z-index: 40;
    max-height: 320px;
    overflow-y: auto;
    padding: 6px;
  }

  .sugg-group {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--text-muted);
    padding: 6px 8px 2px;
  }

  .sugg-item {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--text-primary);
    padding: 6px 10px;
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }

  .sugg-item:hover {
    background: rgba(128, 128, 128, 0.14);
  }

  .zoom-control {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--text-muted);
  }

  .selection-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 18px;
    background: var(--bg-card);
    border-bottom: 1px solid var(--border-color);
    font-size: 13px;
  }

  .selection-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .album-header-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 18px;
    border-bottom: 1px solid var(--border-color);
  }

  .album-header-bar h2 {
    margin: 0;
    font-size: 20px;
  }

  .sub-count {
    font-size: 14px;
    color: var(--text-muted);
    font-weight: 400;
  }

  .timeline-stage {
    position: relative;
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .timeline-scroll {
    flex: 1;
    overflow-y: auto;
    padding: 0 18px 36px;
  }

  .sticky-month-header {
    position: sticky;
    top: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 4px 8px;
    background: var(--bg-main);
    backdrop-filter: blur(12px);
  }

  .month-title {
    font-size: 16px;
    font-weight: 600;
  }

  .month-select-btn {
    border: none;
    background: transparent;
    color: var(--accent);
    font-size: 12.5px;
    cursor: pointer;
  }

  .thumb-grid {
    display: grid;
    gap: 4px;
  }

  .thumb-cell {
    position: relative;
    aspect-ratio: 1 / 1;
    background: rgba(128, 128, 128, 0.15);
    overflow: hidden;
    cursor: pointer;
    border-radius: 3px;
  }

  .thumb-cell.selected {
    outline: 3px solid var(--accent);
    outline-offset: -3px;
  }

  .thumb-cell img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .thumb-placeholder {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    color: var(--text-muted);
    padding: 6px;
    text-align: center;
  }

  .thumb-badges-top {
    position: absolute;
    top: 5px;
    left: 5px;
    display: flex;
    gap: 4px;
  }

  .thumb-badges-bottom {
    position: absolute;
    bottom: 4px;
    left: 5px;
    right: 5px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .filename-overlay {
    font-size: 10px;
    color: #fff;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    font-size: 10px;
    font-weight: 600;
    padding: 2px 5px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.62);
    color: #fff;
  }

  .year-scrubber {
    width: 54px;
    border-left: 1px solid var(--border-color);
    display: flex;
    flex-direction: column;
    align-items: center;
    padding-top: 16px;
    gap: 6px;
    background: var(--bg-card);
  }

  .year-pill {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    padding: 3px 6px;
    border-radius: 6px;
  }

  .year-pill:hover {
    color: var(--accent);
    background: rgba(128, 128, 128, 0.12);
  }

  .panel-view {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    max-width: 880px;
  }

  .card-form,
  .maintenance-card,
  .report-card {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 10px;
    padding: 18px;
    margin-bottom: 18px;
  }

  .form-row {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin-bottom: 12px;
  }

  .form-row.two-col {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .form-row label {
    font-size: 12.5px;
    color: var(--text-muted);
  }

  .form-row input[type="text"],
  .form-row input[type="password"],
  .form-row input[type="number"],
  .form-row select {
    padding: 8px 10px;
    border-radius: 7px;
    border: 1px solid var(--border-color);
    background: var(--bg-main);
    color: var(--text-primary);
    font-size: 13.5px;
  }

  .form-actions {
    display: flex;
    gap: 10px;
    margin-top: 10px;
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 12px;
    margin-bottom: 18px;
  }

  .stat-card {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 10px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .stat-label {
    font-size: 12px;
    color: var(--text-muted);
  }

  .trash-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  .trash-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .trash-row {
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 12px 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .trash-info {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .trash-actions {
    display: flex;
    gap: 8px;
  }

  .empty-state {
    text-align: center;
    padding: 64px 24px;
    color: var(--text-muted);
  }

  .empty-actions {
    display: flex;
    justify-content: center;
    gap: 12px;
    margin-top: 16px;
  }

  .btn-primary,
  .btn-secondary,
  .btn-danger {
    border: none;
    border-radius: 7px;
    padding: 8px 14px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-primary {
    background: var(--accent);
    color: #fff;
  }

  .btn-secondary {
    background: rgba(128, 128, 128, 0.16);
    color: var(--text-primary);
  }

  .btn-danger {
    background: var(--danger);
    color: #fff;
  }

  .small {
    padding: 5px 10px;
    font-size: 12px;
  }

  /* Viewer Overlay */
  .viewer-overlay {
    position: fixed;
    inset: 0;
    background: rgba(10, 10, 12, 0.96);
    color: #fff;
    z-index: 100;
    display: flex;
    flex-direction: column;
  }

  .viewer-topbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 18px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.14);
  }

  .viewer-title {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .live-trigger-badge {
    border: 1px solid rgba(255, 255, 255, 0.35);
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
    border-radius: 14px;
    padding: 3px 10px;
    font-size: 12px;
    cursor: pointer;
  }

  .live-trigger-badge.active {
    background: #ffd60a;
    color: #000;
  }

  .playback-mode-pill {
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 10px;
    background: rgba(10, 132, 255, 0.35);
  }

  .viewer-controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .viewer-body {
    flex: 1;
    display: flex;
    align-items: center;
    overflow: hidden;
    position: relative;
  }

  .viewer-stage {
    flex: 1;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .viewer-img {
    max-width: 85%;
    max-height: 85%;
    object-fit: contain;
    transition: transform 0.12s ease-out;
  }

  .video-player-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    width: 80%;
    max-width: 920px;
  }

  .video-player-container video {
    width: 100%;
    max-height: 72vh;
    background: #000;
    border-radius: 8px;
  }

  .scrubber-bar {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 12px;
  }

  .scrubber-bar input {
    flex: 1;
  }

  .nav-arrow {
    border: none;
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
    font-size: 32px;
    width: 44px;
    height: 64px;
    margin: 0 12px;
    border-radius: 8px;
    cursor: pointer;
  }

  .nav-arrow:disabled {
    opacity: 0.25;
    cursor: default;
  }

  .info-panel {
    width: 310px;
    height: 100%;
    background: rgba(28, 28, 32, 0.95);
    border-left: 1px solid rgba(255, 255, 255, 0.14);
    padding: 18px;
    overflow-y: auto;
  }

  .meta-list {
    font-size: 12.5px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .meta-list dt {
    color: #98989d;
    font-size: 11px;
    text-transform: uppercase;
    margin-top: 6px;
  }

  .meta-list dd {
    margin: 0;
    word-break: break-all;
  }

  .album-tag {
    display: inline-block;
    background: rgba(255, 255, 255, 0.15);
    padding: 2px 7px;
    border-radius: 6px;
    margin-right: 4px;
    font-size: 11.5px;
  }

  .adjust-date-box {
    margin-top: 18px;
    padding-top: 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.14);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .adjust-date-box input {
    padding: 6px 8px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.25);
    background: rgba(0, 0, 0, 0.4);
    color: #fff;
    font-size: 12.5px;
  }

  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 150;
  }

  .modal-card {
    background: var(--bg-card);
    color: var(--text-primary);
    border: 1px solid var(--border-color);
    border-radius: 12px;
    padding: 22px;
    max-width: 440px;
  }
</style>
