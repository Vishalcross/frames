<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import SyncPopover from '$lib/components/dialogs/SyncPopover.svelte';
  import { SAMPLE_ALBUMS, SAMPLE_PINNED_COLLECTIONS } from '$lib/mock/sampleData';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    currentRoute = $bindable('library'),
    currentAlbumId = $bindable(''),
    sidebarWidth = $bindable(240),
    isCollapsed = $bindable(false),
    onnavigate
  }: {
    currentRoute: string;
    currentAlbumId?: string;
    sidebarWidth?: number;
    isCollapsed?: boolean;
    onnavigate?: (route: string, extraId?: string) => void;
  } = $props();

  let syncPopoverOpen = $state(false);
  let syncPaused = $state(false);
  let isUploading = $state(true);
  let openFolders = $state<Record<string, boolean>>({ folder_vacations: true });
  let pinned = $state([...SAMPLE_PINNED_COLLECTIONS]);

  // Resizing state
  let isResizing = $state(false);

  function startResize(e: MouseEvent) {
    e.preventDefault();
    isResizing = true;
    const startX = e.clientX;
    const startW = sidebarWidth;

    const onMouseMove = (ev: MouseEvent) => {
      const delta = ev.clientX - startX;
      sidebarWidth = Math.min(320, Math.max(200, startW + delta));
    };

    const onMouseUp = () => {
      isResizing = false;
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  function navigate(route: string, extraId = '') {
    currentRoute = route;
    currentAlbumId = extraId;
    onnavigate?.(route, extraId);
  }

  function toggleFolder(folderId: string, e: MouseEvent) {
    e.stopPropagation();
    openFolders[folderId] = !openFolders[folderId];
  }

  // Reorder pinned items
  function movePinned(index: number, direction: -1 | 1, e: MouseEvent) {
    e.stopPropagation();
    const target = index + direction;
    if (target < 0 || target >= pinned.length) return;
    const item = pinned[index];
    pinned.splice(index, 1);
    pinned.splice(target, 0, item);
  }
</script>

{#if !isCollapsed}
  <Surface
    kind="sidebar"
    class="frames-sidebar sidebar-docked"
    style="width: {sidebarWidth}px;"
  >
    <!-- Top spacer for titlebar -->
    <div class="sidebar-top-inset"></div>

    <div class="sidebar-scrollable">
      <!-- 1. Main Library Row (§4.1) -->
      <div class="sidebar-group">
        <button
          type="button"
          class="sidebar-row {currentRoute === 'library' ? 'selected' : ''}"
          onclick={() => navigate('library')}
        >
          <span class="row-icon"><Icon name="photo" size={16} /></span>
          <span class="row-label">Library</span>
        </button>
      </div>

      <!-- 2. Collections Section (§4.1) -->
      <div class="sidebar-group">
        <div class="section-header">Collections</div>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'days' ? 'selected' : ''}"
          onclick={() => navigate('days')}
        >
          <span class="row-icon"><Icon name="calendar" size={16} /></span>
          <span class="row-label">Days</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'memories' ? 'selected' : ''}"
          onclick={() => navigate('memories')}
        >
          <span class="row-icon"><Icon name="sparkles" size={16} /></span>
          <span class="row-label">Memories</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'people-pets' ? 'selected' : ''}"
          onclick={() => navigate('people-pets')}
        >
          <span class="row-icon"><Icon name="users" size={16} /></span>
          <span class="row-label">People & Pets</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'trips' ? 'selected' : ''}"
          onclick={() => navigate('trips')}
        >
          <span class="row-icon"><Icon name="compass" size={16} /></span>
          <span class="row-label">Trips</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'collections' ? 'selected' : ''}"
          onclick={() => navigate('collections')}
        >
          <span class="row-icon"><Icon name="album" size={16} /></span>
          <span class="row-label">Albums</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'media-types' ? 'selected' : ''}"
          onclick={() => navigate('media-types')}
        >
          <span class="row-icon"><Icon name="video" size={16} /></span>
          <span class="row-label">Media Types</span>
        </button>
      </div>

      <!-- 3. Pinned Collections (drag / reorder, §4.1) -->
      <div class="sidebar-group">
        <div class="section-header">Pinned collections</div>
        {#each pinned as item, i}
          <div class="pinned-row-wrap">
            <button
              type="button"
              class="sidebar-row {currentRoute === item.id ? 'selected' : ''}"
              onclick={() => navigate(item.id)}
            >
              <span class="row-icon"><Icon name={item.icon} size={16} /></span>
              <span class="row-label">{item.title}</span>
            </button>
            <div class="pinned-reorder-actions">
              {#if i > 0}
                <button
                  type="button"
                  class="reorder-btn"
                  title="Move up"
                  onclick={(e) => movePinned(i, -1, e)}
                >
                  <Icon name="chevron-up" size={10} />
                </button>
              {/if}
              {#if i < pinned.length - 1}
                <button
                  type="button"
                  class="reorder-btn"
                  title="Move down"
                  onclick={(e) => movePinned(i, 1, e)}
                >
                  <Icon name="chevron-down" size={10} />
                </button>
              {/if}
            </div>
          </div>
        {/each}
      </div>

      <!-- 4. My Albums Tree with Collapsible Folders (§4.1) -->
      <div class="sidebar-group">
        <div class="section-header">My albums</div>
        {#each SAMPLE_ALBUMS as album}
          {#if album.isFolder}
            {@const isOpen = openFolders[album.id]}
            <div class="album-folder">
              <button
                type="button"
                class="sidebar-row folder-row"
                onclick={(e) => toggleFolder(album.id, e)}
              >
                <span class="folder-chevron">
                  <Icon name={isOpen ? 'chevron-down' : 'chevron-right'} size={12} />
                </span>
                <span class="row-icon"><Icon name="folder" size={16} /></span>
                <span class="row-label">{album.title}</span>
              </button>

              {#if isOpen && album.children}
                <div class="folder-children">
                  {#each album.children as child}
                    <button
                      type="button"
                      class="sidebar-row {currentAlbumId === child.id ? 'selected' : ''}"
                      onclick={() => navigate('album-detail', child.id)}
                    >
                      <span class="row-icon"><Icon name="album" size={16} /></span>
                      <span class="row-label">{child.title}</span>
                      <span class="row-count">{child.assetCount}</span>
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {:else}
            <button
              type="button"
              class="sidebar-row {currentAlbumId === album.id ? 'selected' : ''}"
              onclick={() => navigate('album-detail', album.id)}
            >
              <span class="row-icon"><Icon name="album" size={16} /></span>
              <span class="row-label">{album.title}</span>
              <span class="row-count">{album.assetCount}</span>
            </button>
          {/if}
        {/each}
      </div>

      <!-- 5. Utilities Section (§4.1) -->
      <div class="sidebar-group">
        <div class="section-header">Utilities</div>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'imports' ? 'selected' : ''}"
          onclick={() => navigate('imports')}
        >
          <span class="row-icon"><Icon name="arrow-down-to-line" size={16} /></span>
          <span class="row-label">Imports</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'duplicates' ? 'selected' : ''}"
          onclick={() => navigate('duplicates')}
        >
          <span class="row-icon"><Icon name="copy" size={16} /></span>
          <span class="row-label">Duplicates</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'hidden' ? 'selected' : ''}"
          onclick={() => navigate('hidden')}
        >
          <span class="row-icon"><Icon name="eye-off" size={16} /></span>
          <span class="row-label">Hidden</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'trash' ? 'selected' : ''}"
          onclick={() => navigate('trash')}
        >
          <span class="row-icon"><Icon name="trash" size={16} /></span>
          <span class="row-label">Recently Deleted</span>
        </button>

        <button
          type="button"
          class="sidebar-row {currentRoute === 'captured-by-me' ? 'selected' : ''}"
          onclick={() => navigate('captured-by-me')}
        >
          <span class="row-icon"><Icon name="camera" size={16} /></span>
          <span class="row-label">Captured by Me</span>
        </button>
      </div>
    </div>

    <!-- 6. Footer Sync Pill (§4.1) -->
    <div class="sidebar-footer">
      <button
        type="button"
        class="sync-pill-btn"
        onclick={() => (syncPopoverOpen = !syncPopoverOpen)}
        aria-label="Cloud sync status"
      >
        <span class="sync-icon-wrap">
          <Icon name={isUploading ? 'cloud-upload' : 'cloud'} size={15} />
        </span>
        <span class="sync-text">
          {isUploading ? 'Uploading 128 of 4,201' : 'Up to date'}
        </span>
        {#if isUploading}
          <div class="sync-progress-ring">
            <svg viewBox="0 0 16 16" width="14" height="14">
              <circle cx="8" cy="8" r="6" fill="none" stroke="var(--separator)" stroke-width="1.8" />
              <circle
                cx="8"
                cy="8"
                r="6"
                fill="none"
                stroke="var(--accent)"
                stroke-width="1.8"
                stroke-dasharray="37.7"
                stroke-dashoffset="12"
                stroke-linecap="round"
              />
            </svg>
          </div>
        {/if}
      </button>

      <SyncPopover bind:open={syncPopoverOpen} bind:isPaused={syncPaused} />
    </div>

    <!-- Drag Resizer Handle -->
    <button
      type="button"
      class="sidebar-resizer {isResizing ? 'resizing' : ''}"
      onmousedown={startResize}
      aria-label="Resize sidebar"
    ></button>
  </Surface>
{/if}

<style>
  :global(.frames-sidebar) {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
    user-select: none;
    z-index: 20;
    flex-shrink: 0;
  }

  .sidebar-top-inset {
    height: 38px;
    flex-shrink: 0;
  }

  .sidebar-scrollable {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 0 10px 10px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .sidebar-group {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .section-header {
    font-size: var(--text-size-caption);
    font-weight: 600;
    line-height: var(--text-lh-caption);
    color: var(--text-2);
    padding: 4px 10px 4px;
    letter-spacing: -0.1px;
  }

  .sidebar-row {
    display: flex;
    align-items: center;
    height: 28px;
    padding: 0 10px;
    border-radius: var(--r-sm);
    border: none;
    background: transparent;
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    font-weight: 500;
    line-height: var(--text-lh-body);
    text-align: left;
    cursor: pointer;
    gap: 8px;
    transition: background var(--dur-fast) var(--ease-standard);
  }

  .sidebar-row:hover {
    background: var(--fill-1);
  }

  .sidebar-row.selected {
    background: var(--fill-2);
    font-weight: 600;
  }

  .row-icon {
    display: flex;
    align-items: center;
    color: var(--accent);
    flex-shrink: 0;
  }

  .row-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-count {
    font-size: var(--text-size-caption);
    color: var(--text-3);
  }

  .pinned-row-wrap {
    display: flex;
    align-items: center;
    position: relative;
  }
  .pinned-row-wrap .sidebar-row {
    flex: 1;
  }

  .pinned-reorder-actions {
    display: none;
    position: absolute;
    right: 4px;
    align-items: center;
    gap: 2px;
  }
  .pinned-row-wrap:hover .pinned-reorder-actions {
    display: flex;
  }
  .reorder-btn {
    border: none;
    background: var(--fill-2);
    color: var(--text-2);
    border-radius: 4px;
    padding: 2px;
    cursor: pointer;
    display: flex;
    align-items: center;
  }

  .folder-row .folder-chevron {
    display: flex;
    align-items: center;
    color: var(--text-3);
    margin-right: -2px;
  }

  .folder-children {
    display: flex;
    flex-direction: column;
    padding-left: 14px;
    gap: 1px;
  }

  .sidebar-footer {
    position: relative;
    padding: 10px 12px;
    border-top: 1px solid var(--separator);
    flex-shrink: 0;
  }

  .sync-pill-btn {
    display: flex;
    align-items: center;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--r-pill);
    border: none;
    background: var(--fill-1);
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-caption);
    font-weight: 500;
    gap: 8px;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard);
  }

  .sync-pill-btn:hover {
    background: var(--fill-2);
  }

  .sync-icon-wrap {
    display: flex;
    color: var(--accent);
  }

  .sync-text {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }

  .sync-progress-ring {
    display: flex;
    align-items: center;
  }

  .sidebar-resizer {
    position: absolute;
    top: 0;
    right: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
    z-index: 30;
  }
  .sidebar-resizer:hover,
  .sidebar-resizer.resizing {
    background: var(--accent);
  }
</style>
