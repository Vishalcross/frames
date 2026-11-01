<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import {
    tauriRepo,
    isTauriAvailable,
    type BackendImportStatusView,
    type BackendImportQueueRow,
    type ImportReportEvent
  } from '$lib/services/tauriRepo';

  let {
    isTauri = true,
    onimport,
    onsync,
    onimported,
    onnavigatelibrary
  }: {
    isTauri?: boolean;
    onimport?: () => void;
    onsync?: () => void;
    onimported?: () => void;
    onnavigatelibrary?: () => void;
  } = $props();

  let status = $state<BackendImportStatusView>({
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
  });

  let liveReport = $state<ImportReportEvent | null>(null);
  let filterQuery = $state('');
  let pollTimer: ReturnType<typeof setInterval> | null = null;
  let unlistenProgress: (() => void) | null = null;
  let startTime = $state<number | null>(null);
  let isClearing = $state(false);

  // Formatting helpers
  function formatBytes(bytes: number): string {
    if (!bytes || bytes <= 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    const val = bytes / Math.pow(k, i);
    return `${val < 10 ? val.toFixed(1) : Math.round(val)} ${sizes[i]}`;
  }

  function getFileName(filePath: string): string {
    if (!filePath) return '';
    const parts = filePath.split(/[/\\]/);
    return parts[parts.length - 1] || filePath;
  }

  function getFileDir(filePath: string): string {
    if (!filePath) return '';
    const parts = filePath.split(/[/\\]/);
    if (parts.length <= 1) return '';
    return parts.slice(0, -1).join('/');
  }

  // Derived progress statistics
  const progressPercent = $derived.by(() => {
    const p = status.progress;
    if (p.total_bytes > 0) {
      return Math.min(100, Math.round((p.completed_bytes / p.total_bytes) * 100));
    }
    if (p.total_files > 0) {
      return Math.min(100, Math.round((p.completed_files / p.total_files) * 100));
    }
    if (liveReport && status.queue.length > 0) {
      const done = liveReport.imported_count + liveReport.duplicates_skipped + liveReport.failed_files.length;
      return Math.min(100, Math.round((done / status.queue.length) * 100));
    }
    return 0;
  });

  const estimatedTimeRemaining = $derived.by(() => {
    if (!status.running || !startTime) return null;
    const p = status.progress;
    if (p.total_bytes <= 0 || p.completed_bytes <= 0) return null;

    const elapsedSec = (Date.now() - startTime) / 1000;
    if (elapsedSec < 1) return 'Calculating…';

    const bytesPerSec = p.completed_bytes / elapsedSec;
    if (bytesPerSec <= 0) return null;

    const remainingBytes = Math.max(0, p.total_bytes - p.completed_bytes);
    if (remainingBytes <= 0) return 'Finishing up…';

    const remainingSec = Math.round(remainingBytes / bytesPerSec);
    if (remainingSec < 60) return `~${remainingSec}s left`;
    const min = Math.floor(remainingSec / 60);
    const sec = remainingSec % 60;
    return `~${min}m ${sec}s left`;
  });

  const filteredQueue = $derived.by(() => {
    if (!filterQuery.trim()) return status.queue;
    const q = filterQuery.toLowerCase();
    return status.queue.filter(
      (item) =>
        item.path.toLowerCase().includes(q) ||
        item.state.toLowerCase().includes(q) ||
        (item.error && item.error.toLowerCase().includes(q))
    );
  });

  const failedItems = $derived.by(() => {
    return status.queue.filter((item) => item.state === 'failed' || Boolean(item.error));
  });

  async function refreshStatus() {
    if (!isTauriAvailable()) return;
    try {
      const s = await tauriRepo.getImportStatus();
      status = s;
      if (s.running && !startTime) {
        startTime = Date.now();
      } else if (!s.running && startTime) {
        startTime = null;
      }
    } catch (e) {
      console.error('Failed to refresh import status:', e);
    }
  }

  async function handlePause() {
    try {
      status = await tauriRepo.pauseImport();
    } catch (e) {
      console.error('Failed to pause import:', e);
    }
  }

  async function handleResume() {
    try {
      status = await tauriRepo.resumeImport();
    } catch (e) {
      console.error('Failed to resume import:', e);
    }
  }

  async function handleCancel() {
    try {
      status = await tauriRepo.cancelImport();
      startTime = null;
    } catch (e) {
      console.error('Failed to cancel import:', e);
    }
  }

  async function handleReset() {
    try {
      status = await tauriRepo.resetImport();
      startTime = null;
      await refreshStatus();
    } catch (e) {
      console.error('Failed to reset import:', e);
    }
  }

  async function handleClearFinished() {
    if (isClearing) return;
    isClearing = true;
    try {
      status = await tauriRepo.clearImportQueue(false);
      onimported?.();
    } catch (e) {
      console.error('Failed to clear finished imports:', e);
    } finally {
      isClearing = false;
    }
  }

  async function handleClearAll() {
    if (isClearing) return;
    isClearing = true;
    try {
      status = await tauriRepo.clearImportQueue(true);
      onimported?.();
    } catch (e) {
      console.error('Failed to clear import queue:', e);
    } finally {
      isClearing = false;
    }
  }

  onMount(() => {
    refreshStatus();

    // Listen for real-time progress events from Tauri
    if (isTauriAvailable()) {
      import('@tauri-apps/api/event').then(({ listen }) => {
        listen<ImportReportEvent>('frames://import-progress', (event) => {
          liveReport = event.payload;
          refreshStatus();
        }).then((un) => {
          unlistenProgress = un;
        });
      });
    }

    // Light polling every 1.5 seconds while active
    pollTimer = setInterval(() => {
      refreshStatus();
    }, 1500);
  });

  onDestroy(() => {
    if (pollTimer) clearInterval(pollTimer);
    if (unlistenProgress) unlistenProgress();
  });
</script>

<div class="imports-view-container">
  <!-- Top View Header -->
  <header class="imports-header">
    <div class="header-info">
      <div class="title-row">
        <h2 class="imports-title">Imports</h2>
        {#if status.running}
          <span class="status-badge status-running">
            <span class="pulse-dot"></span>
            Importing…
          </span>
        {:else if status.paused}
          <span class="status-badge status-paused">
            <Icon name="pause" size={12} />
            Paused
          </span>
        {:else if status.cancelled}
          <span class="status-badge status-cancelled">
            <Icon name="x" size={12} />
            Cancelled
          </span>
        {:else if status.queue.length > 0}
          <span class="status-badge status-idle">
            Ready
          </span>
        {/if}
      </div>
      <p class="imports-sub">
        Monitor active folder imports, file uploads to MEGA S4, and recent import history.
      </p>
    </div>

    <!-- Header Action Controls (§9) -->
    <div class="header-actions">
      {#if status.running}
        <Button variant="standard" onclick={handlePause} title="Pause import">
          <Icon name="pause" size={14} />
          <span>Pause</span>
        </Button>
        <Button variant="danger" onclick={handleCancel} title="Cancel ongoing import">
          <Icon name="x" size={14} />
          <span>Cancel</span>
        </Button>
      {:else if status.paused}
        <Button variant="accent" onclick={handleResume} title="Resume import">
          <Icon name="play" size={14} />
          <span>Resume</span>
        </Button>
        <Button variant="danger" onclick={handleCancel} title="Cancel import">
          <Icon name="x" size={14} />
          <span>Cancel</span>
        </Button>
      {:else}
        {#if onimport}
          <Button variant="accent" onclick={onimport}>
            <Icon name="arrow-down-to-line" size={14} />
            <span>Import Photos…</span>
          </Button>
        {/if}
        {#if onnavigatelibrary && status.progress.completed_files > 0}
          <Button variant="standard" onclick={onnavigatelibrary} title="View imported photos in Library">
            <Icon name="photo" size={14} />
            <span>View in Library</span>
          </Button>
        {/if}
        {#if status.queue.length > 0}
          <Button variant="standard" onclick={handleClearFinished} disabled={isClearing}>
            <span>Clear Finished</span>
          </Button>
        {/if}
      {/if}
    </div>
  </header>

  <!-- Active Import Progress Card (§9: files, GB, time left, duplicates skipped, errors, pause/resume) -->
  {#if status.running || status.paused || status.progress.total_files > 0}
    <section class="progress-card">
      <div class="card-top-row">
        <div class="card-primary-stat">
          <span class="stat-highlight">
            {status.progress.completed_files} of {status.progress.total_files} files
          </span>
          <span class="stat-secondary">
            ({formatBytes(status.progress.completed_bytes)} of {formatBytes(status.progress.total_bytes)})
          </span>
        </div>

        <div class="card-meta-stat">
          {#if status.running && estimatedTimeRemaining}
            <span class="time-remaining-label">
              {estimatedTimeRemaining}
            </span>
          {:else if status.paused}
            <span class="paused-label">Paused</span>
          {:else if !status.running && status.progress.completed_files === status.progress.total_files && status.progress.total_files > 0}
            <span class="completed-label">All files processed</span>
          {/if}
          <span class="percent-label">{progressPercent}%</span>
        </div>
      </div>

      <!-- Linear Progress Bar -->
      <div class="progress-bar-track" role="progressbar" aria-valuenow={progressPercent} aria-valuemin={0} aria-valuemax={100}>
        <div
          class="progress-bar-fill {status.paused ? 'paused' : ''}"
          style="width: {progressPercent}%"
        ></div>
      </div>

      <!-- Pill Metrics Row -->
      <div class="metrics-row">
        <div class="metric-pill">
          <Icon name="cloud-upload" size={13} />
          <span>{status.progress.uploading_files} uploading</span>
        </div>

        <div class="metric-pill">
          <Icon name="sparkles" size={13} />
          <span>{status.progress.hashed_files} hashed</span>
        </div>

        {#if status.progress.skipped_duplicates > 0 || (liveReport && liveReport.duplicates_skipped > 0)}
          <div class="metric-pill pill-skipped">
            <Icon name="copy" size={13} />
            <span>
              {status.progress.skipped_duplicates || liveReport?.duplicates_skipped || 0} duplicates skipped
            </span>
          </div>
        {/if}

        {#if status.progress.failed_files > 0 || failedItems.length > 0}
          <div class="metric-pill pill-failed">
            <Icon name="x" size={13} />
            <span>
              {status.progress.failed_files || failedItems.length} failed
            </span>
          </div>
        {/if}

        {#if liveReport?.paused_offline}
          <div class="metric-pill pill-warning">
            <Icon name="cloud" size={13} />
            <span>Paused: Device is offline</span>
          </div>
        {/if}

        {#if liveReport?.stopped_space_full}
          <div class="metric-pill pill-danger">
            <Icon name="info" size={13} />
            <span>Stopped: MEGA S4 storage full</span>
          </div>
        {/if}
      </div>
    </section>
  {/if}

  <!-- Failed Items Alert Section (if any files failed) -->
  {#if failedItems.length > 0 || (liveReport && liveReport.failed_files.length > 0)}
    <section class="failed-alert-card">
      <div class="failed-alert-header">
        <div class="failed-alert-title">
          <Icon name="info" size={16} />
          <span>Import Issues Encountered ({failedItems.length || liveReport?.failed_files.length})</span>
        </div>
        <Button variant="ghost" onclick={handleReset} title="Reset import state">
          Reset Import Lock
        </Button>
      </div>

      <div class="failed-list">
        {#each failedItems.slice(0, 10) as item}
          <div class="failed-row">
            <span class="failed-path" title={item.path}>{getFileName(item.path)}</span>
            <span class="failed-reason">{item.error || 'Upload or hash failed'}</span>
          </div>
        {/each}
        {#if liveReport}
          {#each liveReport.failed_files.slice(0, 10) as f}
            <div class="failed-row">
              <span class="failed-path" title={f.path}>{getFileName(f.path)}</span>
              <span class="failed-reason">{f.reason || 'Read error'}</span>
            </div>
          {/each}
        {/if}
      </div>
    </section>
  {/if}

  <!-- Queue & Recent Import Items -->
  {#if status.queue.length > 0}
    <section class="queue-section">
      <div class="queue-toolbar">
        <div class="queue-count-label">
          Queue & Recent Items ({status.queue.length})
        </div>

        <div class="queue-search-wrap">
          <input
            type="search"
            class="queue-search-input"
            placeholder="Filter by file or state…"
            bind:value={filterQuery}
          />
        </div>
      </div>

      <div class="queue-table-wrap">
        <table class="queue-table">
          <thead>
            <tr>
              <th class="th-state">Status</th>
              <th class="th-file">File</th>
              <th class="th-size">Size</th>
              <th class="th-details">Details</th>
            </tr>
          </thead>
          <tbody>
            {#each filteredQueue as item (item.path)}
              <tr class="queue-row row-{item.state}">
                <td class="td-state">
                  <span class="badge-state state-{item.state}">
                    {#if item.state === 'uploading'}
                      <span class="spinner-small"></span>
                      Uploading
                    {:else if item.state === 'logged'}
                      <Icon name="check" size={12} stroke={2.5} />
                      Imported
                    {:else if item.state === 'skipped'}
                      <Icon name="copy" size={12} />
                      Duplicate
                    {:else if item.state === 'hashed'}
                      Hashed
                    {:else if item.state === 'failed'}
                      <Icon name="x" size={12} />
                      Failed
                    {:else}
                      Pending
                    {/if}
                  </span>
                </td>
                <td class="td-file">
                  <div class="file-name" title={item.path}>{getFileName(item.path)}</div>
                  <div class="file-dir" title={item.path}>{getFileDir(item.path)}</div>
                </td>
                <td class="td-size">{formatBytes(item.size)}</td>
                <td class="td-details">
                  {#if item.state === 'failed' && item.error}
                    <span class="detail-error">{item.error}</span>
                  {:else if item.state === 'uploading' && item.parts_done > 0}
                    <span class="detail-muted">{item.parts_done} part(s) uploaded</span>
                  {:else if item.state === 'logged'}
                    <span class="detail-success">Saved to library</span>
                  {:else if item.state === 'skipped'}
                    <span class="detail-muted">Identical SHA-256 exists</span>
                  {:else}
                    <span class="detail-muted">—</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {:else if !status.running}
    <!-- Empty State when no items in queue and not importing -->
    <div class="imports-empty-state">
      <div class="empty-icon-circle">
        <Icon name="arrow-down-to-line" size={36} stroke={1.75} />
      </div>
      <h3 class="empty-title">No Active Imports</h3>
      <p class="empty-sub">
        Import photos and videos from any folder on your computer or external drive. Files will be safely encrypted and backed up to MEGA S4.
      </p>
      <div class="empty-actions">
        {#if onimport}
          <Button variant="accent" onclick={onimport}>
            <Icon name="arrow-down-to-line" size={14} />
            <span>Select Folder to Import…</span>
          </Button>
        {/if}
        {#if onsync}
          <Button variant="standard" onclick={onsync}>
            <Icon name="cloud" size={14} />
            <span>Sync MEGA S4</span>
          </Button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .imports-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 24px) 32px 64px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .imports-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--separator);
  }

  .title-row {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .imports-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0;
  }

  .imports-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
    margin: 4px 0 0;
    max-width: 580px;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* Status Badges */
  .status-badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    border-radius: var(--r-pill);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.2px;
  }

  .status-running {
    background: rgba(10, 132, 255, 0.15);
    color: var(--accent);
  }

  .status-paused {
    background: rgba(255, 159, 10, 0.15);
    color: #ff9f0a;
  }

  .status-cancelled {
    background: rgba(255, 69, 58, 0.15);
    color: #ff453a;
  }

  .status-idle {
    background: var(--fill-2);
    color: var(--text-2);
  }

  .pulse-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.4s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% {
      opacity: 1;
      transform: scale(1);
    }
    50% {
      opacity: 0.4;
      transform: scale(0.8);
    }
  }

  /* Progress Card */
  .progress-card {
    background: var(--fill-1);
    border: 1px solid var(--separator);
    border-radius: var(--r-lg);
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .card-top-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 16px;
  }

  .card-primary-stat {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .stat-highlight {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-1);
  }

  .stat-secondary {
    font-size: 13px;
    color: var(--text-2);
  }

  .card-meta-stat {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 13px;
  }

  .time-remaining-label {
    color: var(--accent);
    font-weight: 500;
  }

  .paused-label {
    color: #ff9f0a;
    font-weight: 500;
  }

  .completed-label {
    color: #30d158;
    font-weight: 500;
  }

  .percent-label {
    font-weight: 700;
    color: var(--text-1);
    min-width: 38px;
    text-align: right;
  }

  /* Progress Bar */
  .progress-bar-track {
    width: 100%;
    height: 8px;
    background: var(--fill-3);
    border-radius: 4px;
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 4px;
    transition: width 0.3s ease;
  }

  .progress-bar-fill.paused {
    background: #ff9f0a;
  }

  /* Metrics Pill Row */
  .metrics-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }

  .metric-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: var(--r-pill);
    background: var(--fill-2);
    font-size: 12px;
    font-weight: 500;
    color: var(--text-2);
  }

  .pill-skipped {
    background: rgba(142, 142, 147, 0.15);
    color: var(--text-2);
  }

  .pill-failed {
    background: rgba(255, 69, 58, 0.12);
    color: #ff453a;
  }

  .pill-warning {
    background: rgba(255, 159, 10, 0.15);
    color: #ff9f0a;
  }

  .pill-danger {
    background: rgba(255, 69, 58, 0.15);
    color: #ff453a;
  }

  /* Failed Alert Box */
  .failed-alert-card {
    background: rgba(255, 69, 58, 0.08);
    border: 1px solid rgba(255, 69, 58, 0.25);
    border-radius: var(--r-md);
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .failed-alert-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .failed-alert-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
    color: #ff453a;
  }

  .failed-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .failed-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 12px;
  }

  .failed-path {
    color: var(--text-1);
    font-family: monospace;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 60%;
  }

  .failed-reason {
    color: #ff453a;
  }

  /* Queue Table Section */
  .queue-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .queue-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .queue-count-label {
    font-size: 14px;
    font-weight: 600;
    color: var(--text-1);
  }

  .queue-search-wrap {
    width: 240px;
  }

  .queue-search-input {
    width: 100%;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--r-md);
    border: 1px solid var(--separator);
    background: var(--fill-1);
    color: var(--text-1);
    font-size: 12px;
    box-sizing: border-box;
    outline: none;
  }

  .queue-search-input:focus {
    border-color: var(--accent);
  }

  .queue-table-wrap {
    border: 1px solid var(--separator);
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--fill-1);
  }

  .queue-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    text-align: left;
  }

  .queue-table th {
    background: var(--fill-2);
    padding: 10px 14px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-2);
    border-bottom: 1px solid var(--separator);
  }

  .queue-table td {
    padding: 10px 14px;
    border-bottom: 1px solid var(--separator);
    vertical-align: middle;
  }

  .queue-table tr:last-child td {
    border-bottom: none;
  }

  .th-state { width: 120px; }
  .th-size { width: 90px; }
  .th-details { width: 200px; }

  .badge-state {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    border-radius: var(--r-pill);
    font-size: 11px;
    font-weight: 500;
  }

  .state-logged {
    background: rgba(48, 209, 88, 0.15);
    color: #30d158;
  }

  .state-uploading {
    background: rgba(10, 132, 255, 0.15);
    color: var(--accent);
  }

  .state-hashed {
    background: rgba(191, 90, 242, 0.15);
    color: #bf5af2;
  }

  .state-skipped {
    background: var(--fill-3);
    color: var(--text-2);
  }

  .state-failed {
    background: rgba(255, 69, 58, 0.15);
    color: #ff453a;
  }

  .state-pending {
    background: var(--fill-2);
    color: var(--text-3);
  }

  .file-name {
    font-weight: 500;
    color: var(--text-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 440px;
  }

  .file-dir {
    font-size: 11px;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 440px;
  }

  .td-size {
    color: var(--text-2);
    font-size: 12px;
  }

  .detail-error {
    color: #ff453a;
    font-size: 12px;
  }

  .detail-success {
    color: #30d158;
    font-size: 12px;
  }

  .detail-muted {
    color: var(--text-3);
    font-size: 12px;
  }

  .spinner-small {
    width: 10px;
    height: 10px;
    border: 2px solid rgba(10, 132, 255, 0.2);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Empty State */
  .imports-empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 80px 24px;
    text-align: center;
    gap: 14px;
  }

  .empty-icon-circle {
    width: 68px;
    height: 68px;
    border-radius: 50%;
    background: var(--fill-2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
  }

  .empty-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0;
  }

  .empty-sub {
    font-size: var(--text-size-body);
    line-height: var(--text-lh-body);
    color: var(--text-2);
    margin: 0;
    max-width: 460px;
  }

  .empty-actions {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
  }
</style>
