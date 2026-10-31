<script lang="ts">
  import { tauriRepo, getAssetUrl } from '$lib/services/tauriRepo';

  let {
    year = 2026,
    ondrillmonth
  }: {
    year?: number;
    ondrillmonth?: (month: number) => void;
  } = $props();

  const monthNames = [
    'January', 'February', 'March', 'April', 'May', 'June',
    'July', 'August', 'September', 'October', 'November', 'December'
  ];

  const monthCards = $derived.by(() => {
    const monthGroups = tauriRepo.getMonthGroups().filter((mg) => mg.year === year);
    const assets = tauriRepo.getAssets().filter((a) => a.year === year);

    // If monthGroups has items for this year
    if (monthGroups.length > 0) {
      return monthGroups
        .sort((a, b) => b.month - a.month)
        .map((mg) => {
          const monthAssets = assets.filter((a) => a.month === mg.month);
          const largeThumb = mg.key_asset_id ? getAssetUrl('preview', mg.key_asset_id) : '';
          const smallThumbs = monthAssets
            .filter((a) => a.id !== mg.key_asset_id)
            .slice(0, 3)
            .map((a) => getAssetUrl('thumb', a.id));

          return {
            month: mg.month,
            name: monthNames[mg.month - 1] || `Month ${mg.month}`,
            count: mg.count,
            largeThumb,
            smallThumbs
          };
        });
    }

    // Fallback: group assets by month
    const map = new Map<number, typeof assets>();
    for (const a of assets) {
      const m = a.month || (new Date(a.takenAt).getMonth() + 1);
      const list = map.get(m) || [];
      list.push(a);
      map.set(m, list);
    }

    return Array.from(map.entries())
      .sort((a, b) => b[0] - a[0])
      .map(([m, list]) => ({
        month: m,
        name: monthNames[m - 1] || `Month ${m}`,
        count: list.length,
        largeThumb: list[0] ? getAssetUrl('preview', list[0].id) : '',
        smallThumbs: list.slice(1, 4).map((a) => getAssetUrl('thumb', a.id))
      }));
  });
</script>

<div class="months-view-container">
  {#if monthCards.length === 0}
    <div class="months-empty-notice">
      <p>No photos or videos for {year}.</p>
    </div>
  {:else}
    <div class="months-grid">
      {#each monthCards as card}
        <button
          type="button"
          class="month-card"
          onclick={() => ondrillmonth?.(card.month)}
          aria-label="{card.name} {year}, {card.count} items"
        >
          <!-- Key-photo mosaic (1 large + 3 small) (§4.3) -->
          <div class="mosaic-container">
            <div class="mosaic-large">
              {#if card.largeThumb}
                <img src={card.largeThumb} alt="" loading="lazy" />
              {:else}
                <div class="mosaic-placeholder"></div>
              {/if}
            </div>
            <div class="mosaic-small-col">
              {#each [0, 1, 2] as i}
                <div class="mosaic-small">
                  {#if card.smallThumbs[i]}
                    <img src={card.smallThumbs[i]} alt="" loading="lazy" />
                  {:else}
                    <div class="mosaic-placeholder-small"></div>
                  {/if}
                </div>
              {/each}
            </div>
          </div>

          <!-- Month Scrim & Info (§4.3) -->
          <div class="month-card-footer">
            <div class="month-name">{card.name}</div>
            <div class="month-sub">{card.count.toLocaleString()} {card.count === 1 ? 'photo' : 'photos'}</div>
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .months-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 60px;
    box-sizing: border-box;
  }

  .months-empty-notice {
    padding: 60px 20px;
    text-align: center;
    color: var(--text-3);
    font-size: var(--text-size-body);
    font-style: italic;
  }

  .months-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
    gap: 20px;
  }

  .month-card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    text-align: left;
    transition: transform var(--dur-short) var(--ease-standard);
  }

  .month-card:hover {
    transform: scale(1.015);
  }

  /* Key-photo mosaic (1 large + 3 small) (§4.3) */
  .mosaic-container {
    width: 100%;
    aspect-ratio: 16 / 10;
    border-radius: var(--r-xl);
    overflow: hidden;
    display: grid;
    grid-template-columns: 2fr 1fr;
    gap: 2px;
    background: var(--fill-2);
    box-shadow: var(--shadow-1);
  }

  .mosaic-large {
    width: 100%;
    height: 100%;
    overflow: hidden;
  }

  .mosaic-large img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .mosaic-small-col {
    display: grid;
    grid-template-rows: repeat(3, 1fr);
    gap: 2px;
    height: 100%;
  }

  .mosaic-small {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--fill-3);
  }

  .mosaic-small img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .mosaic-placeholder,
  .mosaic-placeholder-small {
    width: 100%;
    height: 100%;
    background: var(--fill-2);
  }

  .month-card-footer {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 4px;
  }

  .month-name {
    font-size: 22px;
    line-height: 28px;
    font-weight: 700;
    color: var(--text-1);
    letter-spacing: -0.3px;
  }

  .month-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }
</style>
