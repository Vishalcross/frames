<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';
  import { SAMPLE_TRIPS } from '$lib/mock/sampleData';
  import { getThumbnailDataUrl, getAssetByIndex } from '$lib/mock/mockRepo';

  let {
    onselecttrip
  }: {
    onselecttrip?: (id: string) => void;
  } = $props();

  function getTripThumb(keyAssetId: string) {
    const idx = parseInt(keyAssetId.replace('asset_', ''), 10) || 0;
    return getThumbnailDataUrl(getAssetByIndex(idx), 'preview');
  }
</script>

<div class="trips-view-container">
  <header class="trips-header">
    <h2 class="trips-title">Trips</h2>
    <span class="trips-sub">Organised automatically by location clusters and dates</span>
  </header>

  <div class="trips-grid">
    {#each SAMPLE_TRIPS as trip}
      <button
        type="button"
        class="trip-full-card"
        onclick={() => onselecttrip?.(trip.id)}
      >
        <img src={getTripThumb(trip.keyAssetId)} alt={trip.title} loading="lazy" />

        <div class="trip-scrim">
          <div class="trip-icon-badge">
            <Icon name="compass" size={13} />
            <span>Trip</span>
          </div>
          <h3 class="trip-card-title">{trip.title}</h3>
          <span class="trip-card-sub">{trip.subtitle}</span>
        </div>
      </button>
    {/each}
  </div>
</div>

<style>
  .trips-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 32px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .trips-header {
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--separator);
    padding-bottom: 12px;
  }

  .trips-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0 0 2px;
  }

  .trips-sub {
    font-size: var(--text-size-body);
    color: var(--text-2);
  }

  .trips-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
    gap: 20px;
  }

  .trip-full-card {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: var(--r-lg);
    overflow: hidden;
    border: none;
    padding: 0;
    background: var(--fill-2);
    cursor: pointer;
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-fast) var(--ease-standard),
                box-shadow var(--dur-fast) var(--ease-standard);
  }
  .trip-full-card:hover {
    transform: scale(1.015);
    box-shadow: var(--shadow-2);
  }

  .trip-full-card img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .trip-scrim {
    position: absolute;
    inset: auto 0 0 0;
    height: 60%;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.8) 0%, transparent 100%);
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 20px;
    text-align: left;
    color: #ffffff;
  }

  .trip-icon-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    opacity: 0.85;
    margin-bottom: 4px;
  }

  .trip-card-title {
    font-size: 22px;
    font-weight: 700;
    margin: 0 0 4px;
  }

  .trip-card-sub {
    font-size: var(--text-size-caption);
    opacity: 0.8;
  }
</style>
