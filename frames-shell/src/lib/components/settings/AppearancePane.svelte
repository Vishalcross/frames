<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import SegmentedControl from '$lib/components/surface/SegmentedControl.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { theme, resolved, syncDOM } from '$lib/theme/engine.svelte';
  import type { AdaptiveIntensity, Appearance, ColourSource, GlassClarity, GlassTint, Scheme } from '$lib/types';

  let {
    onclose,
    ontoggleGallery,
    onopenConnection
  }: {
    onclose?: () => void;
    ontoggleGallery?: () => void;
    onopenConnection?: () => void;
  } = $props();

  const appearances: { id: Appearance; label: string; desc: string }[] = [
    {
      id: 'standard',
      label: 'Standard',
      desc: 'The familiar Apple Photos look: flat, neutral, quiet chrome.'
    },
    {
      id: 'adaptive',
      label: 'Adaptive',
      desc: 'Takes its vibrant ambient color directly from the photo in front of you.'
    },
    {
      id: 'glass',
      label: 'Liquid Glass',
      desc: 'Translucent lensing material for floating controls with physical depth.'
    }
  ];

  const standardAccents = [
    { name: 'Blue', color: '#007aff' },
    { name: 'Purple', color: '#af52de' },
    { name: 'Pink', color: '#ff2d55' },
    { name: 'Red', color: '#ff3b30' },
    { name: 'Orange', color: '#ff9500' },
    { name: 'Yellow', color: '#ffcc00' },
    { name: 'Green', color: '#34c759' },
    { name: 'Graphite', color: '#8e8e93' }
  ];

  function setAppearance(app: Appearance) {
    theme.appearance = app;
    syncDOM();
  }

  function setScheme(sch: Scheme) {
    theme.scheme = sch;
    syncDOM();
  }

  function setAccent(acc: string) {
    theme.accent = acc;
    syncDOM();
  }

  function setIntensity(int: AdaptiveIntensity) {
    theme.intensity = int;
    syncDOM();
  }

  function setColourSource(cs: ColourSource) {
    theme.colourSource = cs;
    syncDOM();
  }

  function setGlassTint(gt: GlassTint) {
    theme.glassTint = gt;
    syncDOM();
  }

  function setGlassClarity(gc: GlassClarity) {
    theme.glassClarity = gc;
    syncDOM();
  }

  function toggleTransparency() {
    theme.reduceTransparency = !theme.reduceTransparency;
    syncDOM();
  }
</script>

