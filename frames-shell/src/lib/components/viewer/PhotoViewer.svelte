<script lang="ts">
  import { onMount } from 'svelte';
  import type { Asset } from '$lib/types';
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { getThumbnailDataUrl } from '$lib/mock/mockRepo';
  import { isTauriAvailable, tauriRepo, getAssetUrl } from '$lib/services/tauriRepo';
  import { theme, focusAsset } from '$lib/theme/engine.svelte';

  let {
    asset = $bindable(),
    open = $bindable(false),
    onclose,
    onedit,
    oninfo
  }: {
    asset: Asset;
    open: boolean;
    onclose?: () => void;
    onedit?: (asset: Asset) => void;
    oninfo?: (asset: Asset) => void;
  } = $props();

  // Zoom & Pan state (§4.6: scroll with ⌘/Ctrl or pinch zooms to 800%, dblclick toggles 100% and fit)
  let zoomScale = $state(1.0);
  let panOffset = $state({ x: 0, y: 0 });
  let isPanning = $state(false);
  let panStart = $state({ x: 0, y: 0 });

  // Idle timer for filmstrip auto-hide in Adaptive and Glass (2.5s, §4.6)
  let isIdle = $state(false);
  let idleTimer: any = null;

  // Video controls state
  let isPlaying = $state(false);
  let videoProgress = $state(0); // 0..100
  let isMuted = $state(false);
  let playbackSpeed = $state(1);
  let isLooping = $state(false);

  // Loading chain progress (§4.6: ThumbHash -> 256 -> 1024 -> original)
  let loadingOriginalProgress = $state(100);

  // Rotation
  let rotationDeg = $state(0);

  const allAssets = $derived(tauriRepo.getAssets());
  const currentAssetIndex = $derived.by(() => {
    return allAssets.findIndex((a) => a.id === asset.id);
  });

  const neighbours = $derived.by(() => {
    if (allAssets.length === 0) return [asset];
    if (currentAssetIndex < 0) return [asset];
    const start = Math.max(0, currentAssetIndex - 10);
    const end = Math.min(allAssets.length, currentAssetIndex + 11);
    return allAssets.slice(start, end);
  });

  $effect(() => {
    if (asset) {
      // Focus asset in viewer with top priority (§7.6 #1)
      focusAsset(asset.id, { source: 'viewer', swatches: asset.palette || undefined, lum: asset.lum || undefined });
      // Reset zoom
      zoomScale = 1.0;
      panOffset = { x: 0, y: 0 };
      rotationDeg = 0;
      isPlaying = false;
      videoProgress = 0;
    }
  });

  function resetIdle() {
    isIdle = false;
    if (idleTimer) clearTimeout(idleTimer);
    if (theme.appearance !== 'standard') {
      idleTimer = setTimeout(() => {
        isIdle = true;
      }, 2500);
    }
  }

  function navigate(direction: -1 | 1) {
    if (currentAssetIndex < 0) return;
    const nextIdx = currentAssetIndex + direction;
    if (nextIdx >= 0 && nextIdx < allAssets.length) {
      asset = allAssets[nextIdx];
    }
  }

  function toggleZoom() {
    if (zoomScale > 1.0) {
      zoomScale = 1.0;
      panOffset = { x: 0, y: 0 };
    } else {
      zoomScale = 2.5; // 100% or close inspection
    }
  }

  function handleWheel(e: WheelEvent) {
    if (e.metaKey || e.ctrlKey) {
      e.preventDefault();
      const factor = e.deltaY < 0 ? 1.15 : 0.85;
      zoomScale = Math.min(8.0, Math.max(1.0, zoomScale * factor));
      if (zoomScale === 1.0) panOffset = { x: 0, y: 0 };
    }
  }

  function handlePointerDown(e: MouseEvent) {
    if (zoomScale > 1.0) {
      isPanning = true;
      panStart = { x: e.clientX - panOffset.x, y: e.clientY - panOffset.y };

      const onPointerMove = (ev: MouseEvent) => {
        if (isPanning) {
          panOffset = { x: ev.clientX - panStart.x, y: ev.clientY - panStart.y };
        }
      };

      const onPointerUp = () => {
        isPanning = false;
        window.removeEventListener('mousemove', onPointerMove);
        window.removeEventListener('mouseup', onPointerUp);
      };

      window.addEventListener('mousemove', onPointerMove);
      window.addEventListener('mouseup', onPointerUp);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'Escape') {
      onclose?.();
    } else if (e.key === 'ArrowRight') {
      navigate(1);
    } else if (e.key === 'ArrowLeft') {
      navigate(-1);
    } else if (e.key === ' ') {
      e.preventDefault();
      if (asset.kind === 'video') {
        isPlaying = !isPlaying;
      }
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'r') {
      e.preventDefault();
      rotationDeg = (rotationDeg + 90) % 360;
    } else if (e.key === '.') {
      asset.favorite = !asset.favorite;
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'i') {
      e.preventDefault();
      oninfo?.(asset);
    }
  }

  const formattedDate = $derived(
    new Date(asset.takenAt).toLocaleString('en-US', {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    })
  );

  let previewError = $state(false);
  let fallbackPreviewUrl = $state<string | null>(null);

  const previewUrl = $derived.by(() => {
    if (fallbackPreviewUrl) return fallbackPreviewUrl;
    if (previewError) return getThumbnailDataUrl(asset, 'preview');
    if (isTauriAvailable()) {
      return getAssetUrl('preview', asset.id);
    }
    return getThumbnailDataUrl(asset, 'preview');
  });

  async function handlePreviewError() {
    if (!fallbackPreviewUrl && isTauriAvailable()) {
      const url = await tauriRepo.getThumbnailDataUrl(asset.id, true);
      if (url) {
        fallbackPreviewUrl = url;
        return;
      }
    }
    previewError = true;
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
  <div
    role="presentation"
    class="photo-viewer-backdrop appearance-{theme.appearance}"
    onpointermove={resetIdle}
    onwheel={handleWheel}
  >
    <!-- Adaptive ambient photo glow behind the photo (scale 1.15, blur 60px, opacity .35, §7.5) -->
    <div
      class="viewer-ambient-glow"
      style="background-image: url({previewUrl});"
      aria-hidden="true"
    ></div>

    <!-- Viewer Top Bar (§4.6) -->
    <div class="viewer-top-bar {isIdle ? 'is-hidden' : ''}">
      <Surface
        kind={theme.appearance === 'glass' ? 'cluster' : 'floating'}
        clarity="clear"
        dimmed={(asset.lum ?? 0.5) > 0.65}
        class="viewer-top-surface"
      >
        <Button variant="icon" title="Back (Esc)" onclick={onclose}>
          <Icon name="chevron-left" size={17} />
        </Button>

        <div class="viewer-title-group">
          <span class="viewer-datetime">{formattedDate}</span>
          <span class="viewer-location-sub">{asset.location || 'Unknown location'}</span>
        </div>

        <div class="viewer-actions">
          <Button
            variant="icon"
            title="Favorite (.)"
            onclick={() => (asset.favorite = !asset.favorite)}
          >
            <Icon
              name={asset.favorite ? 'heart-filled' : 'heart'}
              size={15}
              class={asset.favorite ? 'text-favorite' : ''}
            />
          </Button>

          <Button
            variant="icon"
            title="Rotate (⌘R)"
            onclick={() => (rotationDeg = (rotationDeg + 90) % 360)}
          >
            <Icon name="rotate-cw" size={15} />
          </Button>

          <Button
            variant="icon"
            title="Info (⌘I)"
            onclick={() => oninfo?.(asset)}
          >
            <Icon name="info" size={15} />
          </Button>

          <Button
            variant="capsule"
            title="Edit mode"
            onclick={() => onedit?.(asset)}
          >
            <Icon name="edit" size={14} />
            <span>Edit</span>
          </Button>

          <Button variant="icon" title="Share">
            <Icon name="share" size={15} />
          </Button>
        </div>
      </Surface>
    </div>

    <!-- Left / Right Navigation chevrons -->
    <button
      type="button"
      class="nav-chevron nav-left {isIdle ? 'is-hidden' : ''}"
      onclick={() => navigate(-1)}
      aria-label="Previous photo"
    >
      <Icon name="chevron-left" size={26} stroke={2} />
    </button>

    <button
      type="button"
      class="nav-chevron nav-right {isIdle ? 'is-hidden' : ''}"
      onclick={() => navigate(1)}
      aria-label="Next photo"
    >
      <Icon name="chevron-right" size={26} stroke={2} />
    </button>

    <!-- Main Media Display Area with shared-element zoom and pan -->
    <div
      class="viewer-media-viewport"
      role="button"
      tabindex="0"
      aria-label="Toggle photo zoom"
      ondblclick={toggleZoom}
      onkeydown={(e) => { if (e.key === 'Enter') toggleZoom(); }}
      onmousedown={handlePointerDown}
    >
      <div
        class="media-transform-wrap"
        style="
          transform: translate({panOffset.x}px, {panOffset.y}px) scale({zoomScale}) rotate({rotationDeg}deg);
          cursor: {zoomScale > 1.0 ? (isPanning ? 'grabbing' : 'grab') : 'default'};
        "
      >
        <img
          src={previewUrl}
          alt={asset.title || 'Photo'}
          class="viewer-main-image"
          onerror={() => (previewError = true)}
        />
      </div>

      <!-- Thin progress ring bottom-right for original loading (§4.6) -->
      {#if loadingOriginalProgress < 100}
        <div class="original-progress-ring">
          <svg viewBox="0 0 20 20" width="18" height="18">
            <circle cx="10" cy="10" r="8" fill="none" stroke="rgba(255,255,255,0.3)" stroke-width="2" />
            <circle
              cx="10"
              cy="10"
              r="8"
              fill="none"
              stroke="#ffffff"
              stroke-width="2"
              stroke-dasharray="50.2"
              stroke-dashoffset={50.2 * (1 - loadingOriginalProgress / 100)}
            />
          </svg>
        </div>
      {/if}
    </div>

    <!-- Video Controls (§4.6) -->
    {#if asset.kind === 'video'}
      <div class="video-controls-bar {isIdle ? 'is-hidden' : ''}">
        <Surface kind="floating" class="video-controls-surface">
          <Button variant="icon" onclick={() => (isPlaying = !isPlaying)}>
            <Icon name={isPlaying ? 'pause' : 'play'} size={15} />
          </Button>

          <div class="video-scrubber-track">
            <input
              type="range"
              min="0"
              max="100"
              bind:value={videoProgress}
              class="video-scrubber-input"
            />
          </div>

          <Button variant="icon" onclick={() => (isMuted = !isMuted)}>
            <Icon name={isMuted ? 'volume-x' : 'volume-2'} size={15} />
          </Button>

          <Button
            variant="ghost"
            class="speed-btn"
            onclick={() => (playbackSpeed = playbackSpeed === 1 ? 1.5 : playbackSpeed === 1.5 ? 2 : 1)}
          >
            {playbackSpeed}x
          </Button>

          <Button
            variant="icon"
            active={isLooping}
            onclick={() => (isLooping = !isLooping)}
          >
            <Icon name="repeat" size={14} />
          </Button>
        </Surface>
      </div>
    {/if}

    <!-- 56px Tall Filmstrip with current item centered (§4.6) -->
    <div class="viewer-filmstrip {isIdle && theme.appearance !== 'standard' ? 'is-hidden' : ''}">
      <Surface kind="floating" class="filmstrip-surface">
        <div class="filmstrip-scroll">
          {#each neighbours as neighbour}
            {@const isCurrent = neighbour.id === asset.id}
            <button
              type="button"
              class="filmstrip-thumb {isCurrent ? 'is-current' : ''}"
              onclick={() => (asset = neighbour)}
            >
              <img
                src={isTauriAvailable() ? getAssetUrl('thumb', neighbour.id) : getThumbnailDataUrl(neighbour, 'thumb')}
                alt=""
                loading="lazy"
              />
            </button>
          {/each}
        </div>
      </Surface>
    </div>
  </div>
{/if}

<style>
  .photo-viewer-backdrop {
    position: fixed;
    inset: 0;
    z-index: 500;
    background: var(--bg-viewer);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    user-select: none;
    animation: viewerOpen 350ms var(--ease-standard);
  }

  @keyframes viewerOpen {
    from {
      opacity: 0;
      transform: scale(0.97);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  /* Adaptive photo glow (§7.5) */
  .viewer-ambient-glow {
    position: absolute;
    inset: 0;
    background-size: cover;
    background-position: center;
    transform: scale(1.15);
    filter: blur(60px);
    opacity: 0.35;
    pointer-events: none;
    z-index: 1;
  }

  .viewer-top-bar {
    position: absolute;
    top: 14px;
    left: 14px;
    right: 14px;
    z-index: 20;
    display: flex;
    justify-content: center;
    transition: opacity var(--dur-base) var(--ease-standard),
                transform var(--dur-base) var(--ease-standard);
  }

  .viewer-top-bar.is-hidden {
    opacity: 0;
    transform: translateY(-10px);
    pointer-events: none;
  }

  :global(.viewer-top-surface) {
    display: inline-flex;
    align-items: center;
    height: 44px;
    padding: 0 12px;
    gap: 14px;
    border-radius: var(--r-pill);
    box-shadow: var(--shadow-2);
  }

  .viewer-title-group {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 0 10px;
  }

  .viewer-datetime {
    font-size: var(--text-size-body);
    font-weight: 600;
    color: var(--text-1);
  }

  .viewer-location-sub {
    font-size: 11px;
    color: var(--text-2);
  }

  .viewer-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  /* Navigation chevrons */
  .nav-chevron {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    width: 44px;
    height: 44px;
    border-radius: var(--r-pill);
    border: none;
    background: rgba(0, 0, 0, 0.35);
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    z-index: 20;
    transition: opacity var(--dur-base) var(--ease-standard),
                background var(--dur-fast) var(--ease-standard);
  }

  .nav-chevron:hover {
    background: rgba(0, 0, 0, 0.6);
  }

  .nav-chevron.is-hidden {
    opacity: 0;
    pointer-events: none;
  }

  .nav-left {
    left: 16px;
  }
  .nav-right {
    right: 16px;
  }

  /* Main Viewport */
  .viewer-media-viewport {
    position: relative;
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    z-index: 10;
  }

  .media-transform-wrap {
    display: flex;
    align-items: center;
    justify-content: center;
    max-width: 90vw;
    max-height: 82vh;
    transition: transform 60ms linear;
  }

  .viewer-main-image {
    max-width: 100%;
    max-height: 82vh;
    object-fit: contain;
    border-radius: 4px;
    box-shadow: 0 4px 30px rgba(0, 0, 0, 0.35);
  }

  .original-progress-ring {
    position: absolute;
    bottom: 20px;
    right: 20px;
    z-index: 30;
  }

  /* Video controls */
  .video-controls-bar {
    position: absolute;
    bottom: 84px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 20;
    transition: opacity var(--dur-base) var(--ease-standard);
  }
  .video-controls-bar.is-hidden {
    opacity: 0;
    pointer-events: none;
  }

  :global(.video-controls-surface) {
    display: inline-flex;
    align-items: center;
    height: 38px;
    padding: 0 12px;
    gap: 10px;
    border-radius: var(--r-pill);
    box-shadow: var(--shadow-2);
  }

  .video-scrubber-track {
    width: 220px;
    display: flex;
    align-items: center;
  }
  .video-scrubber-input {
    width: 100%;
    height: 4px;
    accent-color: var(--accent);
  }

  .speed-btn {
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 0 6px;
  }

  /* 56px Tall Filmstrip (§4.6) */
  .viewer-filmstrip {
    position: absolute;
    bottom: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 20;
    transition: opacity var(--dur-base) var(--ease-standard),
                transform var(--dur-base) var(--ease-standard);
  }

  .viewer-filmstrip.is-hidden {
    opacity: 0;
    transform: translate(-50%, 14px);
    pointer-events: none;
  }

  :global(.filmstrip-surface) {
    display: flex;
    align-items: center;
    height: 56px;
    padding: 4px 8px;
    border-radius: var(--r-md);
    box-shadow: var(--shadow-2);
    overflow: hidden;
  }

  .filmstrip-scroll {
    display: flex;
    align-items: center;
    gap: 4px;
    overflow-x: auto;
    max-width: 600px;
  }

  .filmstrip-thumb {
    width: 44px;
    height: 44px;
    border-radius: 4px;
    overflow: hidden;
    padding: 0;
    border: none;
    background: var(--fill-2);
    cursor: pointer;
    flex-shrink: 0;
    opacity: 0.6;
    transition: opacity var(--dur-fast) var(--ease-standard),
                transform var(--dur-fast) var(--ease-standard);
  }

  .filmstrip-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .filmstrip-thumb:hover {
    opacity: 0.9;
  }

  .filmstrip-thumb.is-current {
    opacity: 1;
    transform: scale(1.08);
    box-shadow: 0 0 0 2px var(--accent);
  }

  :global(.text-favorite) {
    color: var(--favorite) !important;
  }
</style>
