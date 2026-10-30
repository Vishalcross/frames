<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import SegmentedControl from '$lib/components/surface/SegmentedControl.svelte';
  import Slider from '$lib/components/surface/Slider.svelte';
  import Field from '$lib/components/surface/Field.svelte';
  import Badge from '$lib/components/surface/Badge.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';

  let {
    onclose
  }: {
    onclose?: () => void;
  } = $props();

  let segVal = $state('all');
  let sliderVal = $state(45);
  let searchVal = $state('Landscape');

  const appearances: ('standard' | 'adaptive' | 'glass')[] = ['standard', 'adaptive', 'glass'];
  const schemes: ('light' | 'dark')[] = ['light', 'dark'];
</script>

<div class="gallery-modal-backdrop">
  <div class="gallery-modal-container">
    <header class="gallery-header">
      <div class="gallery-title-wrap">
        <h2>Component Kit Gallery (§13 M0 Acceptance)</h2>
        <span class="gallery-sub">Every component in 3 appearances × 2 schemes</span>
      </div>
      <Button variant="icon" title="Close Gallery" onclick={onclose}>
        <Icon name="x" size={16} />
      </Button>
    </header>

    <div class="gallery-scrollable">
      {#each appearances as app}
        <div class="appearance-showcase-section">
          <h3 class="app-section-title">Mode: {app.toUpperCase()}</h3>

          <div class="schemes-grid">
            {#each schemes as sch}
              <div
                class="scheme-box"
                data-appearance={app}
                data-scheme={sch}
                data-glass-tier="1"
              >
                <div class="scheme-header">
                  <span class="scheme-name">{sch.toUpperCase()}</span>
                </div>

                <div class="components-column">
                  <!-- Surface Controls -->
                  <div class="comp-row">
                    <span class="comp-label">Buttons</span>
                    <div class="comp-items">
                      <Button variant="standard">Standard</Button>
                      <Button variant="accent">Accent</Button>
                      <Button variant="capsule">Capsule</Button>
                      <Button variant="icon" title="Icon"><Icon name="heart" size={14} /></Button>
                      <Button variant="danger">Danger</Button>
                    </div>
                  </div>

                  <!-- Segmented Control -->
                  <div class="comp-row">
                    <span class="comp-label">Segmented</span>
                    <SegmentedControl
                      options={[
                        { value: 'years', label: 'Years' },
                        { value: 'months', label: 'Months' },
                        { value: 'all', label: 'All' }
                      ]}
                      bind:value={segVal}
                    />
                  </div>

                  <!-- Slider -->
                  <div class="comp-row">
                    <span class="comp-label">Slider</span>
                    <Slider
                      label="Brilliance"
                      min={0}
                      max={100}
                      bind:value={sliderVal}
                      defaultValue={0}
                      width="180px"
                    />
                  </div>

                  <!-- Field -->
                  <div class="comp-row">
                    <span class="comp-label">Search Field</span>
                    <Field
                      icon="search"
                      placeholder="Search items…"
                      bind:value={searchVal}
                      baseWidth="180px"
                    />
                  </div>

                  <!-- Badges -->
                  <div class="comp-row">
                    <span class="comp-label">Badges</span>
                    <div class="comp-items">
                      <Badge variant="video" text="0:45" />
                      <Badge variant="cloud"><Icon name="cloud" size={11} /></Badge>
                      <Badge variant="favorite"><Icon name="heart-filled" size={13} /></Badge>
                      <Badge variant="count" text="1,420" />
                    </div>
                  </div>

                  <!-- Surface Floating / Popover -->
                  <div class="comp-row">
                    <span class="comp-label">Surface Capsule</span>
                    <Surface kind="floating" style="padding: 6px 14px; gap: 8px; display: inline-flex; align-items: center;">
                      <Icon name="sparkles" size={14} />
                      <span style="font-size: 12px; font-weight: 600;">Floating Surface</span>
                    </Surface>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .gallery-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 800;
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(16px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }

  .gallery-modal-container {
    width: 1200px;
    max-width: 96vw;
    height: 90vh;
    background: var(--surface-raised);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow-3);
    border: 1px solid var(--separator);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .gallery-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-bottom: 1px solid var(--separator);
  }

  .gallery-header h2 {
    margin: 0;
    font-size: var(--text-size-title);
    color: var(--text-1);
  }

  .gallery-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }

  .gallery-scrollable {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 32px;
  }

  .appearance-showcase-section {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .app-section-title {
    font-size: var(--text-size-body);
    font-weight: 700;
    letter-spacing: 0.5px;
    color: var(--accent);
    margin: 0;
  }

  .schemes-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 16px;
  }

  .scheme-box {
    border-radius: var(--r-lg);
    background: var(--bg-canvas);
    color: var(--text-1);
    border: 1px solid var(--separator);
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    box-shadow: var(--shadow-1);
  }

  .scheme-header {
    font-size: var(--text-size-caption);
    font-weight: 700;
    color: var(--text-2);
    border-bottom: 1px solid var(--separator);
    padding-bottom: 6px;
  }

  .components-column {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .comp-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .comp-label {
    font-size: var(--text-size-caption);
    color: var(--text-2);
    width: 90px;
  }

  .comp-items {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
