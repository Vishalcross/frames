<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Slider from '$lib/components/surface/Slider.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { getThumbnailDataUrl } from '$lib/mock/mockRepo';
  import type { Asset } from '$lib/types';
  import { theme } from '$lib/theme/engine.svelte';

  interface EditStack {
    // Light
    exposure: number; // -100..100
    brilliance: number;
    highlights: number;
    shadows: number;
    contrast: number;
    brightness: number;
    blackPoint: number;
    // Color
    saturation: number;
    vibrance: number;
    warmth: number;
    tint: number;
    // B&W
    blackAndWhite: boolean;
    // Filter preset
    filter: 'none' | 'vivid' | 'dramatic' | 'silvertone' | 'noir';
  }

  const defaultStack: EditStack = {
    exposure: 0,
    brilliance: 0,
    highlights: 0,
    shadows: 0,
    contrast: 0,
    brightness: 0,
    blackPoint: 0,
    saturation: 0,
    vibrance: 0,
    warmth: 0,
    tint: 0,
    blackAndWhite: false,
    filter: 'none'
  };

  let {
    asset,
    open = $bindable(false),
    oncancel,
    ondone
  }: {
    asset: Asset;
    open: boolean;
    oncancel?: () => void;
    ondone?: (savedStack: EditStack) => void;
  } = $props();

  let activeTab = $state<'adjust' | 'filters' | 'crop'>('adjust');
  let edits = $state<EditStack>({ ...defaultStack });

  // Collapsible inspector groups (§4.8)
  let lightExpanded = $state(true);
  let colorExpanded = $state(true);
  let bwExpanded = $state(false);

  // Compute live CSS preview filter stack (§4.8)
  const computedFilterStyle = $derived.by(() => {
    const brightnessPct = 100 + edits.exposure * 0.4 + edits.brightness * 0.3;
    const contrastPct = 100 + edits.contrast * 0.4;
    const saturatePct = edits.blackAndWhite ? 0 : Math.max(0, 100 + edits.saturation * 0.8 + edits.vibrance * 0.4);
    const sepiaPct = edits.warmth > 0 ? edits.warmth * 0.3 : 0;
    const hueRotateDeg = edits.tint * 0.6;

    let presetFilter = '';
    if (edits.filter === 'vivid') presetFilter = 'saturate(1.3) contrast(1.1)';
    else if (edits.filter === 'dramatic') presetFilter = 'contrast(1.35) brightness(0.95)';
    else if (edits.filter === 'silvertone') presetFilter = 'grayscale(1) contrast(1.2)';
    else if (edits.filter === 'noir') presetFilter = 'grayscale(1) contrast(1.6) brightness(0.85)';

    return `brightness(${brightnessPct}%) contrast(${contrastPct}%) saturate(${saturatePct}%) sepia(${sepiaPct}%) hue-rotate(${hueRotateDeg}deg) ${presetFilter}`;
  });

  function revertAll() {
    edits = { ...defaultStack };
  }

  function handleDone() {
    ondone?.(edits);
    open = false;
  }

  const previewUrl = $derived(getThumbnailDataUrl(asset, 'preview'));
</script>

