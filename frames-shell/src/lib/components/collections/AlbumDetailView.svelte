<script lang="ts">
  import VirtualGrid from '$lib/components/grid/VirtualGrid.svelte';
  import type { AlbumItem, Asset, Query, AssetRepo } from '$lib/types';
  import { tauriRepo, getAssetUrl, isTauriAvailable } from '$lib/services/tauriRepo';
  import { focusAsset } from '$lib/theme/engine.svelte';

  let {
    album,
    repo = tauriRepo,
    zoomLevelIndex = $bindable(2),
    isFill = $bindable(true),
    selectedIds = $bindable<Set<string>>(new Set()),
    onopenphoto,
    onselectionchange
  }: {
    album: AlbumItem;
    repo?: AssetRepo;
    zoomLevelIndex?: number;
    isFill?: boolean;
    selectedIds?: Set<string>;
    onopenphoto?: (asset: Asset) => void;
    onselectionchange?: (ids: Set<string>) => void;
  } = $props();

  import { onMount } from 'svelte';

  let heroFallback = $state<string | null>(null);
  let countOverride = $state<number | null>(null);
  const assetCount = $derived(countOverride ?? album.assetCount);

  function logDebugIfEnabled(albumId: string) {
    if (typeof window !== 'undefined' && albumId && isTauriAvailable()) {
      const urlParams = new URLSearchParams(window.location.search);
      if (urlParams.get('debug') === '1' || window.location.hash.includes('debug=1')) {
        import('@tauri-apps/api/core').then(({ invoke }) => {
          invoke('debug_album', { albumId })
            .then((dbg) => {
              console.log(`[debug_album ${albumId}]`, dbg);
            })
            .catch((err) => {
              console.warn(`[debug_album ${albumId} error]`, err);
            });
        });
      }
    }
  }

  $effect(() => {
    // Reset override when album id changes
    const _id = album.id;
    countOverride = null;
    if (_id && isTauriAvailable()) {
      tauriRepo.getAlbumAssetCount(_id).then((cnt) => {
        countOverride = cnt;
      });
      logDebugIfEnabled(_id);
    }
  });

  onMount(() => {
    if (!isTauriAvailable()) return;
    let unlistens: Array<() => void> = [];

    import('@tauri-apps/api/event').then(({ listen }) => {
      listen<{ album_id?: string }>('album-synced', (ev) => {
        if (!ev.payload?.album_id || ev.payload.album_id === album.id) {
          tauriRepo.getAlbumAssetCount(album.id).then((cnt) => {
            countOverride = cnt;
          });
          logDebugIfEnabled(album.id);
        }
      }).then((un) => unlistens.push(un));

      listen('scan-finished', () => {
        tauriRepo.getAlbumAssetCount(album.id).then((cnt) => {
          countOverride = cnt;
        });
        logDebugIfEnabled(album.id);
      }).then((un) => unlistens.push(un));
    });

    return () => {
      for (const un of unlistens) {
        un();
      }
    };
  });

  const heroThumb = $derived(
    heroFallback || (album.keyAssetId ? getAssetUrl('preview', album.keyAssetId) : '')
  );

  $effect(() => {
    if (album.keyAssetId && isTauriAvailable()) {
      tauriRepo.getThumbnailDataUrl(album.keyAssetId, true).then((url) => {
        if (url) heroFallback = url;
      });
    }
  });

  $effect(() => {
    if (album.keyAssetId) {
      focusAsset(album.keyAssetId, {
        source: 'collection'
      });
    }
  });

  const query: Query = $derived({
    scope: 'album',
    albumId: album.id
  });
</script>

<div class="album-detail-container">
  <!-- Ambient Hero with parallax (§4.9) -->
  <div class="album-hero-wrap">
    {#if heroThumb}
      <div
        class="hero-ambient-bg"
        style="background-image: url({heroThumb});"
        aria-hidden="true"
      ></div>
    {/if}

    <div class="hero-scrim">
      <!-- Title in 34/40, date range, count (§4.9) -->
      <h1 class="hero-title">{album.title}</h1>
      <div class="hero-metadata">
        {#if album.dateRange}
          <span>{album.dateRange}</span>
          <span class="meta-dot">·</span>
        {/if}
        <span>{assetCount.toLocaleString()} {assetCount === 1 ? 'item' : 'items'}</span>
      </div>
    </div>
  </div>

  <!-- Album Assets Grid -->
  <div class="album-grid-wrap">
    <VirtualGrid
      {repo}
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
    display: flex;
    flex-direction: column;
    overflow: hidden;
    position: relative;
    box-sizing: border-box;
  }

  .album-hero-wrap {
    position: relative;
    width: 100%;
    height: 180px;
    flex-shrink: 0;
    overflow: hidden;
    display: flex;
    align-items: flex-end;
    background: var(--fill-2);
  }

  .hero-ambient-bg {
    position: absolute;
    top: -40px;
    left: 0;
    right: 0;
    height: 320px;
    background-size: cover;
    background-position: center;
    filter: brightness(0.65) blur(2px);
    pointer-events: none;
  }

  .hero-scrim {
    position: relative;
    z-index: 2;
    padding: 20px 32px;
    background: linear-gradient(to top, var(--bg-canvas) 0%, rgba(0, 0, 0, 0.4) 60%, transparent 100%);
    color: var(--text-1);
    width: 100%;
    box-sizing: border-box;
  }

  .hero-title {
    font-size: 34px;
    line-height: 40px;
    font-weight: 700;
    margin: 0 0 4px;
    color: #ffffff;
    letter-spacing: -0.5px;
    text-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }

  .hero-metadata {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-size-caption);
    color: rgba(255, 255, 255, 0.8);
  }

  .meta-dot {
    opacity: 0.6;
  }

  .album-grid-wrap {
    position: relative;
    width: 100%;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
</style>
