<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';
  import type { AlbumItem } from '$lib/types';
  import { getAssetUrl } from '$lib/services/tauriRepo';

  let {
    albums = [],
    onnavigatealbum,
    onnavigateroute
  }: {
    albums?: AlbumItem[];
    onnavigatealbum?: (albumId: string) => void;
    onnavigateroute?: (route: string) => void;
  } = $props();

  function getCardThumb(assetId?: string) {
    if (!assetId) return '';
    return getAssetUrl('thumb', assetId);
  }
</script>

<div class="collections-landing-container">
  <!-- 1. Albums Section (§4.9) -->
  <section class="collection-rail-section">
    <div class="rail-header">
      <span class="rail-title">Albums</span>
    </div>
    {#if albums.length === 0}
      <div class="empty-rail-message">No albums found in library. Create an album or sync with MEGA S4.</div>
    {:else}
      <div class="rail-scroll">
        {#each albums as album}
          <button
            type="button"
            class="album-card"
            onclick={() => onnavigatealbum?.(album.id)}
          >
            <div class="album-thumb-container {album.isFolder ? 'is-folder-stack' : ''}">
              {#if album.keyAssetId}
                <img src={getCardThumb(album.keyAssetId)} alt="" class="card-thumb" loading="lazy" />
              {:else}
                <div class="card-placeholder-icon">
                  <Icon name="album" size={28} />
                </div>
              {/if}
              {#if album.isFolder}
                <div class="folder-badge">
                  <Icon name="folder" size={14} />
                </div>
              {/if}
            </div>
            <div class="album-card-info">
              <span class="album-title">{album.title}</span>
              <span class="album-count">{album.assetCount} photos</span>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  </section>

  <!-- 2. Media Types Rail (§4.9) -->
  <section class="collection-rail-section">
    <div class="rail-header">
      <span class="rail-title">Media Types</span>
    </div>
    <div class="media-types-grid">
      <button type="button" class="media-type-pill" onclick={() => onnavigateroute?.('videos')}>
        <Icon name="video" size={16} />
        <span class="media-type-label">Videos</span>
      </button>

      <button type="button" class="media-type-pill" onclick={() => onnavigateroute?.('favorites')}>
        <Icon name="heart" size={16} />
        <span class="media-type-label">Favorites</span>
      </button>

      <button type="button" class="media-type-pill" onclick={() => onnavigateroute?.('captured-by-me')}>
        <Icon name="camera" size={16} />
        <span class="media-type-label">Captured by Me</span>
      </button>
    </div>
  </section>
</div>

<style>
  .collections-landing-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 60px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 36px;
  }

  .collection-rail-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .rail-header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }

  .rail-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    letter-spacing: -0.3px;
  }

  .rail-scroll {
    display: flex;
    gap: 16px;
    overflow-x: auto;
    padding-bottom: 8px;
    scrollbar-width: thin;
  }

  .empty-rail-message {
    font-size: var(--text-size-body);
    color: var(--text-3);
    padding: 24px 0;
    font-style: italic;
  }

  .card-placeholder-icon {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--fill-2);
    color: var(--text-3);
  }

  /* Album Cards: 1:1, radius 14, title 13 semibold, count 12 secondary (§4.9) */
  .album-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    width: 160px;
    flex-shrink: 0;
  }

  .album-thumb-container {
    position: relative;
    width: 160px;
    height: 160px;
    border-radius: 14px;
    overflow: hidden;
    background: var(--fill-2);
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-short) var(--ease-standard);
  }

  .album-card:hover .album-thumb-container {
    transform: scale(1.02);
  }

  .card-thumb {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .album-card-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .album-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-1);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .album-count {
    font-size: 12px;
    color: var(--text-2);
  }

  .is-folder-stack::after {
    content: '';
    position: absolute;
    top: -4px;
    left: 8px;
    right: 8px;
    height: 6px;
    border-radius: 10px 10px 0 0;
    background: var(--fill-3);
    z-index: -1;
  }

  .folder-badge {
    position: absolute;
    bottom: 8px;
    right: 8px;
    width: 24px;
    height: 24px;
    border-radius: var(--r-full);
    background: var(--surface-overlay);
    backdrop-filter: blur(12px);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-1);
  }

  .media-types-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
  }

  .media-type-pill {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 38px;
    padding: 0 16px;
    border-radius: var(--r-full);
    background: var(--fill-1);
    border: 1px solid var(--separator);
    color: var(--text-1);
    font-size: var(--text-size-body);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard);
  }

  .media-type-pill:hover {
    background: var(--fill-2);
  }
</style>