{#if open}
  <div class="edit-modal-fullscreen appearance-{theme.appearance}">
    <!-- Edit Header: Revert, Tabs (Adjust, Filters, Crop, Tools), Cancel, Done (§4.8) -->
    <header class="edit-header">
      <Button variant="ghost" onclick={revertAll} title="Revert to original">
        Revert
      </Button>

      <div class="edit-tabs">
        <button
          type="button"
          class="tab-btn {activeTab === 'adjust' ? 'active' : ''}"
          onclick={() => (activeTab = 'adjust')}
        >
          Adjust
        </button>
        <button
          type="button"
          class="tab-btn {activeTab === 'filters' ? 'active' : ''}"
          onclick={() => (activeTab = 'filters')}
        >
          Filters
        </button>
        <button
          type="button"
          class="tab-btn {activeTab === 'crop' ? 'active' : ''}"
          onclick={() => (activeTab = 'crop')}
        >
          Crop
        </button>
        <!-- Disabled Tools tab reserved for later (§4.8) -->
        <button type="button" class="tab-btn disabled" disabled title="Reserved for Apple Intelligence edits">
          Tools
        </button>
      </div>

      <div class="edit-header-actions">
        <Button variant="ghost" onclick={oncancel}>Cancel</Button>
        <Button variant="accent" onclick={handleDone}>Done</Button>
      </div>
    </header>

    <div class="edit-workspace">
      <!-- Live Preview Canvas -->
      <div class="edit-canvas-viewport">
        <div class="edit-image-wrap">
          <img
            src={previewUrl}
            alt=""
            class="edit-image"
            style="filter: {computedFilterStyle};"
          />
        </div>
      </div>

      <!-- Right Inspector (280 px, §4.8) -->
      <aside class="edit-inspector">
        {#if activeTab === 'adjust'}
          <div class="inspector-scrollable">
            <!-- 1. Light Group (§4.8) -->
            <div class="slider-group">
              <button
                type="button"
                class="group-header"
                onclick={() => (lightExpanded = !lightExpanded)}
              >
                <span>Light</span>
                <Icon name={lightExpanded ? 'chevron-down' : 'chevron-right'} size={14} />
              </button>

              {#if lightExpanded}
                <div class="group-sliders">
                  <Slider label="Exposure" min={-100} max={100} bind:value={edits.exposure} defaultValue={0} />
                  <Slider label="Brilliance" min={-100} max={100} bind:value={edits.brilliance} defaultValue={0} />
                  <Slider label="Highlights" min={-100} max={100} bind:value={edits.highlights} defaultValue={0} />
                  <Slider label="Shadows" min={-100} max={100} bind:value={edits.shadows} defaultValue={0} />
                  <Slider label="Contrast" min={-100} max={100} bind:value={edits.contrast} defaultValue={0} />
                  <Slider label="Brightness" min={-100} max={100} bind:value={edits.brightness} defaultValue={0} />
                  <Slider label="Black Point" min={-100} max={100} bind:value={edits.blackPoint} defaultValue={0} />
                </div>
              {/if}
            </div>

            <!-- 2. Color Group (§4.8) -->
            <div class="slider-group">
              <button
                type="button"
                class="group-header"
                onclick={() => (colorExpanded = !colorExpanded)}
              >
                <span>Color</span>
                <Icon name={colorExpanded ? 'chevron-down' : 'chevron-right'} size={14} />
              </button>

              {#if colorExpanded}
                <div class="group-sliders">
                  <Slider label="Saturation" min={-100} max={100} bind:value={edits.saturation} defaultValue={0} />
                  <Slider label="Vibrance" min={-100} max={100} bind:value={edits.vibrance} defaultValue={0} />
                  <Slider label="Warmth" min={-100} max={100} bind:value={edits.warmth} defaultValue={0} />
                  <Slider label="Tint" min={-100} max={100} bind:value={edits.tint} defaultValue={0} />
                </div>
              {/if}
            </div>

            <!-- 3. Black & White Group (§4.8) -->
            <div class="slider-group">
              <button
                type="button"
                class="group-header"
                onclick={() => (bwExpanded = !bwExpanded)}
              >
                <span>Black & White</span>
                <Icon name={bwExpanded ? 'chevron-down' : 'chevron-right'} size={14} />
              </button>

              {#if bwExpanded}
                <div class="group-sliders">
                  <div class="bw-toggle-row">
                    <span class="bw-label">Enable B&W</span>
                    <input
                      type="checkbox"
                      bind:checked={edits.blackAndWhite}
                      class="toggle-checkbox"
                    />
                  </div>
                </div>
              {/if}
            </div>
          </div>
        {:else if activeTab === 'filters'}
          <div class="filters-panel">
            {#each ['none', 'vivid', 'dramatic', 'silvertone', 'noir'] as f}
              <button
                type="button"
                class="filter-preset-card {edits.filter === f ? 'selected' : ''}"
                onclick={() => (edits.filter = f as any)}
              >
                <div class="filter-preset-thumb {f}">
                  <img src={previewUrl} alt="" />
                </div>
                <span class="filter-preset-name">{f.toUpperCase()}</span>
              </button>
            {/each}
          </div>
        {:else if activeTab === 'crop'}
          <div class="crop-panel">
            <span class="crop-title">Aspect Ratio</span>
            <div class="crop-options">
              <Button variant="standard">Original</Button>
              <Button variant="standard">Square (1:1)</Button>
              <Button variant="standard">16:9</Button>
              <Button variant="standard">4:3</Button>
            </div>
          </div>
        {/if}
      </aside>
    </div>
  </div>
{/if}

<style>
  .edit-modal-fullscreen {
    position: fixed;
    inset: 0;
    z-index: 600;
    background: #000000;
    color: #ffffff;
    display: flex;
    flex-direction: column;
    user-select: none;
    animation: fadeIn 200ms var(--ease-standard);
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .edit-header {
    height: 52px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.12);
    flex-shrink: 0;
  }

  .edit-tabs {
    display: flex;
    gap: 4px;
    background: rgba(255, 255, 255, 0.1);
    padding: 3px;
    border-radius: var(--r-sm);
  }

  .tab-btn {
    border: none;
    background: transparent;
    color: rgba(255, 255, 255, 0.7);
    padding: 4px 14px;
    border-radius: 6px;
    font-size: var(--text-size-body);
    font-weight: 500;
    cursor: pointer;
  }
  .tab-btn.active {
    background: rgba(255, 255, 255, 0.25);
    color: #ffffff;
    font-weight: 600;
  }
  .tab-btn.disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .edit-header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .edit-workspace {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .edit-canvas-viewport {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    overflow: hidden;
  }

  .edit-image-wrap {
    max-width: 90%;
    max-height: 85vh;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .edit-image {
    max-width: 100%;
    max-height: 85vh;
    object-fit: contain;
    border-radius: 4px;
    box-shadow: 0 4px 40px rgba(0, 0, 0, 0.8);
  }

  /* Right inspector (280 px, §4.8) */
  .edit-inspector {
    width: var(--edit-inspector-w, 280px);
    background: rgba(28, 28, 30, 0.95);
    border-left: 1px solid rgba(255, 255, 255, 0.12);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
  }

  .inspector-scrollable {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .slider-group {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    padding-bottom: 14px;
  }

  .group-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    border: none;
    background: transparent;
    color: #ffffff;
    font-size: var(--text-size-body);
    font-weight: 600;
    cursor: pointer;
    padding: 0;
  }

  .group-sliders {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .bw-toggle-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: var(--text-size-body);
  }

  .filters-panel {
    padding: 16px;
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 12px;
  }

  .filter-preset-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    border: 2px solid transparent;
    border-radius: var(--r-sm);
    padding: 4px;
    background: transparent;
    cursor: pointer;
    color: #ffffff;
  }
  .filter-preset-card.selected {
    border-color: var(--accent);
  }

  .filter-preset-thumb {
    aspect-ratio: 1;
    border-radius: 4px;
    overflow: hidden;
  }
  .filter-preset-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .filter-preset-thumb.vivid img { filter: saturate(1.4); }
  .filter-preset-thumb.dramatic img { filter: contrast(1.4); }
  .filter-preset-thumb.silvertone img { filter: grayscale(1); }
  .filter-preset-thumb.noir img { filter: grayscale(1) contrast(1.8); }

  .filter-preset-name {
    font-size: 10px;
    font-weight: 600;
    text-align: center;
  }

  .crop-panel {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .crop-title {
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: rgba(255, 255, 255, 0.6);
  }
  .crop-options {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
</style>
