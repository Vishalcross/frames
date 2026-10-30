<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';
  import {
    SAMPLE_ALBUMS,
    SAMPLE_MEMORIES,
    SAMPLE_PEOPLE,
    SAMPLE_TRIPS
  } from '$lib/mock/sampleData';
  import { getThumbnailDataUrl, getAssetByIndex } from '$lib/mock/mockRepo';
  import type { AlbumItem } from '$lib/types';

  let {
    onnavigatealbum,
    onnavigateroute
  }: {
    onnavigatealbum?: (albumId: string) => void;
    onnavigateroute?: (route: string) => void;
  } = $props();

  function getCardThumb(assetId: string) {
    const idx = parseInt(assetId.replace('asset_', ''), 10) || 0;
    return getThumbnailDataUrl(getAssetByIndex(idx), 'thumb');
  }
</script>

<div class="collections-landing-container">
  <!-- 1. Memories Rail (§4.9) -->
  <section class="collection-rail-section">
    <div class="rail-header">
      <span class="rail-title">Memories</span>
      <button type="button" class="rail-see-all" onclick={() => onnavigateroute?.('memories')}>
        See All
      </button>
    </div>
    <div class="rail-scroll">
      {#each SAMPLE_MEMORIES as item}
        <button
          type="button"
          class="memory-card"
          onclick={() => onnavigateroute?.('memories')}
        >
          <img src={getCardThumb(item.keyAssetId)} alt="" class="card-thumb" loading="lazy" />
          <div class="memory-card-scrim">
            <span class="memory-title">{item.title}</span>
            <span class="memory-sub">{item.subtitle}</span>
          </div>
        </button>
      {/each}
    </div>
  </section>

  <!-- 2. Albums Rail (§4.9: 1:1, radius 14, title 13 semibold, count 12 secondary, stacked-card folders) -->
  <section class="collection-rail-section">
    <div class="rail-header">
      <span class="rail-title">Albums</span>
    </div>
    <div class="rail-scroll">
      {#each SAMPLE_ALBUMS as album}
        <button
          type="button"
          class="album-card"
          onclick={() => onnavigatealbum?.(album.id)}
        >
          <div class="album-thumb-container {album.isFolder ? 'is-folder-stack' : ''}">
            <img src={getCardThumb(album.keyAssetId)} alt="" class="card-thumb" loading="lazy" />
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
  </section>

  <!-- 3. People & Pets Rail (§4.9) -->
  <section class="collection-rail-section">
    <div class="rail-header">
      <span class="rail-title">People & Pets</span>
      <button type="button" class="rail-see-all" onclick={() => onnavigateroute?.('people-pets')}>
        See All
      </button>
    </div>
    <div class="rail-scroll">
      {#each SAMPLE_PEOPLE as person}
        <button
          type="button"
          class="person-circle-card"
          onclick={() => onnavigateroute?.('people-pets')}
        >
          <div class="person-avatar">
            <img src={getCardThumb(person.keyAssetId)} alt="" loading="lazy" />
          </div>
          <span class="person-name">{person.title}</span>
          <span class="person-count">{person.subtitle}</span>
        </button>
      {/each}
    </div>
  </section>

  <!-- 4. Trips Rail (§4.9) -->
  <section class="collection-rail-section">
    <div class="rail-header">
      <span class="rail-title">Trips</span>
      <button type="button" class="rail-see-all" onclick={() => onnavigateroute?.('trips')}>
        See All
      </button>
    </div>
    <div class="rail-scroll">
      {#each SAMPLE_TRIPS as trip}
        <button
          type="button"
          class="trip-card"
          onclick={() => onnavigateroute?.('trips')}
        >
          <img src={getCardThumb(trip.keyAssetId)} alt="" class="card-thumb" loading="lazy" />
          <div class="trip-scrim">
            <span class="trip-title">{trip.title}</span>
            <span class="trip-sub">{trip.subtitle}</span>
          </div>
        </button>
      {/each}
    </div>
  </section>

  <!-- 5. Media Types Rail (§4.9) -->
  <section class="collection-rail-section">
    <div class="rail-header">
      <span class="rail-title">Media Types</span>
    </div>
    <div class="media-types-grid">
      <button type="button" class="media-type-pill" onclick={() => onnavigateroute?.('videos')}>
        <Icon name="video" size={16} />
        <span class="media-type-label">Videos</span>
        <span class="media-type-count">14,200</span>
      </button>

      <button type="button" class="media-type-pill" onclick={() => onnavigateroute?.('favorites')}>
        <Icon name="heart" size={16} />
        <span class="media-type-label">Favorites</span>
        <span class="media-type-count">8,150</span>
      </button>

      <button type="button" class="media-type-pill" onclick={() => onnavigateroute?.('captured-by-me')}>
        <Icon name="camera" size={16} />
        <span class="media-type-label">Captured by Me</span>
        <span class="media-type-count">64,500</span>
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

  .rail-see-all {
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: var(--accent);
    border: none;
    background: transparent;
    cursor: pointer;
  }

  .rail-scroll {
    display: flex;
    gap: 16px;
    overflow-x: auto;
    padding-bottom: 8px;
  }

  /* Memory Card */
  .memory-card {
    position: relative;
    width: 220px;
    height: 160px;
    border-radius: 14px;
    overflow: hidden;
    border: none;
    padding: 0;
    flex-shrink: 0;
    background: var(--fill-2);
    cursor: pointer;
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-fast) var(--ease-standard);
  }
  .memory-card:hover {
    transform: scale(1.02);
  }

  .card-thumb {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .memory-card-scrim {
    position: absolute;
    inset: auto 0 0 0;
    height: 60%;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.75) 0%, transparent 100%);
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 12px;
    text-align: left;
    color: #ffffff;
  }
  .memory-title {
    font-weight: 600;
    font-size: var(--text-size-body);
  }
  .memory-sub {
    font-size: var(--text-size-caption);
    opacity: 0.8;
  }

  /* Album Card 1:1, radius 14 (§4.9) */
  .album-card {
    display: flex;
    flex-direction: column;
    width: 160px;
    border: none;
    padding: 0;
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    gap: 8px;
    text-align: left;
  }

  .album-thumb-container {
    position: relative;
    width: 160px;
    height: 160px;
    border-radius: 14px;
    overflow: hidden;
    background: var(--fill-2);
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-fast) var(--ease-standard);
  }
  .album-card:hover .album-thumb-container {
    transform: scale(1.03);
  }

  .folder-badge {
    position: absolute;
    top: 8px;
    right: 8px;
    background: rgba(0, 0, 0, 0.65);
    color: #ffffff;
    border-radius: var(--r-pill);
    padding: 4px;
    display: flex;
  }

  .album-card-info {
    display: flex;
    flex-direction: column;
  }
  .album-title {
    font-size: var(--text-size-body);
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

  /* People & Pets */
  .person-circle-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 120px;
    border: none;
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    gap: 6px;
  }
  .person-avatar {
    width: 90px;
    height: 90px;
    border-radius: 50%;
    overflow: hidden;
    box-shadow: var(--shadow-1);
    background: var(--fill-2);
  }
  .person-avatar img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .person-name {
    font-size: var(--text-size-body);
    font-weight: 600;
    color: var(--text-1);
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 100%;
  }
  .person-count {
    font-size: 11px;
    color: var(--text-3);
  }

  /* Trips */
  .trip-card {
    position: relative;
    width: 240px;
    height: 150px;
    border-radius: 14px;
    overflow: hidden;
    border: none;
    padding: 0;
    background: var(--fill-2);
    cursor: pointer;
    flex-shrink: 0;
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-fast) var(--ease-standard);
  }
  .trip-card:hover {
    transform: scale(1.02);
  }
  .trip-scrim {
    position: absolute;
    inset: auto 0 0 0;
    height: 60%;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.75) 0%, transparent 100%);
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 12px;
    text-align: left;
    color: #ffffff;
  }
  .trip-title {
    font-weight: 600;
    font-size: var(--text-size-body);
  }
  .trip-sub {
    font-size: var(--text-size-caption);
    opacity: 0.8;
  }

  /* Media Types grid */
  .media-types-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 10px;
  }

  .media-type-pill {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 14px;
    border-radius: var(--r-md);
    background: var(--fill-1);
    border: none;
    color: var(--text-1);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard);
  }
  .media-type-pill:hover {
    background: var(--fill-2);
  }
  .media-type-label {
    flex: 1;
    font-size: var(--text-size-body);
    font-weight: 600;
    text-align: left;
  }
  .media-type-count {
    font-size: var(--text-size-caption);
    color: var(--text-3);
    font-family: var(--font-mono);
  }
</style>
