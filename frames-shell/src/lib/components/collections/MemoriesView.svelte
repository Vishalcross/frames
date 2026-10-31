<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';
  import { tauriRepo, getAssetUrl } from '$lib/services/tauriRepo';

  let {
    onselectmemory
  }: {
    onselectmemory?: (id: string) => void;
  } = $props();

  const memories = $derived.by(() => {
    const albums = tauriRepo.getAlbums();
    return albums.map((a) => ({
      id: a.id,
      title: a.title,
      subtitle: `${a.assetCount} photos`,
      count: a.assetCount,
      thumb: a.keyAssetId ? getAssetUrl('preview', a.keyAssetId) : ''
    }));
  });
</script>

<div class="memories-view-container">
  <header class="memories-header">
    <h2 class="memories-title">Memories</h2>
    <span class="memories-subtitle">Rediscover your past journeys and special moments</span>
  </header>

  {#if memories.length === 0}
    <div class="memories-empty">
      <div class="empty-icon-wrap">
        <Icon name="sparkles" size={32} />
      </div>
      <p class="empty-title">No Memories Yet</p>
      <p class="empty-sub">As you import albums and photos, Frames automatically highlights your key moments.</p>
    </div>
  {:else}
    <div class="memories-grid">
      {#each memories as mem}
        <button
          type="button"
          class="memory-card-large"
          onclick={() => onselectmemory?.(mem.id)}
        >
          {#if mem.thumb}
            <img src={mem.thumb} alt={mem.title} loading="lazy" />
          {:else}
            <div class="memory-placeholder"></div>
          {/if}

          <div class="memory-scrim">
            <div class="memory-badge">
              <Icon name="sparkles" size={13} />
              <span>Memory</span>
            </div>
            <h3 class="memory-card-title">{mem.title}</h3>
            <span class="memory-card-sub">{mem.subtitle}</span>
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .memories-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 32px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .memories-header {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .memories-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0;
  }

  .memories-subtitle {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }

  .memories-empty {
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

  .memories-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 20px;
  }

  .memory-card-large {
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

  .memory-card-large:hover {
    transform: scale(1.02);
  }

  .memory-card-large img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .memory-placeholder {
    width: 100%;
    height: 100%;
    background: var(--fill-2);
  }

  .memory-scrim {
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

  .memory-badge {
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

  .memory-card-title {
    font-size: 20px;
    font-weight: 700;
    margin: 0 0 2px;
    letter-spacing: -0.3px;
  }

  .memory-card-sub {
    font-size: 12px;
    opacity: 0.85;
  }
</style>
