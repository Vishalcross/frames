<script lang="ts">
  import { getThumbnailDataUrl, getAssetByIndex } from '$lib/mock/mockRepo';
  import type { Asset } from '$lib/types';

  let {
    ondrillyear
  }: {
    ondrillyear?: (year: number) => void;
  } = $props();

  // Years from 2018 to 2026
  const years = [2026, 2025, 2024, 2023, 2022, 2021, 2020, 2019, 2018];

  const yearData = $derived(
    years.map((year, i) => {
      // Pick representative key asset for the year
      const sampleAsset = getAssetByIndex(Math.floor((i / years.length) * 90000));
      return {
        year,
        count: Math.round(8000 + ((year * 17) % 6000)),
        keyAsset: sampleAsset,
        thumbUrl: getThumbnailDataUrl(sampleAsset, 'preview')
      };
    })
  );
</script>

<div class="years-view-container">
  <div class="years-grid">
    {#each yearData as item}
      <button
        type="button"
        class="year-card"
        onclick={() => ondrillyear?.(item.year)}
        aria-label="Year {item.year}, {item.count} items"
      >
        <img src={item.thumbUrl} alt="" class="year-bg-img" loading="lazy" />

        <!-- Bottom-left scrim with Year and photo count (§4.3) -->
        <div class="year-scrim">
          <div class="year-title">{item.year}</div>
          <div class="year-count">{item.count.toLocaleString()} photos</div>
        </div>
      </button>
    {/each}
  </div>
</div>

<style>
  .years-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 32px;
    box-sizing: border-box;
  }

  .years-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
    gap: 12px;
    max-width: 1400px;
    margin: 0 auto;
  }

  /* Cards 16:10, radius 14, gap 12 (§4.3) */
  .year-card {
    position: relative;
    aspect-ratio: 16 / 10;
    border-radius: 14px;
    overflow: hidden;
    border: none;
    padding: 0;
    background: var(--fill-2);
    cursor: pointer;
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-fast) var(--ease-standard),
                box-shadow var(--dur-fast) var(--ease-standard);
  }

  .year-card:hover {
    transform: scale(1.015);
    box-shadow: var(--shadow-2);
  }

  .year-bg-img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .year-scrim {
    position: absolute;
    inset: auto 0 0 0;
    height: 55%;
    background: linear-gradient(to top, rgba(0, 0, 0, 0.78) 0%, transparent 100%);
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    padding: 16px 20px;
    text-align: left;
    color: #ffffff;
    user-select: none;
  }

  /* Year in 34/40 bold white (§4.3) */
  .year-title {
    font-size: var(--text-size-hero-title);
    line-height: var(--text-lh-hero-title);
    font-weight: 700;
    letter-spacing: -0.5px;
  }

  .year-count {
    font-size: var(--text-size-caption);
    line-height: var(--text-lh-caption);
    font-weight: 500;
    opacity: 0.85;
    margin-top: 2px;
  }
</style>
