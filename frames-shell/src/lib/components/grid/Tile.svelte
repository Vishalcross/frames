<script lang="ts">
  import type { Asset } from '$lib/types';
  import Badge from '$lib/components/surface/Badge.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { getThumbnailDataUrl } from '$lib/mock/mockRepo';
  import { isTauriAvailable, tauriRepo, getAssetUrl } from '$lib/services/tauriRepo';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    asset,
    isSelected = false,
    isFill = true,
    size = 128,
    tabindex = -1,
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
    onclick?: (e: MouseEvent) => void;
    ondblclick?: (e: MouseEvent) => void;
    onpointerenter?: (e: PointerEvent) => void;
    onpointerleave?: (e: PointerEvent) => void;
  } = $props();

  let isLoaded = $state(false);
  let imgError = $state(false);
  let fallbackDataUrl = $state<string | null>(null);

  const thumbUrl = $derived.by(() => {
    if (fallbackDataUrl) return fallbackDataUrl;
    if (isTauriAvailable() && !imgError) {
      return getAssetUrl('thumb', asset.id);
    }
    return getThumbnailDataUrl(asset, 'thumb');
  });

  async function handleImgError() {
    if (!fallbackDataUrl && isTauriAvailable()) {
      const dataUrl = await tauriRepo.getThumbnailDataUrl(asset.id, false);
      if (dataUrl) {
        fallbackDataUrl = dataUrl;
        isLoaded = true;
        return;
      }
    }
    imgError = true;
    isLoaded = true;
  }

  const formattedDuration = $derived.by(() => {
    if (!asset.durationMs) return '';
    const secs = Math.floor(asset.durationMs / 1000);
    const m = Math.floor(secs / 60);
    const s = secs % 60;
    return `${m}:${s.toString().padStart(2, '0')}`;
  });

  const accessibleLabel = $derived(
    `${asset.kind === 'video' ? 'Video' : 'Photo'}, ${new Date(asset.takenAt).toLocaleDateString()}${asset.favorite ? ', favourite' : ''}`
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
  <div class="thumb-container">
    <img
      src={thumbUrl}
      alt=""
      loading="lazy"
      class="tile-image {isLoaded ? 'loaded' : ''}"
      onload={() => (isLoaded = true)}
      onerror={handleImgError}
    />
  </div>

  <!-- Tile States (§4.4) -->
  <!-- Check badge top-left on selection -->
  {#if isSelected}
    <div class="selection-check-badge">
      <Icon name="check" size={14} stroke={2.4} />
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

  .cloud-badge {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 2;
  }
</style>
