<script lang="ts">
  import { tauriRepo, getAssetUrl } from '$lib/services/tauriRepo';

  let {
    ondrillyear
  }: {
    ondrillyear?: (year: number) => void;
  } = $props();

  const yearData = $derived.by(() => {
    const monthGroups = tauriRepo.getMonthGroups();
    const assets = tauriRepo.getAssets();
    const map = new Map<number, { count: number; keyAssetId: string }>();

    for (const mg of monthGroups) {
      const cur = map.get(mg.year) || { count: 0, keyAssetId: mg.key_asset_id };
      cur.count += mg.count;
      if (!cur.keyAssetId && mg.key_asset_id) cur.keyAssetId = mg.key_asset_id;
      map.set(mg.year, cur);
    }

    if (map.size === 0 && assets.length > 0) {
      for (const a of assets) {
        const y = a.year || new Date(a.takenAt).getFullYear();
        const cur = map.get(y) || { count: 0, keyAssetId: a.id };
        cur.count++;
        map.set(y, cur);
      }
    }

    return Array.from(map.entries())
      .sort((a, b) => b[0] - a[0])
      .map(([year, val]) => ({
        year,
        count: val.count,
        thumbUrl: val.keyAssetId ? getAssetUrl('preview', val.keyAssetId) : ''
      }));
  });
</script>

<div class="years-view-container">
  {#if yearData.length === 0}
    <div class="years-empty-notice">
      <p>No photos or videos indexed in library yet.</p>
    </div>
  {:else}
    <div class="years-grid">
      {#each yearData as item}
        <button
          type="button"
          class="year-card"
          onclick={() => ondrillyear?.(item.year)}
          aria-label="Year {item.year}, {item.count} items"
        >
          {#if item.thumbUrl}
            <img src={item.thumbUrl} alt="" class="year-bg-img" loading="lazy" />
          {:else}
            <div class="year-bg-placeholder"></div>
          {/if}

          <!-- Bottom-left scrim with Year and photo count (§4.3) -->
          <div class="year-scrim">
            <div class="year-title">{item.year}</div>
            <div class="year-count">{item.count.toLocaleString()} {item.count === 1 ? 'photo' : 'photos'}</div>
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .years-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 60px;
    box-sizing: border-box;
  }

  .years-empty-notice {
    padding: 60px 20px;
    text-align: center;
    color: var(--text-3);
    font-size: var(--text-size-body);
    font-style: italic;
  }

  /* Years cards 16:10, radius 14, gap 12, 1-3 per row (§4.3) */
  .years-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 12px;
  }

  .year-card {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: 14px;
    overflow: hidden;
    border: none;
    padding: 0;
    background: var(--fill-2);
    box-shadow: var(--shadow-1);
    cursor: pointer;
    text-align: left;
    transition: transform var(--dur-short) var(--ease-standard);
  }

  .year-card:hover {
    transform: scale(1.015);
  }

  .year-bg-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .year-bg-placeholder {
    width: 100%;
    height: 100%;
    background: var(--fill-2);
  }

  /* 34/40 bold white on bottom-left scrim (§4.3) */
  .year-scrim {
    position: absolute;
    inset: 0;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.75) 0%, rgba(0, 0, 0, 0.1) 50%, transparent 100%);
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 20px 24px;
    box-sizing: border-box;
    color: #ffffff;
  }

  .year-title {
    font-size: 34px;
    line-height: 40px;
    font-weight: 700;
    letter-spacing: -0.5px;
  }

  .year-count {
    font-size: var(--text-size-caption);
    opacity: 0.85;
    margin-top: 2px;
  }
</style>
