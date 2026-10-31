<script lang="ts">
  import { onMount } from 'svelte';
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Badge from '$lib/components/surface/Badge.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { tauriRepo } from '$lib/services/tauriRepo';
  import type { Asset, AssetDetail } from '$lib/types';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    asset,
    open = $bindable(false),
    onclose
  }: {
    asset: Asset;
    open: boolean;
    onclose?: () => void;
  } = $props();

  let detail = $state<AssetDetail | null>(null);
  let captionText = $state('');
  let isEditingCaption = $state(false);

  $effect(() => {
    if (asset) {
      tauriRepo.detail(asset.id).then((d) => {
        detail = d;
        captionText = d.caption;
      });
    }
  });

  function saveCaption() {
    isEditingCaption = false;
    if (detail) {
      detail.caption = captionText;
      tauriRepo.updateCaption(asset.id, captionText);
    }
  }

  const formattedDateTime = $derived.by(() => {
    if (!asset) return '';
    return new Date(asset.takenAt).toLocaleString('en-US', {
      weekday: 'short',
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: 'numeric',
      minute: '2-digit'
    });
  });
</script>

{#if open && detail}
  <Surface
    kind="panel"
    class="info-panel-surface appearance-{theme.appearance}"
  >
    <div class="info-header">
      <span class="info-header-title">Info</span>
      <Button variant="icon" title="Close info (⌘I)" onclick={onclose}>
        <Icon name="x" size={14} />
      </Button>
    </div>

    <div class="info-content">
      <!-- 1. Caption Field (§4.7) -->
      <div class="info-section">
        <label for="info-caption-input" class="section-label">Caption</label>
        {#if isEditingCaption}
          <div class="caption-edit-wrap">
            <textarea
              id="info-caption-input"
              class="caption-textarea"
              bind:value={captionText}
              rows="3"
            ></textarea>
            <div class="caption-actions">
              <Button variant="accent" onclick={saveCaption}>Save</Button>
            </div>
          </div>
        {:else}
          <button
            type="button"
            class="caption-display"
            onclick={() => (isEditingCaption = true)}
            title="Click to edit caption"
          >
            {captionText || 'Add a caption…'}
          </button>
        {/if}
      </div>

      <!-- 2. Date and Time (editable, §4.7) -->
      <div class="info-section">
        <div class="section-label">Date and time</div>
        <div class="datetime-row">
          <Icon name="calendar" size={15} />
          <span class="datetime-text">{formattedDateTime}</span>
        </div>
      </div>

      <!-- 3. File Metadata: filename, dimensions, size (§4.7) -->
      <div class="info-section">
        <div class="section-label">File details</div>
        <div class="meta-row">
          <span class="meta-prop">Filename</span>
          <span class="meta-val font-mono">{detail.filename}</span>
        </div>
        <div class="meta-row">
          <span class="meta-prop">Dimensions</span>
          <span class="meta-val">{detail.dimensions}</span>
        </div>
        <div class="meta-row">
          <span class="meta-prop">Size</span>
          <span class="meta-val">{detail.fileSize}</span>
        </div>
      </div>

      <!-- 4. Camera, lens and exposure chips (§4.7) -->
      <div class="info-section">
        <div class="section-label">Camera & exposure</div>
        <div class="camera-info-block">
          <div class="camera-make-model">
            <Icon name="camera" size={15} />
            <span>{detail.camera}</span>
          </div>
          <div class="camera-lens">{detail.lens}</div>
          <div class="exposure-chips">
            {#each detail.exposure.split('·') as chip}
              <Badge variant="pill" text={chip.trim()} />
            {/each}
          </div>
        </div>
      </div>

      <!-- 5. Location (Static placeholder tile, no network calls, §4.7) -->
      <div class="info-section">
        <div class="section-label">Location</div>
        <div class="location-map-tile">
          <!-- Static SVG Map placeholder (§4.7) -->
          <svg viewBox="0 0 300 120" width="100%" height="100px" class="map-svg">
            <rect width="300" height="120" fill="var(--fill-1)" />
            <!-- Stylized roads & coastlines -->
            <path d="M 0 40 Q 90 20, 180 60 T 300 40" stroke="var(--separator)" stroke-width="6" fill="none" />
            <path d="M 40 120 Q 120 70, 200 100 T 300 110" stroke="var(--separator)" stroke-width="4" fill="none" />
            <path d="M 150 0 L 150 120" stroke="var(--fill-2)" stroke-width="2" />
            <!-- Pin location marker -->
            <circle cx="150" cy="55" r="10" fill="var(--accent)" opacity="0.3" />
            <circle cx="150" cy="55" r="5" fill="var(--accent)" />
          </svg>
          <div class="location-name-pill">
            <Icon name="compass" size={13} />
            <span>{detail.locationName}</span>
          </div>
        </div>
      </div>

      <!-- 6. Albums containing the photo (§4.7) -->
      <div class="info-section">
        <div class="section-label">Albums</div>
        <div class="albums-tag-list">
          {#each detail.albums as album}
            <Badge variant="pill" icon="album" text={album} />
          {/each}
        </div>
      </div>

      <!-- 7. Keywords (§4.7) -->
      <div class="info-section">
        <div class="section-label">Keywords</div>
        <div class="keywords-list">
          {#each detail.keywords as kw}
            <span class="keyword-chip">#{kw}</span>
          {/each}
        </div>
      </div>
    </div>
  </Surface>
{/if}

<style>
  :global(.info-panel-surface) {
    width: var(--info-panel-w, 320px);
    height: 100vh;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    user-select: none;
    z-index: 150;
    flex-shrink: 0;
  }

  :global(.appearance-glass .info-panel-surface) {
    position: absolute;
    top: 8px;
    right: 8px;
    height: calc(100vh - 16px);
    border-radius: var(--r-xl);
  }

  .info-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--toolbar-h);
    padding: 0 16px;
    border-bottom: 1px solid var(--separator);
    flex-shrink: 0;
  }

  .info-header-title {
    font-size: var(--text-size-title);
    font-weight: 600;
    color: var(--text-1);
  }

  .info-content {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .info-section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .section-label {
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .caption-display {
    width: 100%;
    padding: 8px 10px;
    border-radius: var(--r-sm);
    border: 1px dashed var(--separator);
    background: transparent;
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    line-height: var(--text-lh-body);
    text-align: left;
    cursor: pointer;
  }
  .caption-display:hover {
    background: var(--fill-1);
  }

  .caption-textarea {
    width: 100%;
    box-sizing: border-box;
    padding: 8px;
    border-radius: var(--r-sm);
    border: 1px solid var(--accent);
    background: var(--surface-raised);
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    outline: none;
    resize: none;
  }

  .caption-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 4px;
  }

  .datetime-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-size-body);
    color: var(--text-1);
  }

  .meta-row {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-size-body);
  }
  .meta-prop {
    color: var(--text-2);
  }
  .meta-val {
    color: var(--text-1);
  }
  .font-mono {
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .camera-info-block {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px;
    background: var(--fill-1);
    border-radius: var(--r-sm);
  }
  .camera-make-model {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    font-size: var(--text-size-body);
    color: var(--text-1);
  }
  .camera-lens {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }
  .exposure-chips {
    display: flex;
    gap: 4px;
    margin-top: 4px;
    flex-wrap: wrap;
  }

  .location-map-tile {
    position: relative;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--fill-1);
    border: 1px solid var(--separator);
  }
  .map-svg {
    display: block;
    width: 100%;
    height: 90px;
  }
  .location-name-pill {
    position: absolute;
    bottom: 6px;
    left: 6px;
    right: 6px;
    padding: 4px 8px;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
    color: #ffffff;
    border-radius: var(--r-xs);
    font-size: 11px;
    font-weight: 500;
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .albums-tag-list {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .keywords-list {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .keyword-chip {
    font-size: var(--text-size-caption);
    color: var(--accent);
    background: var(--fill-1);
    padding: 2px 8px;
    border-radius: var(--r-pill);
  }
</style>
