<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import SegmentedControl from '$lib/components/surface/SegmentedControl.svelte';
  import Slider from '$lib/components/surface/Slider.svelte';
  import Field from '$lib/components/surface/Field.svelte';
  import Menu, { type MenuItem } from '$lib/components/surface/Menu.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    currentRoute = 'library',
    librarySubView = $bindable<'years' | 'months' | 'all'>('all'),
    pageTitle = '',
    canGoBack = false,
    canGoForward = false,
    zoomLevelIndex = $bindable(2), // 0..5
    isFill = $bindable(true),
    filter = $bindable<'all' | 'favorites' | 'videos' | 'photos'>('all'),
    searchQuery = $bindable(''),
    hasSelectionOrOpen = false,
    isFavorite = false,
    infoPanelOpen = $bindable(false),
    isSidebarCollapsed = $bindable(false),
    isScrolled = false,
    useRealBackend = false,
    isTauri = false,
    onback,
    onforward,
    onshare,
    onfavorite,
    onrotate,
    onopenSettings,
    ontogglebackend,
    onimport
  }: {
    currentRoute: string;
    librarySubView?: 'years' | 'months' | 'all';
    pageTitle?: string;
    canGoBack?: boolean;
    canGoForward?: boolean;
    zoomLevelIndex?: number;
    isFill?: boolean;
    filter?: 'all' | 'favorites' | 'videos' | 'photos';
    searchQuery?: string;
    hasSelectionOrOpen?: boolean;
    isFavorite?: boolean;
    infoPanelOpen?: boolean;
    isSidebarCollapsed?: boolean;
    isScrolled?: boolean;
    useRealBackend?: boolean;
    isTauri?: boolean;
    onback?: () => void;
    onforward?: () => void;
    onshare?: () => void;
    onfavorite?: () => void;
    onrotate?: () => void;
    onopenSettings?: () => void;
    ontogglebackend?: () => void;
    onimport?: () => void;
  } = $props();

  let filterMenuOpen = $state(false);

  const filterOptions: MenuItem[] = $derived([
    { id: 'all', label: 'All Items', checked: filter === 'all' },
    { id: 'favorites', label: 'Favorites', icon: 'heart', checked: filter === 'favorites' },
    { id: 'photos', label: 'Photos Only', icon: 'photo', checked: filter === 'photos' },
    { id: 'videos', label: 'Videos Only', icon: 'video', checked: filter === 'videos' }
  ]);

  function handleFilterSelect(item: MenuItem) {
    filter = item.id as any;
  }

  const libraryViewOptions = [
    { value: 'years', label: 'Years' },
    { value: 'months', label: 'Months' },
    { value: 'all', label: 'All' }
  ];
</script>

