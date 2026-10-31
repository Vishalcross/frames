<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Tile from './Tile.svelte';
  import type { Asset, AssetRepo, Query } from '$lib/types';
  import { tauriRepo } from '$lib/services/tauriRepo';
  import { focusAsset } from '$lib/theme/engine.svelte';

  const ZOOM_TARGETS = [64, 96, 128, 176, 240, 320];

  let {
    query = {},
    repo = tauriRepo,
    zoomLevelIndex = $bindable(2),
    isFill = $bindable(true),
    selectedIds = $bindable<Set<string>>(new Set()),
    onopenphoto,
    onselectionchange
  }: {
    query?: Query;
    repo?: AssetRepo;
    zoomLevelIndex?: number;
    isFill?: boolean;
    selectedIds?: Set<string>;
    onopenphoto?: (asset: Asset) => void;
    onselectionchange?: (ids: Set<string>) => void;
  } = $props();

  let containerRef: HTMLDivElement | null = $state(null);
  let containerWidth = $state(1000);
  let containerHeight = $state(800);
  let scrollTop = $state(0);

  let totalAssets = $state(0);
  let assetsMap = $state<Map<number, Asset>>(new Map());

  const PAGE_SIZE = 200;
  let loadedPages = new Set<number>();
  let inFlightPages = new Set<number>();
  let activeQueryKey = '';

  // Date pill state (§4.3)
  let datePillText = $state('October 2026');
  let datePillVisible = $state(false);
  let datePillTimer: any = null;

  // Scrubber state (§4.3)
  let isHoveringScrubber = $state(false);
  let scrubberBubbleText = $state('');
  let scrubberBubbleY = $state(0);
  let isDraggingScrubber = $state(false);

  // Marquee selection state (§4.5)
  let isMarqueeActive = $state(false);
  let marqueeStart = $state({ x: 0, y: 0 });
  let marqueeCurrent = $state({ x: 0, y: 0 });

  // Compute layout dimensions
  const targetTileSize = $derived(ZOOM_TARGETS[zoomLevelIndex] || 128);
  const gap = $derived(targetTileSize <= 128 ? 2 : 4);
  const cols = $derived(Math.max(1, Math.floor((containerWidth + gap) / (targetTileSize + gap))));
  const actualTileSize = $derived(Math.floor((containerWidth - (cols - 1) * gap) / cols));
  const rowHeight = $derived(actualTileSize + gap);
  const totalRows = $derived(Math.ceil(totalAssets / cols));
  const totalGridHeight = $derived(totalRows * rowHeight);

  // Visible window range calculation with 2 viewports overscan (§11)
  const overscanRows = 8;
  const startRow = $derived(
    totalRows <= 0 ? 0 : Math.max(0, Math.min(Math.max(0, totalRows - 1), Math.floor(scrollTop / rowHeight) - overscanRows))
  );
  const visibleRowCount = $derived(Math.ceil(containerHeight / rowHeight) + overscanRows * 2);
  const endRow = $derived(
    totalRows <= 0 ? 0 : Math.max(startRow, Math.min(totalRows, startRow + visibleRowCount))
  );

  const startIndex = $derived(startRow * cols);
  const endIndex = $derived(Math.min(totalAssets, endRow * cols));

  // Query count & cache invalidation
  $effect(() => {
    const qKey = JSON.stringify(query);
    if (qKey !== activeQueryKey) {
      activeQueryKey = qKey;
      const currentKey = qKey;
      loadedPages.clear();
      inFlightPages.clear();
      assetsMap = new Map();
      totalAssets = 0;
      scrollTop = 0;
      if (containerRef) containerRef.scrollTop = 0;
      repo.count(query).then((cnt) => {
        if (activeQueryKey === currentKey) {
          totalAssets = cnt;
        }
      });
    }
  });

  // Load pages of 200 assets covering visible rows
  $effect(() => {
    if (totalAssets <= 0) return;
    const s = startIndex;
    const e = Math.min(totalAssets, endIndex);
    if (e <= s) return;

    const startPage = Math.floor(s / PAGE_SIZE);
    const endPage = Math.floor(Math.max(0, e - 1) / PAGE_SIZE);
    const reqKey = activeQueryKey;

    for (let p = startPage; p <= endPage; p++) {
      if (p * PAGE_SIZE >= totalAssets) continue;
      if (!loadedPages.has(p) && !inFlightPages.has(p)) {
        inFlightPages.add(p);
        const offset = p * PAGE_SIZE;
        const pageIdx = p;
        repo.range(query, offset, PAGE_SIZE).then((assets) => {
          if (activeQueryKey !== reqKey) return;
          loadedPages.add(pageIdx);
          inFlightPages.delete(pageIdx);
          const newMap = new Map(assetsMap);
          assets.forEach((a, i) => {
            newMap.set(offset + i, a);
          });
          assetsMap = newMap;
        }).catch(() => {
          if (activeQueryKey === reqKey) {
            inFlightPages.delete(pageIdx);
          }
        });
      }
    }
  });

  onMount(async () => {
    if (!containerRef) return;
    const rect = containerRef.getBoundingClientRect();
    containerWidth = rect.width || 1000;
    containerHeight = rect.height || 800;

    await tick();
    // Opens scrolled to the newest photo at the bottom (§4.3) unless viewing an album
    if (totalGridHeight > containerHeight && query.scope !== 'album') {
      containerRef.scrollTop = totalGridHeight - containerHeight;
    }
  });

  // Handle scroll events: Sticky date pill & center tile focus (§4.3, §7.6)
  function handleScroll(e: Event) {
    const target = e.target as HTMLDivElement;
    scrollTop = target.scrollTop;

    // Show date pill and reset fade timer (1.2s fade, §4.3)
    datePillVisible = true;
    if (datePillTimer) clearTimeout(datePillTimer);
    datePillTimer = setTimeout(() => {
      datePillVisible = false;
    }, 1200);

    // Compute month & year of topmost visible row
    const topRowIndex = Math.floor(scrollTop / rowHeight);
    const topAssetIndex = Math.min(totalAssets - 1, topRowIndex * cols);
    const topAsset = assetsMap.get(topAssetIndex);
    if (topAsset) {
      const d = new Date(topAsset.takenAt);
      datePillText = d.toLocaleString('en-US', { month: 'long', year: 'numeric' });

      // Adaptive scroll center focus (§7.6 #4)
      const centerAssetIndex = Math.min(totalAssets - 1, topAssetIndex + Math.floor((cols * visibleRowCount) / 2));
      const centerAsset = assetsMap.get(centerAssetIndex);
      if (centerAsset) {
        focusAsset(centerAsset.id, { source: 'scroll' });
      }
    }
  }

  // Right-edge scrubber interaction (§4.3)
  function handleScrubberPointerDown(e: PointerEvent) {
    e.preventDefault();
    isDraggingScrubber = true;
    updateScrubberPosition(e);

    const onPointerMove = (ev: PointerEvent) => {
      if (isDraggingScrubber) updateScrubberPosition(ev);
    };
    const onPointerUp = () => {
      isDraggingScrubber = false;
      window.removeEventListener('pointermove', onPointerMove);
      window.removeEventListener('pointerup', onPointerUp);
    };

    window.addEventListener('pointermove', onPointerMove);
    window.addEventListener('pointerup', onPointerUp);
  }

  function updateScrubberPosition(e: MouseEvent) {
    if (!containerRef) return;
    const rect = containerRef.getBoundingClientRect();
    const progress = Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
    containerRef.scrollTop = progress * (totalGridHeight - rect.height);

    scrubberBubbleY = e.clientY - rect.top;
    const approxIndex = Math.floor(progress * (totalAssets - 1));
    const approxAsset = assetsMap.get(approxIndex);
    if (approxAsset) {
      const d = new Date(approxAsset.takenAt);
      scrubberBubbleText = d.toLocaleString('en-US', { month: 'short', year: 'numeric' });
    }
  }

  // Selection logic (§4.5)
  let lastSelectedIndex = -1;

  function handleTileClick(index: number, asset: Asset, e: MouseEvent) {
    const newSelection = new Set(selectedIds);

    if (e.shiftKey && lastSelectedIndex >= 0) {
      // Range selection
      const start = Math.min(lastSelectedIndex, index);
      const end = Math.max(lastSelectedIndex, index);
      for (let i = start; i <= end; i++) {
        const a = assetsMap.get(i);
        if (a) newSelection.add(a.id);
      }
    } else if (e.metaKey || e.ctrlKey) {
      // Toggle selection
      if (newSelection.has(asset.id)) {
        newSelection.delete(asset.id);
      } else {
        newSelection.add(asset.id);
      }
      lastSelectedIndex = index;
    } else {
      // Single selection
      newSelection.clear();
      newSelection.add(asset.id);
      lastSelectedIndex = index;
    }

    selectedIds = newSelection;
    onselectionchange?.(newSelection);
  }

  function handleTileDoubleClick(asset: Asset) {
    onopenphoto?.(asset);
  }

  // Hover dwell adaptive focus (§7.6 #3)
  function handleTileHover(asset: Asset) {
    focusAsset(asset.id, { source: 'hover' });
  }

  // Marquee drag selection (§4.5)
  function handleContainerMouseDown(e: MouseEvent) {
    if (!containerRef) return;
    if (e.target !== containerRef && !(e.target as HTMLElement).classList.contains('virtual-grid-content')) {
      return;
    }
    const rect = containerRef.getBoundingClientRect();
    marqueeStart = { x: e.clientX - rect.left, y: e.clientY - rect.top + scrollTop };
    marqueeCurrent = { ...marqueeStart };
    isMarqueeActive = true;

    const onMouseMove = (ev: MouseEvent) => {
      if (!containerRef) return;
      const r = containerRef.getBoundingClientRect();
      marqueeCurrent = { x: ev.clientX - r.left, y: ev.clientY - r.top + scrollTop };
    };

    const onMouseUp = () => {
      isMarqueeActive = false;
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  // Keyboard navigation & Shortcuts (§12)
  function handleKeyDown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      // Select All (§4.5)
      const allSet = new Set<string>();
      for (let i = 0; i < Math.min(totalAssets, 2000); i++) {
        const a = assetsMap.get(i);
        if (a) allSet.add(a.id);
      }
      selectedIds = allSet;
      onselectionchange?.(allSet);
    } else if (e.key === 'Escape') {
      // Clear selection
      selectedIds = new Set();
      onselectionchange?.(new Set());
    }
  }
