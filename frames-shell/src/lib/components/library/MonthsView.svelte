<script lang="ts">
  import { getThumbnailDataUrl, getAssetByIndex } from '$lib/mock/mockRepo';

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

  const locations = [
    'Big Sur & Monterey', 'Kyoto, Japan', 'Dolomites, Italy',
    'Amalfi Coast', 'Reykjavík, Iceland', 'Swiss Alps',
    'Yosemite Valley', 'Santorini, Greece', 'Banff National Park',
    'Paris, France', 'Zion National Park', 'Lake Tahoe'
  ];

  const monthCards = $derived(
    monthNames.map((name, idx) => {
      const monthNum = idx + 1;
      const baseIdx = (year * 31 + idx * 79) % 80000;
      const keyAsset = getAssetByIndex(baseIdx);
      const sub1 = getAssetByIndex(baseIdx + 1);
      const sub2 = getAssetByIndex(baseIdx + 2);
      const sub3 = getAssetByIndex(baseIdx + 3);

      return {
        month: monthNum,
        name,
        location: locations[idx % locations.length],
        largeThumb: getThumbnailDataUrl(keyAsset, 'preview'),
        smallThumbs: [
          getThumbnailDataUrl(sub1, 'thumb'),
          getThumbnailDataUrl(sub2, 'thumb'),
          getThumbnailDataUrl(sub3, 'thumb')
        ]
      };
    })
  );
</script>

<div class="months-view-container">
  <div class="months-grid">
    {#each monthCards as card}
      <button
        type="button"
        class="month-card"
        onclick={() => ondrillmonth?.(card.month)}
        aria-label="{card.name} {year}, {card.location}"
      >
        <!-- Key-photo mosaic (1 large + 3 small) (§4.3) -->
        <div class="mosaic-container">
          <div class="mosaic-large">
            <img src={card.largeThumb} alt="" loading="lazy" />
          </div>
          <div class="mosaic-small-col">
            {#each card.smallThumbs as st}
              <div class="mosaic-small">
                <img src={st} alt="" loading="lazy" />
              </div>
            {/each}
          </div>
        </div>

        <!-- Month info footer -->
        <div class="month-info">
          <!-- Month name 22/28 bold (§4.3) -->
          <div class="month-name">{card.name}</div>
          <!-- Location subtitle (§4.3) -->
          <div class="month-location">{card.location}</div>
        </div>
      </button>
    {/each}
  </div>
</div>

<style>
  .months-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 32px;
    box-sizing: border-box;
  }

  .months-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 20px;
    max-width: 1400px;
    margin: 0 auto;
  }

  .month-card {
    display: flex;
    flex-direction: column;
    border: none;
    padding: 0;
    background: transparent;
    cursor: pointer;
    text-align: left;
    outline: none;
    transition: transform var(--dur-fast) var(--ease-standard);
  }

  .month-card:hover {
    transform: translateY(-2px);
  }

  /* 1 large + 3 small mosaic layout */
  .mosaic-container {
    display: flex;
    gap: 3px;
    aspect-ratio: 16 / 10;
    border-radius: 14px;
    overflow: hidden;
    background: var(--fill-2);
    box-shadow: var(--shadow-1);
  }

  .mosaic-large {
    flex: 2;
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
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 3px;
    height: 100%;
  }

  .mosaic-small {
    flex: 1;
    overflow: hidden;
  }
  .mosaic-small img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .month-info {
    padding: 10px 4px 4px;
    user-select: none;
  }

  .month-name {
    font-size: 22px;
    line-height: 28px;
    font-weight: 700;
    color: var(--text-1);
    letter-spacing: -0.3px;
  }

  .month-location {
    font-size: var(--text-size-caption);
    line-height: var(--text-lh-caption);
    color: var(--text-2);
    font-weight: 500;
    margin-top: 1px;
  }
</style>
