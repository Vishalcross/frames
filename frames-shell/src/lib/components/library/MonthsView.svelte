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

  // Three more thumbnails per month for the mosaic, fetched per month
  // (positions 1-3; position 0 is the month's cover).
  let mosaicIds = $state<Record<number, string[]>>({});

  $effect(() => {
    const y = year;
    mosaicIds = {};
    for (const mg of tauriRepo.getMonthGroups().filter((g) => g.year === y)) {
      tauriRepo.range({ year: y, month: mg.month }, 1, 3).then((assets) => {
        if (year === y) mosaicIds = { ...mosaicIds, [mg.month]: assets.map((a) => a.id) };
      });
    }
  });

  const monthCards = $derived(
    tauriRepo
      .getMonthGroups()
      .filter((mg) => mg.year === year)
      .sort((a, b) => b.month - a.month)
      .map((mg) => ({
        month: mg.month,
        name: monthNames[mg.month - 1] || `Month ${mg.month}`,
        count: mg.count,
        largeThumb: mg.key_asset_id ? getAssetUrl('preview', mg.key_asset_id) : '',
        smallThumbs: (mosaicIds[mg.month] ?? []).map((id) => getAssetUrl('thumb', id))
      }))
  );
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
