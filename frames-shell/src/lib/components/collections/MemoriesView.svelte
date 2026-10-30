<script lang="ts">
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { SAMPLE_MEMORIES } from '$lib/mock/sampleData';
  import { getThumbnailDataUrl, getAssetByIndex } from '$lib/mock/mockRepo';

  let {
    onselectmemory
  }: {
    onselectmemory?: (id: string) => void;
  } = $props();

  function getCardThumb(keyAssetId: string) {
    const idx = parseInt(keyAssetId.replace('asset_', ''), 10) || 0;
    return getThumbnailDataUrl(getAssetByIndex(idx), 'preview');
  }
</script>

<div class="memories-view-container">
  <header class="memories-header">
    <h2 class="memories-title">Memories</h2>
    <span class="memories-subtitle">Rediscover your past journeys and special moments</span>
  </header>

  <div class="memories-grid">
    {#each SAMPLE_MEMORIES as mem}
      <button
        type="button"
        class="memory-card-large"
        onclick={() => onselectmemory?.(mem.id)}
      >
        <img src={getCardThumb(mem.keyAssetId)} alt={mem.title} loading="lazy" />

        <div class="memory-scrim">
          <div class="memory-badge">
            <Icon name="sparkles" size={13} />
            <span>Memory</span>
          </div>
          <h3 class="memory-card-title">{mem.title}</h3>
          <span class="memory-card-sub">{mem.subtitle} · {mem.count} photos</span>
        </div>
      </button>
    {/each}
  </div>
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
    border-bottom: 1px solid var(--separator);
    padding-bottom: 12px;
  }

  .memories-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0 0 2px;
  }

  .memories-subtitle {
    font-size: var(--text-size-body);
    color: var(--text-2);
  }

  .memories-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
    gap: 20px;
  }

  .memory-card-large {
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
  .memory-card-large:hover {
    transform: scale(1.015);
    box-shadow: var(--shadow-2);
  }

  .memory-card-large img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .memory-scrim {
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

  .memory-badge {
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

  .memory-card-title {
    font-size: 22px;
    font-weight: 700;
    margin: 0 0 4px;
  }

  .memory-card-sub {
    font-size: var(--text-size-caption);
    opacity: 0.8;
  }
</style>