</script>

<svelte:window onkeydown={handleKeyDown} />

<div
  bind:this={containerRef}
  class="virtual-grid-container"
  role="grid"
  tabindex="0"
  aria-label="Photo library continuous grid"
  onscroll={handleScroll}
  onmousedown={handleContainerMouseDown}
>
  <!-- Sticky Date Pill top-left (§4.3) -->
  <div class="sticky-date-pill {datePillVisible ? 'visible' : ''}">
    <span>{datePillText}</span>
  </div>

  {#if totalAssets === 0}
    <div class="empty-grid-notice">
      <p class="empty-notice-title">No Photos Found</p>
      <p class="empty-notice-desc">The library has no items in this view. Use the Import button in the toolbar to load photos or seed demo data.</p>
    </div>
  {/if}

  <!-- Content canvas with virtualized row spacer -->
  <div
    class="virtual-grid-content"
    style="height: {totalGridHeight}px; width: {containerWidth}px;"
  >
    {#each Array(Math.max(0, endRow - startRow)) as _, r}
      {@const rowIndex = startRow + r}
      {@const rowTop = rowIndex * rowHeight}
      <div
        class="virtual-grid-row"
        style="top: {rowTop}px; height: {actualTileSize}px; gap: {gap}px;"
      >
        {#each Array(cols) as __, c}
          {@const itemIndex = rowIndex * cols + c}
          {#if itemIndex < totalAssets}
            {@const asset = assetsMap.get(itemIndex)}
            {#if asset}
              <Tile
                {asset}
                size={actualTileSize}
                {isFill}
                isSelected={selectedIds.has(asset.id)}
                onclick={(e) => handleTileClick(itemIndex, asset, e)}
                ondblclick={() => handleTileDoubleClick(asset)}
                onpointerenter={() => handleTileHover(asset)}
              />
            {:else}
              <div
                class="tile-skeleton"
                style="width: {actualTileSize}px; height: {actualTileSize}px;"
              ></div>
            {/if}
          {/if}
        {/each}
      </div>
    {/each}
  </div>

  <!-- Marquee Selection Rectangle (§4.5) -->
  {#if isMarqueeActive}
    {@const left = Math.min(marqueeStart.x, marqueeCurrent.x)}
    {@const top = Math.min(marqueeStart.y, marqueeCurrent.y) - scrollTop}
    {@const width = Math.abs(marqueeCurrent.x - marqueeStart.x)}
    {@const height = Math.abs(marqueeCurrent.y - marqueeStart.y)}
    <div
      class="marquee-box"
      style="left: {left}px; top: {top}px; width: {width}px; height: {height}px;"
    ></div>
  {/if}

  <!-- 6px Right-edge Scrubber on Hover (§4.3) -->
  <div
    role="slider"
    tabindex="-1"
    aria-label="Timeline scrubber"
    aria-valuenow={Math.round(scrollTop / rowHeight)}
    class="right-edge-scrubber {isHoveringScrubber || isDraggingScrubber ? 'active' : ''}"
    onpointerenter={() => (isHoveringScrubber = true)}
    onpointerleave={() => (isHoveringScrubber = false)}
    onpointerdown={handleScrubberPointerDown}
  >
    {#if isHoveringScrubber || isDraggingScrubber}
      <div
        class="scrubber-bubble"
        style="top: {Math.max(20, Math.min(containerHeight - 30, scrubberBubbleY))}px;"
      >
        {scrubberBubbleText || datePillText}
      </div>
    {/if}
  </div>
</div>

<style>
  .virtual-grid-container {
    position: relative;
    width: 100%;
    height: 100%;
    overflow-y: scroll;
    overflow-x: hidden;
    background: transparent;
    outline: none;
    box-sizing: border-box;
    padding-top: var(--toolbar-h);
  }

  .virtual-grid-content {
    position: relative;
    margin: 0 auto;
  }

  .virtual-grid-row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    justify-content: flex-start;
  }

  .tile-skeleton {
    background: var(--fill-1);
    box-sizing: border-box;
  }

  /* Sticky Date Pill (§4.3) */
  .sticky-date-pill {
    position: fixed;
    top: calc(var(--toolbar-h) + 14px);
    left: 260px;
    padding: 6px 14px;
    border-radius: var(--r-pill);
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(20px);
    -webkit-backdrop-filter: blur(20px);
    color: #ffffff;
    font-size: var(--text-size-caption);
    font-weight: 600;
    letter-spacing: -0.1px;
    box-shadow: var(--shadow-1);
    z-index: 50;
    pointer-events: none;
    opacity: 0;
    transform: translateY(-4px);
    transition: opacity 300ms var(--ease-standard),
                transform 300ms var(--ease-standard);
  }

  .sticky-date-pill.visible {
    opacity: 1;
    transform: translateY(0);
  }

  /* 6px Right-edge Scrubber (§4.3) */
  .right-edge-scrubber {
    position: fixed;
    top: var(--toolbar-h);
    right: 0;
    bottom: 0;
    width: 6px;
    z-index: 60;
    background: transparent;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard),
                width var(--dur-fast) var(--ease-standard);
  }

  .right-edge-scrubber:hover,
  .right-edge-scrubber.active {
    width: 10px;
    background: var(--fill-2);
  }

  .scrubber-bubble {
    position: absolute;
    right: 18px;
    transform: translateY(-50%);
    padding: 4px 10px;
    border-radius: var(--r-sm);
    background: var(--surface-overlay);
    backdrop-filter: blur(30px);
    -webkit-backdrop-filter: blur(30px);
    color: var(--text-1);
    font-size: var(--text-size-caption);
    font-weight: 600;
    box-shadow: var(--shadow-2);
    border: 1px solid var(--separator);
    white-space: nowrap;
    pointer-events: none;
  }

  /* Marquee selection box (§4.5) */
  .marquee-box {
    position: absolute;
    border: 1.5px solid var(--accent);
    background: color-mix(in oklab, var(--accent) 15%, transparent);
    border-radius: 4px;
    pointer-events: none;
    z-index: 40;
  }

  .empty-grid-notice {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    max-width: 360px;
    gap: 8px;
    z-index: 10;
  }

  .empty-notice-title {
    font-size: var(--text-size-title);
    font-weight: 600;
    color: var(--text-1);
    margin: 0;
  }

  .empty-notice-desc {
    font-size: var(--text-size-body);
    line-height: var(--text-lh-body);
    color: var(--text-2);
    margin: 0;
  }
</style>
