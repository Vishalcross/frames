<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Badge from '$lib/components/surface/Badge.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { getAssetByIndex, getThumbnailDataUrl } from '$lib/mock/mockRepo';
  import type { Asset } from '$lib/types';

  let {
    onrestore,
    ondeletepermanently
  }: {
    onrestore?: (ids: string[]) => void;
    ondeletepermanently?: (ids: string[]) => void;
  } = $props();

  // Mock sample deleted items with days remaining (§4.12)
  const trashItems = $derived(
    [0, 1, 2, 3, 4, 5].map((i) => {
      const asset = getAssetByIndex(1000 + i * 43);
      const daysRemaining = 30 - (i * 5 + 2);
      return {
        asset,
        daysRemaining: Math.max(1, daysRemaining),
        thumb: getThumbnailDataUrl(asset, 'thumb')
      };
    })
  );

  let selectedTrashIds = $state<Set<string>>(new Set());

  function toggleSelect(id: string) {
    const s = new Set(selectedTrashIds);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    selectedTrashIds = s;
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
        <Button variant="standard" onclick={() => onrestore?.(Array.from(selectedTrashIds))}>
          Recover ({selectedTrashIds.size})
        </Button>
        <Button variant="danger" onclick={() => ondeletepermanently?.(Array.from(selectedTrashIds))}>
          Delete Permanently
        </Button>
      </div>
    {/if}
  </header>

  <div class="trash-grid">
    {#each trashItems as item}
      {@const isSelected = selectedTrashIds.has(item.asset.id)}
      <button
        type="button"
        class="trash-item-card {isSelected ? 'selected' : ''}"
        onclick={() => toggleSelect(item.asset.id)}
      >
        <div class="trash-thumb-wrap">
          <img src={item.thumb} alt="" loading="lazy" />

          {#if isSelected}
            <div class="trash-check">
              <Icon name="check" size={13} stroke={2.4} />
            </div>
          {/if}
        </div>

        <!-- Days remaining label (§4.12) -->
        <div class="days-remaining-pill">
          {item.daysRemaining} days left
        </div>
      </button>
    {/each}
  </div>
</div>

<style>
  .trash-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 32px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .trash-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    border-bottom: 1px solid var(--separator);
    padding-bottom: 16px;
  }

  .trash-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0 0 4px;
  }

  .trash-sub {
    font-size: var(--text-size-body);
    line-height: var(--text-lh-body);
    color: var(--text-2);
    margin: 0;
    max-width: 540px;
  }

  .trash-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .trash-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: 16px;
  }

  .trash-item-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    border: none;
    background: transparent;
    cursor: pointer;
    padding: 0;
  }

  .trash-thumb-wrap {
    position: relative;
    width: 100%;
    aspect-ratio: 1;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--fill-2);
    box-shadow: var(--shadow-1);
  }

  .trash-thumb-wrap img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .trash-item-card.selected .trash-thumb-wrap {
    box-shadow: inset 0 0 0 3px var(--accent);
  }

  .trash-check {
    position: absolute;
    top: 6px;
    left: 6px;
    width: 20px;
    height: 20px;
    border-radius: var(--r-pill);
    background: var(--accent);
    color: var(--accent-fg);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* Days remaining label (§4.12) */
  .days-remaining-pill {
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: var(--text-2);
  }
</style>
