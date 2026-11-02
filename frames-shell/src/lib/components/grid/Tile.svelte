<script module lang="ts">
  import { isTauriAvailable } from '$lib/services/tauriRepo';

  type ThumbReadyCallback = (assetId: string) => void;
  const thumbReadyListeners = new Set<ThumbReadyCallback>();
  let unlistenThumbReady: (() => void) | null = null;

  function onThumbReady(callback: ThumbReadyCallback): () => void {
    thumbReadyListeners.add(callback);
    if (thumbReadyListeners.size === 1 && isTauriAvailable()) {
      import('@tauri-apps/api/event').then(({ listen }) => {
        listen<{ asset_id: string }>('thumb-ready', (event) => {
          if (event.payload?.asset_id) {
            thumbReadyListeners.forEach((cb) => cb(event.payload.asset_id));
          }
        }).then((unlisten) => {
          unlistenThumbReady = unlisten;
        });
      });
    }
    return () => {
      thumbReadyListeners.delete(callback);
      if (thumbReadyListeners.size === 0 && unlistenThumbReady) {
        unlistenThumbReady();
        unlistenThumbReady = null;
      }
    };
  }
</script>

<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import type { Asset } from '$lib/types';
  import Badge from '$lib/components/surface/Badge.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { placeholderUrl, thumbUrl } from '$lib/media';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    asset,
    isSelected = false,
    isFill = true,
    size = 128,
    tabindex = -1,
    deferLoad = false,
    onclick,
    ondblclick,
    onpointerenter,
    onpointerleave
  }: {
    asset: Asset;
    isSelected?: boolean;
    isFill?: boolean;
    size?: number;
    tabindex?: number;
    /** The grid is flinging or scrubbing past: do not start loading yet. */
    deferLoad?: boolean;
    onclick?: (e: MouseEvent) => void;
    ondblclick?: (e: MouseEvent) => void;
    onpointerenter?: (e: PointerEvent) => void;
    onpointerleave?: (e: PointerEvent) => void;
  } = $props();

  let isLoaded = $state(false);
  let reloadCounter = $state(0);
  // Loading starts once the grid has slowed down, then carries on regardless.
  let started = $state(false);
  // Large tiles swap the 300 px thumbnail for the 1600 px preview once it is
  // decoded: something fast first, the sharp version when ready (PhotoKit's
  // opportunistic delivery, at the size the tile is actually shown).
  let upgraded = $state(false);

  $effect(() => {
    const _ = asset.id;
    isLoaded = false;
    reloadCounter = 0;
    upgraded = false;
    started = untrack(() => !deferLoad);
  });

  $effect(() => {
    if (!deferLoad) started = true;
  });

  const wantsLarge = $derived(
    typeof window !== 'undefined' && size * (window.devicePixelRatio || 1) > 360
  );

  $effect(() => {
    if (!wantsLarge || !isLoaded || upgraded || deferLoad) return;
    const id = asset.id;
    const large = new Image();
    large.src = thumbUrl(id, 1024);
    large
      .decode()
      .then(() => {
        if (asset.id === id) upgraded = true;
      })
      .catch(() => {});
  });

  // A missing thumbnail answers 404 and the tile keeps its placeholder until
  // the backend announces it with `thumb-ready`.
  const currentThumbUrl = $derived(
    upgraded
      ? thumbUrl(asset.id, 1024)
      : reloadCounter > 0
        ? `${thumbUrl(asset.id)}?v=${reloadCounter}`
        : thumbUrl(asset.id)
  );

  // The blurred ThumbHash sketch under the image, shown until it loads.
  const placeholder = $derived(isTauriAvailable() ? placeholderUrl(asset.thumbHash) : '');

  onMount(() => {
    return onThumbReady((assetId) => {
      if (assetId === asset.id) {
        reloadCounter++;
      }
    });
  });

  const formattedDuration = $derived.by(() => {
    if (!asset.durationMs) return '';
    const secs = Math.floor(asset.durationMs / 1000);
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, '0')}`;
  });

  const accessibleLabel = $derived(
    `${asset.kind === 'video' ? 'Video' : asset.kind === 'live' ? 'Live Photo' : 'Photo'}, ${new Date(asset.takenAt).toLocaleDateString()}${asset.favorite ? ', favourite' : ''}`
  );
  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      ondblclick?.(e as any);
    }
  }
</script>

<div
  role="gridcell"
  aria-selected={isSelected}
  aria-label={accessibleLabel}
  {tabindex}
  class="photo-tile {isSelected ? 'is-selected' : ''} {isFill ? 'is-fill' : 'is-fit'} appearance-{theme.appearance}"
  style="--tile-size: {size}px;"
  {onclick}
  {ondblclick}
  onkeydown={handleKeyDown}
  {onpointerenter}
  {onpointerleave}
>
  <!-- Background ThumbHash blur placeholder crossfading to real image (§4.4) -->
  <div class="thumb-container" style:background-image={placeholder ? `url(${placeholder})` : null}>
    {#if started}
      <img
        src={currentThumbUrl}
        alt=""
        loading="lazy"
        decoding="async"
        class="tile-image {isLoaded ? 'loaded' : ''}"
        onload={() => (isLoaded = true)}
      />
    {/if}
  </div>

  <!-- Tile States (§4.4) -->
  <!-- Check badge top-left on selection -->
  {#if isSelected}
    <div class="selection-check-badge">
      <Icon name="check" size={14} stroke={2.4} />
    </div>
  {/if}

  <!-- Live Photo marker top-left (the selection check takes its place) -->
  {#if asset.kind === 'live' && !isSelected}
    <div class="live-badge">
      <Badge variant="pill" text="LIVE" />
    </div>
  {/if}

  <!-- Favorite heart bottom-left -->
  {#if asset.favorite}
    <div class="favorite-badge">
      <Badge variant="favorite">
        <Icon name="heart-filled" size={14} class="heart-icon" />
      </Badge>
    </div>
  {/if}

  <!-- Video duration pill bottom-right -->
  {#if asset.kind === 'video' && formattedDuration}
    <div class="duration-badge">
      <Badge variant="video" text={formattedDuration} />
    </div>
  {/if}

  <!-- Cloud-only badge (not local) -->
  {#if !asset.originalLocal}
    <div class="cloud-badge">
      <Badge variant="cloud">
        <Icon name="cloud" size={12} />
      </Badge>
    </div>
  {/if}
</div>

<style>
  .photo-tile {
    position: relative;
    box-sizing: border-box;
    overflow: hidden;
    cursor: pointer;
    user-select: none;
    outline: none;
    background: var(--fill-1);
    transition: filter var(--dur-fast) var(--ease-standard),
                transform var(--dur-fast) var(--ease-standard);
  }

  .photo-tile.is-fill {
    width: var(--tile-size);
    height: var(--tile-size);
    aspect-ratio: 1 / 1;
  }

  .photo-tile.is-fit {
    height: var(--tile-size);
    width: auto;
  }

  /* Hover state: +4% brightness (§4.4) */
  .photo-tile:hover {
    filter: brightness(1.04);
  }

  /* Focus ring for roving tabindex (§12) */
  .photo-tile:focus-visible {
    box-shadow: inset 0 0 0 2px var(--focus-ring);
  }

  /* Selected state: inset 3px accent ring (§4.4) */
  .photo-tile.is-selected {
    box-shadow: inset 0 0 0 3px var(--accent);
  }

  .thumb-container {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    background-color: var(--fill-1);
    background-size: cover;
    background-position: center;
  }

  .tile-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
    opacity: 0;
    filter: blur(8px);
    transition: opacity 150ms var(--ease-standard),
                filter 150ms var(--ease-standard);
  }

  .tile-image.loaded {
    opacity: 1;
    filter: blur(0px);
  }

  /* Check badge top-left (22px, §4.4) */
  .selection-check-badge {
    position: absolute;
    top: 6px;
    left: 6px;
    width: 22px;
    height: 22px;
    border-radius: var(--r-pill);
    background: var(--accent);
    color: var(--accent-fg);
    display: flex;
    align-items: center;
    justify-content: center;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
    z-index: 2;
  }

  .favorite-badge {
    position: absolute;
    bottom: 6px;
    left: 6px;
    z-index: 2;
  }

  :global(.heart-icon) {
    color: var(--favorite);
  }

  .duration-badge {
    position: absolute;
    bottom: 6px;
    right: 6px;
    z-index: 2;
  }

  .live-badge {
    position: absolute;
    top: 6px;
    left: 6px;
    pointer-events: none;
    letter-spacing: 0.04em;
  }

  .cloud-badge {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 2;
  }
</style>
