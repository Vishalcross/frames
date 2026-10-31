<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';
  import { tauriRepo, getAssetUrl } from '$lib/services/tauriRepo';

  let {
    onselecttrip
  }: {
    onselecttrip?: (id: string) => void;
  } = $props();

  const trips = $derived.by(() => {
    const albums = tauriRepo.getAlbums();
    return albums.map((a) => ({
      id: a.id,
      title: a.title,
      subtitle: `${a.assetCount} items`,
      thumb: a.keyAssetId ? getAssetUrl('preview', a.keyAssetId) : ''
    }));
  });
</script>

<div class="trips-view-container">
  <header class="trips-header">
    <h2 class="trips-title">Trips</h2>
    <span class="trips-sub">Organised automatically by location clusters and dates</span>
  </header>

  {#if trips.length === 0}
    <div class="trips-empty">
      <div class="empty-icon-wrap">
        <Icon name="compass" size={32} />
      </div>
      <p class="empty-title">No Trips Recorded</p>
      <p class="empty-sub">Import photos with location metadata to explore your travels by destination.</p>
    </div>
  {:else}
    <div class="trips-grid">
      {#each trips as trip}
        <button
          type="button"
          class="trip-full-card"
          onclick={() => onselecttrip?.(trip.id)}
        >
          {#if trip.thumb}
            <img src={trip.thumb} alt={trip.title} loading="lazy" />
          {:else}
            <div class="trip-placeholder"></div>
          {/if}

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
  {/if}
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
    gap: 4px;
    border-bottom: 1px solid var(--separator);
    padding-bottom: 12px;
  }

  .trips-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0;
  }

  .trips-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }

  .trips-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 80px 20px;
    text-align: center;
    gap: 12px;
  }

  .empty-icon-wrap {
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--fill-2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
  }

  .empty-title {
    font-size: var(--text-size-title);
    font-weight: 600;
    color: var(--text-1);
    margin: 0;
  }

  .empty-sub {
    font-size: var(--text-size-body);
    color: var(--text-2);
    margin: 0;
    max-width: 380px;
  }

  .trips-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 20px;
  }

  .trip-full-card {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: var(--r-xl);
    overflow: hidden;
    border: none;
    padding: 0;
    background: var(--fill-2);
    box-shadow: var(--shadow-2);
    cursor: pointer;
    text-align: left;
    transition: transform var(--dur-short) var(--ease-standard);
  }

  .trip-full-card:hover {
    transform: scale(1.02);
  }

  .trip-full-card img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .trip-placeholder {
    width: 100%;
    height: 100%;
    background: var(--fill-2);
  }

  .trip-scrim {
    position: absolute;
    inset: 0;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.8) 0%, rgba(0, 0, 0, 0.15) 50%, transparent 100%);
    padding: 20px;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    color: #ffffff;
    box-sizing: border-box;
  }

  .trip-icon-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    border-radius: var(--r-full);
    background: rgba(255, 255, 255, 0.25);
    backdrop-filter: blur(10px);
    font-size: 11px;
    font-weight: 600;
    width: fit-content;
    margin-bottom: 8px;
  }

  .trip-card-title {
    font-size: 20px;
    font-weight: 700;
    margin: 0 0 2px;
    letter-spacing: -0.3px;
  }

  .trip-card-sub {
    font-size: 12px;
    opacity: 0.85;
  }
</style>