<div class="settings-modal-backdrop">
  <Surface kind="sheet" class="appearance-sheet">
    <div class="sheet-header">
      <div class="sheet-title-group">
        <h2 class="sheet-title">Appearance Settings</h2>
        <span class="sheet-sub">Customise visual material, colour derivation and chrome styles</span>
      </div>
      <div class="sheet-header-actions">
        <Button variant="ghost" onclick={onopenConnection}>
          <Icon name="cloud" size={15} />
          <span>MEGA S4 Connection…</span>
        </Button>
        <Button variant="ghost" onclick={ontoggleGallery}>
          <Icon name="layers" size={15} />
          <span>Component Gallery</span>
        </Button>
        <Button variant="icon" title="Close" onclick={onclose}>
          <Icon name="x" size={15} />
        </Button>
      </div>
    </div>

    <div class="sheet-content">
      <!-- 1. Three Large Live-Preview Cards (§4.11) -->
      <div class="preview-cards-section">
        <div class="section-heading">Visual Mode</div>
        <div class="preview-cards-grid">
          {#each appearances as app}
            {@const isSelected = theme.appearance === app.id}
            <button
              type="button"
              class="appearance-preview-card {isSelected ? 'selected' : ''}"
              onclick={() => setAppearance(app.id)}
            >
              <!-- Live preview mockup -->
              <div class="mockup-window appearance-{app.id}">
                <div class="mockup-sidebar"></div>
                <div class="mockup-content">
                  <div class="mockup-toolbar"></div>
                  <div class="mockup-grid">
                    <div class="mockup-tile"></div>
                    <div class="mockup-tile"></div>
                    <div class="mockup-tile"></div>
                    <div class="mockup-tile"></div>
                  </div>
                </div>
              </div>

              <div class="card-info">
                <div class="card-title-row">
                  <span class="card-title">{app.label}</span>
                  {#if isSelected}
                    <span class="check-icon"><Icon name="check" size={14} stroke={2.5} /></span>
                  {/if}
                </div>
                <span class="card-desc">{app.desc}</span>
              </div>
            </button>
          {/each}
        </div>
      </div>

      <!-- 2. Controls Section (§4.11 Table) -->
      <div class="appearance-controls-group">
        <!-- Scheme: Auto / Light / Dark / Follow photo -->
        <div class="control-row">
          <div class="control-label-group">
            <span class="control-label">Color Scheme</span>
            <span class="control-sub">Light or dark interface style</span>
          </div>
          <div class="control-input-wrap">
            <div class="scheme-buttons">
              <Button
                variant={theme.scheme === 'system' ? 'accent' : 'standard'}
                onclick={() => setScheme('system')}
              >
                Auto (System)
              </Button>
              <Button
                variant={theme.scheme === 'light' ? 'accent' : 'standard'}
                onclick={() => setScheme('light')}
              >
                Light
              </Button>
              <Button
                variant={theme.scheme === 'dark' ? 'accent' : 'standard'}
                onclick={() => setScheme('dark')}
              >
                Dark
              </Button>
              {#if theme.appearance === 'adaptive'}
                <Button
                  variant={theme.scheme === 'photo' ? 'accent' : 'standard'}
                  onclick={() => setScheme('photo')}
                  title="Dark or light derived from photo luminance"
                >
                  Follow photo
                </Button>
              {/if}
            </div>
          </div>
        </div>

        <!-- Mode-dependent controls: Standard Accent (§4.11) -->
        {#if theme.appearance === 'standard'}
          <div class="control-row">
            <div class="control-label-group">
              <span class="control-label">Accent Color</span>
              <span class="control-sub">Highlights and interactive elements</span>
            </div>
            <div class="accent-swatches">
              {#each standardAccents as acc}
                <button
                  type="button"
                  class="accent-swatch {theme.accent === acc.name ? 'selected' : ''}"
                  style="background: {acc.color};"
                  title={acc.name}
                  onclick={() => setAccent(acc.name)}
                ></button>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Mode-dependent controls: Adaptive Intensity & Colour Source (§4.11) -->
        {#if theme.appearance === 'adaptive'}
          <div class="control-row">
            <div class="control-label-group">
              <span class="control-label">Chroma Intensity</span>
              <span class="control-sub">Saturation strength of tonal roles</span>
            </div>
            <div class="scheme-buttons">
              <Button
                variant={theme.intensity === 'subtle' ? 'accent' : 'standard'}
                onclick={() => setIntensity('subtle')}
              >
                Subtle (×0.6)
              </Button>
              <Button
                variant={theme.intensity === 'balanced' ? 'accent' : 'standard'}
                onclick={() => setIntensity('balanced')}
              >
                Balanced (×1.0)
              </Button>
              <Button
                variant={theme.intensity === 'vivid' ? 'accent' : 'standard'}
                onclick={() => setIntensity('vivid')}
              >
                Vivid (×1.5)
              </Button>
            </div>
          </div>

          <div class="control-row">
            <div class="control-label-group">
              <span class="control-label">Colour Source</span>
              <span class="control-sub">Where ambient tints are active</span>
            </div>
            <div class="scheme-buttons">
              <Button
                variant={theme.colourSource === 'viewer' ? 'accent' : 'standard'}
                onclick={() => setColourSource('viewer')}
              >
                Viewer only
              </Button>
              <Button
                variant={theme.colourSource === 'both' ? 'accent' : 'standard'}
                onclick={() => setColourSource('both')}
              >
                Viewer and browsing
              </Button>
            </div>
          </div>
        {/if}

        <!-- Mode-dependent controls: Liquid Glass Tint & Clarity (§4.11) -->
        {#if theme.appearance === 'glass'}
          <div class="control-row">
            <div class="control-label-group">
              <span class="control-label">Glass Tint</span>
              <span class="control-sub">Material color filtration</span>
            </div>
            <div class="scheme-buttons">
              <Button
                variant={theme.glassTint === 'neutral' ? 'accent' : 'standard'}
                onclick={() => setGlassTint('neutral')}
              >
                Neutral
              </Button>
              <Button
                variant={theme.glassTint === 'adaptive' ? 'accent' : 'standard'}
                onclick={() => setGlassTint('adaptive')}
              >
                Adaptive (Photo seed)
              </Button>
            </div>
          </div>

          <div class="control-row">
            <div class="control-label-group">
              <span class="control-label">Clarity</span>
              <span class="control-sub">Lensing blur depth over photos</span>
            </div>
            <div class="scheme-buttons">
              <Button
                variant={theme.glassClarity === 'regular' ? 'accent' : 'standard'}
                onclick={() => setGlassClarity('regular')}
              >
                Regular (24px)
              </Button>
              <Button
                variant={theme.glassClarity === 'clear' ? 'accent' : 'standard'}
                onclick={() => setGlassClarity('clear')}
              >
                Clear (8px)
              </Button>
            </div>
          </div>
        {/if}

        <!-- Reduce Transparency toggle (§4.11) -->
        <div class="control-row">
          <div class="control-label-group">
            <span class="control-label">Reduce Transparency</span>
            <span class="control-sub">Replaces glass and blurs with solid fills (Governor Level 3)</span>
          </div>
          <input
            type="checkbox"
            checked={theme.reduceTransparency}
            onchange={toggleTransparency}
            class="toggle-switch"
          />
        </div>
      </div>
    </div>
  </Surface>
</div>

<style>
  .settings-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 700;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(10px);
    -webkit-backdrop-filter: blur(10px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }

  :global(.appearance-sheet) {
    width: 760px;
    max-width: 95vw;
    max-height: 90vh;
    border-radius: var(--r-xl);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    animation: sheetIn 220ms var(--ease-overshoot);
  }

  @keyframes sheetIn {
    from {
      opacity: 0;
      transform: scale(0.96) translateY(12px);
    }
    to {
      opacity: 1;
      transform: scale(1) translateY(0);
    }
  }

  .sheet-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--separator);
  }

  .sheet-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    margin: 0;
    color: var(--text-1);
  }

  .sheet-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }

  .sheet-header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .sheet-content {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .section-heading {
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 10px;
  }

  /* 3 Live Preview Cards (§4.11) */
  .preview-cards-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 14px;
  }

  .appearance-preview-card {
    display: flex;
    flex-direction: column;
    border: 2px solid var(--separator);
    border-radius: var(--r-md);
    background: var(--surface-raised);
    padding: 10px;
    cursor: pointer;
    text-align: left;
    gap: 10px;
    outline: none;
    transition: border-color var(--dur-fast) var(--ease-standard),
                transform var(--dur-fast) var(--ease-standard);
  }
  .appearance-preview-card:hover {
    border-color: var(--text-2);
    transform: translateY(-2px);
  }
  .appearance-preview-card.selected {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .mockup-window {
    width: 100%;
    height: 100px;
    border-radius: 8px;
    overflow: hidden;
    display: flex;
    border: 1px solid var(--separator);
  }

  .appearance-standard .mockup-sidebar {
    width: 30%;
    background: #ececf0;
  }
  .appearance-standard .mockup-content {
    flex: 1;
    background: #ffffff;
    display: flex;
    flex-direction: column;
  }
  .appearance-standard .mockup-toolbar {
    height: 16px;
    background: #f8f8fa;
    border-bottom: 1px solid #e0e0e4;
  }

  .appearance-adaptive .mockup-sidebar {
    width: 30%;
    background: rgba(30, 90, 160, 0.4);
    backdrop-filter: blur(10px);
  }
  .appearance-adaptive .mockup-content {
    flex: 1;
    background: radial-gradient(circle at 10% 10%, rgba(0, 122, 255, 0.3), rgba(28, 28, 30, 0.9));
    display: flex;
    flex-direction: column;
  }
  .appearance-adaptive .mockup-toolbar {
    height: 16px;
    background: rgba(30, 90, 160, 0.3);
  }

  .appearance-glass .mockup-sidebar {
    width: 28%;
    margin: 4px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.2);
    backdrop-filter: blur(8px);
    border: 1px solid rgba(255, 255, 255, 0.4);
  }
  .appearance-glass .mockup-content {
    flex: 1;
    background: #111113;
    display: flex;
    flex-direction: column;
    padding: 4px;
  }
  .appearance-glass .mockup-toolbar {
    height: 14px;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.25);
    backdrop-filter: blur(8px);
  }

  .mockup-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 4px;
    padding: 6px;
    flex: 1;
  }
  .mockup-tile {
    background: var(--fill-2);
    border-radius: 3px;
  }

  .card-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .card-title-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .card-title {
    font-size: var(--text-size-body);
    font-weight: 600;
    color: var(--text-1);
  }
  .check-icon {
    color: var(--accent);
  }
  .card-desc {
    font-size: 11px;
    color: var(--text-2);
    line-height: 14px;
  }

  /* Control rows */
  .appearance-controls-group {
    display: flex;
    flex-direction: column;
    gap: 16px;
    border-top: 1px solid var(--separator);
    padding-top: 16px;
  }

  .control-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .control-label-group {
    display: flex;
    flex-direction: column;
  }
  .control-label {
    font-size: var(--text-size-body);
    font-weight: 600;
    color: var(--text-1);
  }
  .control-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }

  .scheme-buttons {
    display: flex;
    gap: 6px;
  }

  .accent-swatches {
    display: flex;
    gap: 8px;
  }
  .accent-swatch {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    border: none;
    cursor: pointer;
    outline: none;
    transition: transform var(--dur-fast) var(--ease-standard);
  }
  .accent-swatch:hover {
    transform: scale(1.15);
  }
  .accent-swatch.selected {
    box-shadow: 0 0 0 2px var(--surface-raised), 0 0 0 4px var(--accent);
  }

  .toggle-switch {
    width: 36px;
    height: 20px;
    accent-color: var(--accent);
    cursor: pointer;
  }
</style>
