<script lang="ts">
  import { onMount } from 'svelte';
  import VirtualGrid from '$lib/components/grid/VirtualGrid.svelte';
  import type { AlbumItem, Asset, Query } from '$lib/types';
  import { getAssetByIndex, getThumbnailDataUrl } from '$lib/mock/mockRepo';
  import { focusAsset } from '$lib/theme/engine.svelte';

  let {
    album,
    zoomLevelIndex = $bindable(2),
    isFill = $bindable(true),
    selectedIds = $bindable<Set<string>>(new Set()),
    onopenphoto,
    onselectionchange
  }: {
    album: AlbumItem;
    zoomLevelIndex?: number;
    isFill?: boolean;
    selectedIds?: Set<string>;
    onopenphoto?: (asset: Asset) => void;
    onselectionchange?: (ids: Set<string>) => void;
  } = $props();

  let scrollContainer: HTMLDivElement | null = $state(null);
  let scrollY = $state(0);

  const keyAssetIdx = $derived(parseInt(album.keyAssetId.replace('asset_', ''), 10) || 0);
  const keyAsset = $derived(getAssetByIndex(keyAssetIdx));
  const heroThumb = $derived(getThumbnailDataUrl(keyAsset, 'preview'));

  $effect(() => {
    if (keyAsset) {
      // Key photo drives the palette on collection/album detail page (§7.6 #2)
      focusAsset(keyAsset.id, {
        source: 'collection',
        swatches: keyAsset.palette || undefined,
        lum: keyAsset.lum || undefined
      });
    }
  });

  const query: Query = $derived({
    scope: 'album',
    albumId: album.id
  });

  function handleScroll(e: Event) {
    const target = e.target as HTMLDivElement;
    scrollY = target.scrollTop;
  }
</script>

<div class="album-detail-container" onscroll={handleScroll}>
  <!-- Ambient Hero with parallax 0.4 (§4.9) -->
  <div class="album-hero-wrap">
    <div
      class="hero-ambient-bg"
      style="
        background-image: url({heroThumb});
        transform: translateY({scrollY * 0.4}px);
      "
      aria-hidden="true"
    ></div>

    <div class="hero-scrim">
      <!-- Title in 34/40, date range, count (§4.9) -->
      <h1 class="hero-title">{album.title}</h1>
      <div class="hero-metadata">
        <span>{album.dateRange}</span>
        <span class="meta-dot">·</span>
        <span>{album.assetCount.toLocaleString()} photos</span>
      </div>
    </div>
  </div>

  <!-- Album Assets Grid -->
  <div class="album-grid-wrap">
    <VirtualGrid
      {query}
      bind:zoomLevelIndex
      bind:isFill
      bind:selectedIds
      {onopenphoto}
      {onselectionchange}
    />
  </div>
</div>

<style>
  .album-detail-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    position: relative;
    box-sizing: border-box;
  }

  .album-hero-wrap {
    position: relative;
    height: 320px;
    width: 100%;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
  }

  /* Parallax 0.4 ambient hero (§4.9) */
  .hero-ambient-bg {
    position: absolute;
    top: -40px;
    left: 0;
    right: 0;
    height: 400px;
    background-size: cover;
    background-position: center;
    filter: brightness(0.75);
    will-change: transform;
    pointer-events: none;
  }

  .hero-scrim {
    position: relative;
    z-index: 2;
    padding: 24px 32px;
    background: linear-gradient(to top, var(--bg-canvas) 0%, transparent 100%);
    color: var(--text-1);
  }

  /* 34/40 bold title (§4.9) */
  .hero-title {
    font-size: var(--text-size-hero-title);
    line-height: var(--text-lh-hero-title);
    font-weight: 700;
    margin: 0;
    letter-spacing: -0.5px;
  }

  .hero-metadata {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-size-body);
    color: var(--text-2);
    margin-top: 4px;
    font-weight: 500;
  }

  .meta-dot {
    opacity: 0.5;
  }

  .album-grid-wrap {
    min-height: 800px;
  }
</style>
