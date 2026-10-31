<script lang="ts">
  import Button from '$lib/components/surface/Button.svelte';
  import Badge from '$lib/components/surface/Badge.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { tauriRepo, getAssetUrl } from '$lib/services/tauriRepo';
  import { invoke } from '@tauri-apps/api/core';

  let {
    onrestore,
    ondeletepermanently
  }: {
    onrestore?: (ids: string[]) => void;
    ondeletepermanently?: (ids: string[]) => void;
  } = $props();

  let selectedTrashIds = $state<Set<string>>(new Set());

  const trashItems = $derived.by(() => {
    const list = tauriRepo.getTrashItems();
    return list.map((item) => ({
      id: item.id,
      filename: item.filename,
      daysRemaining: item.days_remaining,
      thumb: getAssetUrl('thumb', item.id)
    }));
  });

  function toggleSelect(id: string) {
    const s = new Set(selectedTrashIds);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    selectedTrashIds = s;
  }

  async function handleRecover() {
    const ids = Array.from(selectedTrashIds);
    if (ids.length === 0) return;
    try {
      await invoke('restore_assets', { ids });
      await tauriRepo.loadBootstrap();
      selectedTrashIds = new Set();
      onrestore?.(ids);
    } catch (e) {
      console.error('Failed to restore assets:', e);
    }
  }

  async function handleDeletePermanently() {
    const ids = Array.from(selectedTrashIds);
    if (ids.length === 0) return;
    try {
      await invoke('delete_trash_now', { ids });
      await tauriRepo.loadBootstrap();
      selectedTrashIds = new Set();
      ondeletepermanently?.(ids);
    } catch (e) {
      console.error('Failed to purge assets:', e);
    }
  }
</script>

<div class="trash-view-container">
  <header class="trash-header">
    <div class="trash-header-info">
      <h2 class="trash-title">Recently Deleted</h2>
      <p class="trash-sub">
        Items show the days remaining before permanent deletion. After 30 days, items are automatically purged from MEGA S4.
      </p>
    </div>

    {#if selectedTrashIds.size > 0}
      <div class="trash-actions">
        <Button variant="standard" onclick={handleRecover}>
          Recover ({selectedTrashIds.size})
        </Button>
        <Button variant="danger" onclick={handleDeletePermanently}>
          Delete Permanently
        </Button>
      </div>
    {/if}
  </header>

  {#if trashItems.length === 0}
    <div class="trash-empty-state">
      <div class="empty-trash-icon">
        <Icon name="trash" size={32} />
      </div>
      <p class="empty-trash-title">Trash is Empty</p>
      <p class="empty-trash-sub">Photos and videos you delete will appear here for 30 days before being permanently removed.</p>
    </div>
  {:else}
    <div class="trash-grid">
      {#each trashItems as item}
        {@const isSelected = selectedTrashIds.has(item.id)}
        <div
          class="trash-tile {isSelected ? 'selected' : ''}"
          onclick={() => toggleSelect(item.id)}
          role="button"
          tabindex="0"
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              toggleSelect(item.id);
            }
          }}
          aria-label="{item.filename}, {item.daysRemaining} days remaining"
        >
          <img src={item.thumb} alt={item.filename} class="trash-img" loading="lazy" />

          <!-- Days remaining label (§4.12) -->
          <div class="trash-badge-wrap">
            <Badge variant="pill">{item.daysRemaining}d left</Badge>
          </div>

          {#if isSelected}
            <div class="trash-check-badge">
              <Icon name="check" size={14} stroke={2.5} />
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .trash-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 60px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .trash-header {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--separator);
  }

  .trash-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0 0 4px;
  }

  .trash-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
    margin: 0;
    max-width: 500px;
  }

  .trash-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .trash-empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 80px 20px;
    text-align: center;
    gap: 12px;
  }

  .empty-trash-icon {
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--fill-2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-3);
  }

  .empty-trash-title {
    font-size: var(--text-size-title);
    font-weight: 600;
    color: var(--text-1);
    margin: 0;
  }

  .empty-trash-sub {
    font-size: var(--text-size-body);
    color: var(--text-2);
    margin: 0;
    max-width: 400px;
  }

  .trash-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 8px;
  }

  .trash-tile {
    position: relative;
    aspect-ratio: 1;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--fill-2);
    cursor: pointer;
    box-sizing: border-box;
  }

  .trash-tile.selected {
    outline: 3px solid var(--accent);
    outline-offset: -3px;
  }

  .trash-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .trash-badge-wrap {
    position: absolute;
    bottom: 8px;
    left: 8px;
    z-index: 5;
  }

  .trash-check-badge {
    position: absolute;
    top: 8px;
    left: 8px;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--accent);
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }
</style>
