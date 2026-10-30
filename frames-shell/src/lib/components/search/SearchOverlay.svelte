<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Badge from '$lib/components/surface/Badge.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import VirtualGrid from '$lib/components/grid/VirtualGrid.svelte';
  import type { Asset, Query } from '$lib/types';

  let {
    searchQuery = $bindable(''),
    activeScope = $bindable<'all' | 'dates' | 'albums' | 'videos'>('all'),
    onopenphoto,
    onclosesearch
  }: {
    searchQuery: string;
    activeScope?: 'all' | 'dates' | 'albums' | 'videos';
    onopenphoto?: (asset: Asset) => void;
    onclosesearch?: () => void;
  } = $props();

  const recentSearches = [
    'Kyoto Japan',
    'Big Sur road trip',
    'Summer 2025',
    'Videos with sound',
    'Golden retriever'
  ];

  const suggestions = [
    { type: 'date', label: 'October 2026', icon: 'calendar' },
    { type: 'album', label: 'Kyoto & Tokyo Autumn', icon: 'album' },
    { type: 'media', label: 'Videos only', icon: 'video' },
    { type: 'location', label: 'Dolomites, Italy', icon: 'compass' }
  ];

  const query: Query = $derived({
    search: searchQuery,
    filter: activeScope === 'videos' ? 'videos' : undefined
  });

  function selectRecent(s: string) {
    searchQuery = s;
  }
</script>

<div class="search-view-container">
  <!-- Search Scope Pills (§4.10) -->
  <div class="search-scope-header">
    <div class="scope-pills-row">
      <button
        type="button"
        class="scope-pill {activeScope === 'all' ? 'active' : ''}"
        onclick={() => (activeScope = 'all')}
      >
        All Results
      </button>
      <button
        type="button"
        class="scope-pill {activeScope === 'dates' ? 'active' : ''}"
        onclick={() => (activeScope = 'dates')}
      >
        Dates
      </button>
      <button
        type="button"
        class="scope-pill {activeScope === 'albums' ? 'active' : ''}"
        onclick={() => (activeScope = 'albums')}
      >
        Albums
      </button>
      <button
        type="button"
        class="scope-pill {activeScope === 'videos' ? 'active' : ''}"
        onclick={() => (activeScope = 'videos')}
      >
        Videos
      </button>
    </div>

    {#if searchQuery}
      <button type="button" class="clear-search-link" onclick={() => (searchQuery = '')}>
        Clear search
      </button>
    {/if}
  </div>

  {#if !searchQuery}
    <!-- Empty state shows recent searches (§4.10) -->
    <div class="search-empty-state">
      <div class="empty-section">
        <h3 class="empty-heading">Recent Searches</h3>
        <div class="recent-searches-list">
          {#each recentSearches as search}
            <button
              type="button"
              class="recent-search-item"
              onclick={() => selectRecent(search)}
            >
              <Icon name="search" size={14} />
              <span>{search}</span>
            </button>
          {/each}
        </div>
      </div>

      <div class="empty-section">
        <h3 class="empty-heading">Suggestions</h3>
        <div class="suggestions-grid">
          {#each suggestions as sug}
            <button
              type="button"
              class="suggestion-card"
              onclick={() => selectRecent(sug.label)}
            >
              <Icon name={sug.icon} size={16} />
              <span>{sug.label}</span>
            </button>
          {/each}
        </div>
      </div>
    </div>
  {:else}
    <!-- Full Results replace content (§4.10) -->
    <div class="search-results-grid">
      <VirtualGrid
        {query}
        {onopenphoto}
      />
    </div>
  {/if}
</div>

<style>
  .search-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    padding-top: var(--toolbar-h);
  }

  .search-scope-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 24px;
    border-bottom: 1px solid var(--separator);
    flex-shrink: 0;
  }

  .scope-pills-row {
    display: flex;
    gap: 8px;
  }

  .scope-pill {
    border: none;
    background: var(--fill-1);
    color: var(--text-2);
    font-family: var(--font-stack);
    font-size: var(--text-size-caption);
    font-weight: 500;
    padding: 4px 12px;
    border-radius: var(--r-pill);
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease-standard),
                color var(--dur-fast) var(--ease-standard);
  }
  .scope-pill:hover {
    background: var(--fill-2);
    color: var(--text-1);
  }
  .scope-pill.active {
    background: var(--accent);
    color: var(--accent-fg);
    font-weight: 600;
  }

  .clear-search-link {
    border: none;
    background: transparent;
    color: var(--accent);
    font-size: var(--text-size-caption);
    font-weight: 500;
    cursor: pointer;
  }

  /* Empty state */
  .search-empty-state {
    padding: 24px 32px;
    display: flex;
    flex-direction: column;
    gap: 28px;
    max-width: 680px;
  }

  .empty-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .empty-heading {
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: var(--text-2);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin: 0;
  }

  .recent-searches-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .recent-search-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    text-align: left;
    cursor: pointer;
  }
  .recent-search-item:hover {
    background: var(--fill-1);
  }

  .suggestions-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: 8px;
  }

  .suggestion-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-radius: var(--r-sm);
    background: var(--fill-1);
    border: none;
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    font-weight: 500;
    cursor: pointer;
  }
  .suggestion-card:hover {
    background: var(--fill-2);
  }

  .search-results-grid {
    flex: 1;
    position: relative;
  }
</style>