{#if theme.appearance === 'glass'}
  <!-- LIQUID GLASS FLOATING CLUSTERS (§3, §8.4) -->
  <header class="glass-toolbar-container">
    <!-- Left Cluster -->
    <Surface kind="cluster" class="toolbar-cluster left-cluster">
      <Button
        variant="icon"
        title="Toggle Sidebar (⌥⌘S)"
        onclick={() => (isSidebarCollapsed = !isSidebarCollapsed)}
      >
        <Icon name="panel-left" size={17} />
      </Button>

      <div class="nav-history-buttons">
        <Button
          variant="icon"
          title="Back"
          disabled={!canGoBack}
          onclick={onback}
        >
          <Icon name="chevron-left" size={16} />
        </Button>
        <Button
          variant="icon"
          title="Forward"
          disabled={!canGoForward}
          onclick={onforward}
        >
          <Icon name="chevron-right" size={16} />
        </Button>
      </div>
    </Surface>

    <!-- Center Cluster -->
    <Surface kind="cluster" class="toolbar-cluster center-cluster">
      {#if currentRoute === 'library'}
        <SegmentedControl
          options={libraryViewOptions}
          bind:value={librarySubView}
        />
      {:else}
        <h1 class="cluster-page-title">{pageTitle}</h1>
      {/if}
    </Surface>

    <!-- Right Cluster -->
    <Surface kind="cluster" class="toolbar-cluster right-cluster">
      {#if currentRoute === 'library' && librarySubView === 'all'}
        <!-- 96px Zoom Slider (§4.2) -->
        <div class="zoom-slider-wrap">
          <Icon name="zoom-out" size={13} />
          <Slider
            min={0}
            max={5}
            step={1}
            bind:value={zoomLevelIndex}
            width="80px"
          />
          <Icon name="zoom-in" size={13} />
        </div>

        <!-- Fill / Fit toggle -->
        <Button
          variant="icon"
          title={isFill ? 'Fill (Square crops)' : 'Fit (Aspect ratio)'}
          active={!isFill}
          onclick={() => (isFill = !isFill)}
        >
          <Icon name={isFill ? 'fill' : 'fit'} size={15} />
        </Button>

        <!-- Filter menu -->
        <div class="relative-wrap">
          <Button
            variant="icon"
            title="Filter Library"
            active={filter !== 'all'}
            onclick={() => (filterMenuOpen = !filterMenuOpen)}
          >
            <Icon name="filter" size={15} />
          </Button>
          <Menu
            items={filterOptions}
            bind:open={filterMenuOpen}
            onselect={handleFilterSelect}
            style="top: 36px; right: 0;"
          />
        </div>
      {/if}

      <!-- Selection / Open photo actions (§4.2) -->
      <Button
        variant="icon"
        title="Share"
        disabled={!hasSelectionOrOpen}
        onclick={onshare}
      >
        <Icon name="share" size={15} />
      </Button>

      <Button
        variant="icon"
        title="Favorite (.)"
        disabled={!hasSelectionOrOpen}
        onclick={onfavorite}
      >
        <Icon name={isFavorite ? 'heart-filled' : 'heart'} size={15} class={isFavorite ? 'text-favorite' : ''} />
      </Button>

      <Button
        variant="icon"
        title="Rotate (⌘R)"
        disabled={!hasSelectionOrOpen}
        onclick={onrotate}
      >
        <Icon name="rotate-cw" size={15} />
      </Button>

      <Button
        variant="icon"
        title="Info (⌘I)"
        active={infoPanelOpen}
        onclick={() => (infoPanelOpen = !infoPanelOpen)}
      >
        <Icon name="info" size={15} />
      </Button>

      <!-- Expandable Search field (§4.2, §4.10) -->
      <Field
        icon="search"
        placeholder="Search"
        expandOnFocus={true}
        baseWidth="120px"
        expandedWidth="240px"
        bind:value={searchQuery}
      />

      <!-- Settings / Appearance toggle -->
      <Button
        variant="icon"
        title="Appearance & Settings"
        onclick={onopenSettings}
      >
        <Icon name="settings" size={15} />
      </Button>

      {#if onimport}
        <Button
          variant="icon"
          title="Import Photos…"
          onclick={onimport}
        >
          <Icon name="arrow-down-to-line" size={15} />
        </Button>
      {/if}

      {#if ontogglebackend}
        <button
          type="button"
          class="backend-mode-pill {useRealBackend ? 'is-sqlite' : 'is-benchmark'}"
          title="Click to toggle between SQLite Core and 100k Benchmark fixture"
          onclick={ontogglebackend}
        >
          <span class="mode-dot"></span>
          <span class="mode-label">{useRealBackend ? 'SQLite' : '100k'}</span>
        </button>
      {/if}
    </Surface>
  </header>
{:else}
  <!-- STANDARD & ADAPTIVE SINGLE BAR (§3, §4.2) -->
  <Surface
    kind="toolbar"
    class="standard-toolbar {isScrolled ? 'has-scrolled' : ''}"
  >
    <div class="toolbar-section toolbar-left">
      <Button
        variant="icon"
        title="Toggle Sidebar (⌥⌘S)"
        onclick={() => (isSidebarCollapsed = !isSidebarCollapsed)}
      >
        <Icon name="panel-left" size={17} />
      </Button>

      <div class="nav-history-buttons">
        <Button
          variant="icon"
          title="Back"
          disabled={!canGoBack}
          onclick={onback}
        >
          <Icon name="chevron-left" size={16} />
        </Button>
        <Button
          variant="icon"
          title="Forward"
          disabled={!canGoForward}
          onclick={onforward}
        >
          <Icon name="chevron-right" size={16} />
        </Button>
      </div>
    </div>

    <div class="toolbar-section toolbar-center">
      {#if currentRoute === 'library'}
        <SegmentedControl
          options={libraryViewOptions}
          bind:value={librarySubView}
        />
      {:else}
        <h1 class="standard-page-title">{pageTitle}</h1>
      {/if}
    </div>

    <div class="toolbar-section toolbar-right">
      {#if currentRoute === 'library' && librarySubView === 'all'}
        <!-- 96px Zoom Slider (§4.2) -->
        <div class="zoom-slider-wrap">
          <Icon name="zoom-out" size={13} />
          <Slider
            min={0}
            max={5}
            step={1}
            bind:value={zoomLevelIndex}
            width="80px"
          />
          <Icon name="zoom-in" size={13} />
        </div>

        <Button
          variant="icon"
          title={isFill ? 'Fill (Square crops)' : 'Fit (Aspect ratio)'}
          active={!isFill}
          onclick={() => (isFill = !isFill)}
        >
          <Icon name={isFill ? 'fill' : 'fit'} size={15} />
        </Button>

        <div class="relative-wrap">
          <Button
            variant="icon"
            title="Filter Library"
            active={filter !== 'all'}
            onclick={() => (filterMenuOpen = !filterMenuOpen)}
          >
            <Icon name="filter" size={15} />
          </Button>
          <Menu
            items={filterOptions}
            bind:open={filterMenuOpen}
            onselect={handleFilterSelect}
            style="top: 36px; right: 0;"
          />
        </div>
      {/if}

      <!-- Selection / Open photo actions (§4.2) -->
      <Button
        variant="icon"
        title="Share"
        disabled={!hasSelectionOrOpen}
        onclick={onshare}
      >
        <Icon name="share" size={15} />
      </Button>

      <Button
        variant="icon"
        title="Favorite (.)"
        disabled={!hasSelectionOrOpen}
        onclick={onfavorite}
      >
        <Icon name={isFavorite ? 'heart-filled' : 'heart'} size={15} class={isFavorite ? 'text-favorite' : ''} />
      </Button>

      <Button
        variant="icon"
        title="Rotate (⌘R)"
        disabled={!hasSelectionOrOpen}
        onclick={onrotate}
      >
        <Icon name="rotate-cw" size={15} />
      </Button>

      <Button
        variant="icon"
        title="Info (⌘I)"
        active={infoPanelOpen}
        onclick={() => (infoPanelOpen = !infoPanelOpen)}
      >
        <Icon name="info" size={15} />
      </Button>

      <!-- Expandable Search field (§4.2, §4.10) -->
      <Field
        icon="search"
        placeholder="Search"
        expandOnFocus={true}
        baseWidth="140px"
        expandedWidth="260px"
        bind:value={searchQuery}
      />

      <Button
        variant="icon"
        title="Appearance & Settings"
        onclick={onopenSettings}
      >
        <Icon name="settings" size={15} />
      </Button>

      {#if onimport}
        <Button
          variant="icon"
          title="Import Photos…"
          onclick={onimport}
        >
          <Icon name="arrow-down-to-line" size={15} />
        </Button>
      {/if}

      {#if ontogglebackend}
        <button
          type="button"
          class="backend-mode-pill {useRealBackend ? 'is-sqlite' : 'is-benchmark'}"
          title="Click to toggle between SQLite Core and 100k Benchmark fixture"
          onclick={ontogglebackend}
        >
          <span class="mode-dot"></span>
          <span class="mode-label">{useRealBackend ? 'SQLite Core' : '100k Benchmark'}</span>
        </button>
      {/if}
    </div>
  </Surface>
{/if}

<style>
  /* ---------------- GLASS CLUSTERS ---------------- */
  .glass-toolbar-container {
    position: absolute;
    top: 8px;
    left: 8px;
    right: 8px;
    height: var(--toolbar-h);
    display: flex;
    align-items: center;
    justify-content: space-between;
    pointer-events: none;
    z-index: 100;
  }

  :global(.toolbar-cluster) {
    pointer-events: auto;
    display: inline-flex;
    align-items: center;
    height: 40px;
    padding: 0 8px;
    gap: 6px;
    border-radius: var(--r-pill);
    box-sizing: border-box;
  }

  .cluster-page-title {
    font-size: var(--text-size-title);
    font-weight: 600;
    line-height: var(--text-lh-title);
    color: inherit;
    margin: 0;
    padding: 0 10px;
  }

  /* ---------------- STANDARD & ADAPTIVE SINGLE BAR ---------------- */
  :global(.standard-toolbar) {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: var(--toolbar-h);
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 16px;
    z-index: 100;
    border-bottom: 1px solid transparent;
    transition: border-color var(--dur-base) var(--ease-standard);
  }

  :global(.standard-toolbar.has-scrolled) {
    border-bottom-color: var(--separator);
  }

  .toolbar-section {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .toolbar-center {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
  }

  .standard-page-title {
    font-size: var(--text-size-title);
    font-weight: 600;
    color: var(--text-1);
    margin: 0;
  }

  .nav-history-buttons {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .zoom-slider-wrap {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-3);
  }

  .relative-wrap {
    position: relative;
  }

  :global(.text-favorite) {
    color: var(--favorite) !important;
  }

  .backend-mode-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 26px;
    padding: 0 10px;
    border-radius: var(--r-full);
    border: 1px solid var(--separator);
    background: var(--fill-1);
    color: var(--text-2);
    font-size: var(--text-size-caption);
    font-weight: 500;
    cursor: pointer;
    user-select: none;
    -webkit-user-select: none;
    transition: all var(--dur-short) var(--ease-standard);
  }

  .backend-mode-pill:hover {
    background: var(--fill-2);
    color: var(--text-1);
    border-color: var(--separator-strong);
  }

  .mode-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    transition: background var(--dur-short) var(--ease-standard);
  }

  .backend-mode-pill.is-sqlite .mode-dot {
    background: var(--accent);
    box-shadow: 0 0 6px var(--accent);
  }

  .backend-mode-pill.is-benchmark .mode-dot {
    background: var(--text-3);
  }
</style>
