<script lang="ts">
  import { onMount } from 'svelte';
  import '$lib/theme/tokens.css';
  import '$lib/theme/themes/standard.css';
  import '$lib/theme/themes/adaptive.css';
  import '$lib/theme/themes/glass.css';

  import TitleBar from '$lib/components/chrome/TitleBar.svelte';
  import Sidebar from '$lib/components/sidebar/Sidebar.svelte';
  import Toolbar from '$lib/components/toolbar/Toolbar.svelte';
  import VirtualGrid from '$lib/components/grid/VirtualGrid.svelte';
  import YearsView from '$lib/components/library/YearsView.svelte';
  import MonthsView from '$lib/components/library/MonthsView.svelte';
  import CollectionsView from '$lib/components/collections/CollectionsView.svelte';
  import AlbumDetailView from '$lib/components/collections/AlbumDetailView.svelte';
  import PeoplePetsView from '$lib/components/collections/PeoplePetsView.svelte';
  import MemoriesView from '$lib/components/collections/MemoriesView.svelte';
  import TripsView from '$lib/components/collections/TripsView.svelte';
  import TrashView from '$lib/components/library/TrashView.svelte';
  import EmptyLibraryView from '$lib/components/library/EmptyLibraryView.svelte';
  import SearchOverlay from '$lib/components/search/SearchOverlay.svelte';
  import SelectionBar from '$lib/components/surface/SelectionBar.svelte';
  import PhotoViewer from '$lib/components/viewer/PhotoViewer.svelte';
  import InfoPanel from '$lib/components/info/InfoPanel.svelte';
  import EditModal from '$lib/components/edit/EditModal.svelte';
  import AppearancePane from '$lib/components/settings/AppearancePane.svelte';
  import ConnectionPane from '$lib/components/settings/ConnectionPane.svelte';
  import KitGallery from '$lib/components/settings/KitGallery.svelte';
  import DeleteSheet from '$lib/components/dialogs/DeleteSheet.svelte';
  import ImportOverlay from '$lib/components/dialogs/ImportOverlay.svelte';
  import Toast, { type ToastMessage } from '$lib/components/dialogs/Toast.svelte';

  import LensFilterSvg from '$lib/components/effects/LensFilterSvg.svelte';
  import AmbientBackdrop from '$lib/components/effects/AmbientBackdrop.svelte';

  import { theme, initThemeEngine, cycleAppearance } from '$lib/theme/engine.svelte';
  import { startPerformanceGovernor } from '$lib/theme/glass';
  import { tauriRepo, isTauriAvailable } from '$lib/services/tauriRepo';
  import { initAdaptiveListeners, handleViewedPhotoChange } from '$lib/adaptive';
  import { invoke } from '@tauri-apps/api/core';
  import type { AlbumItem, Asset, Query } from '$lib/types';

  // Real Backend State
  const isTauri = isTauriAvailable();
  let gridKey = $state(0);
  let albums = $state<AlbumItem[]>([]);
  let isSyncing = $state(false);
  let outboxPending = $state(0);
  let isOffline = $state(false);
  const activeRepo = tauriRepo;

  // Navigation & View Routing State
  let currentRoute = $state('library');
  let currentAlbumId = $state('');
  let librarySubView = $state<'years' | 'months' | 'all'>('all');
  let selectedYear = $state(2026);
  let selectedMonth = $state(10);
  let navigationHistory = $state<{ route: string; albumId: string; subView: 'years' | 'months' | 'all' }[]>([]);
  let historyIndex = $state(0);

  // Layout & Chrome State
  let sidebarWidth = $state(240);
  let isSidebarCollapsed = $state(false);
  let isScrolled = $state(false);
  let infoPanelOpen = $state(false);
  let settingsOpen = $state(false);
  let galleryOpen = $state(false);
  let connectionOpen = $state(false);
  let connectionHint = $state('');
  let deleteSheetOpen = $state(false);
  let importDragging = $state(false);

  // Search & Filters State
  let searchQuery = $state('');
  let activeFilter = $state<'all' | 'favorites' | 'videos' | 'photos'>('all');
  let zoomLevelIndex = $state(2); // 0..5
  let isFill = $state(true); // Fill (square crops) vs Fit (justified rows)

  // Selection & Active Item State
  let selectedIds = $state<Set<string>>(new Set());
  let activeViewerAsset = $state<Asset | null>(null);
  let viewerOpen = $state(false);
  let editModalOpen = $state(false);

  // Feedback State
  let toasts = $state<ToastMessage[]>([]);

  // Active Album lookup
  const activeAlbum = $derived.by<AlbumItem>(() => {
    const found = albums.find((a) => a.id === currentAlbumId);
    if (found) return found;
    return {
      id: currentAlbumId,
      title: 'Album',
      assetCount: 0,
      keyAssetId: '',
      dateRange: ''
    };
  });

  // Page title for Collections pages (§4.2)
  const pageTitle = $derived.by(() => {
    if (currentRoute === 'library') return 'Library';
    if (currentRoute === 'days') return 'Days';
    if (currentRoute === 'memories') return 'Memories';
    if (currentRoute === 'people-pets') return 'People & Pets';
    if (currentRoute === 'trips') return 'Trips';
    if (currentRoute === 'collections') return 'Albums';
    if (currentRoute === 'album-detail') return activeAlbum.title;
    if (currentRoute === 'media-types') return 'Media Types';
    if (currentRoute === 'imports') return 'Imports';
    if (currentRoute === 'duplicates') return 'Duplicates';
    if (currentRoute === 'hidden') return 'Hidden';
    if (currentRoute === 'trash') return 'Recently Deleted';
    if (currentRoute === 'captured-by-me') return 'Captured by Me';
    return 'Frames';
  });

  // Query generation based on current route
  const currentQuery = $derived.by<Query>(() => {
    const q: Query = {
      filter: activeFilter,
      search: searchQuery || undefined
    };

    if (currentRoute === 'library') {
      q.scope = 'all';
    } else if (currentRoute === 'favorites') {
      q.scope = 'favorites';
    } else if (currentRoute === 'videos') {
      q.scope = 'videos';
    } else if (currentRoute === 'hidden') {
      q.scope = 'hidden';
    } else if (currentRoute === 'trash') {
      q.scope = 'trash';
    } else if (currentRoute === 'captured-by-me') {
      q.scope = 'captured-by-me';
    } else if (currentRoute === 'album-detail') {
      q.scope = 'album';
      q.albumId = currentAlbumId;
    }
    return q;
  });

  const hasSelectionOrOpen = $derived(selectedIds.size > 0 || viewerOpen);

  const isFavoriteActive = $derived.by(() => {
    if (viewerOpen && activeViewerAsset) return activeViewerAsset.favorite;
    if (selectedIds.size === 1) {
      const firstId = Array.from(selectedIds)[0];
      const found = tauriRepo.getAssets().find((a) => a.id === firstId);
      return found ? found.favorite : false;
    }
    return false;
  });

  async function loadLibraryData() {
    await tauriRepo.loadBootstrap();
    albums = tauriRepo.getAlbums();
    outboxPending = tauriRepo.outboxPending;
    isOffline = tauriRepo.offline;
    gridKey++;
  }

  async function handleSyncNow() {
    if (tauriRepo.settings && !tauriRepo.settings.s4_configured) {
      openConnection('Add your MEGA S4 Access Key ID and Secret Access Key to sync your albums.');
      return;
    }
    isSyncing = true;
    toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: 'Syncing with MEGA S4 cloud…' }];
    const rep = await tauriRepo.syncNow();
    isSyncing = false;
    await loadLibraryData();
    if (rep?.error) {
      toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: `Sync status: ${rep.error}` }];
    } else if (rep) {
      toasts = [
        ...toasts,
        {
          id: Date.now().toString(),
          type: 'info',
          text: `Synced with MEGA S4 (${rep.batches_pulled ?? 0} pulled, ${rep.batches_pushed ?? 0} pushed)`
        }
      ];
    }
  }

  onMount(() => {
    const cleanupEngine = initThemeEngine();
    const cleanupGovernor = startPerformanceGovernor();
    initAdaptiveListeners();

    // Initial navigation entry
    navigationHistory.push({ route: 'library', albumId: '', subView: 'all' });

    // Initialise real SQLite library and sync down any uploaded album from Mega S4
    loadLibraryData().then(() => {
      if (tauriRepo.settings && !tauriRepo.settings.s4_configured) {
        openConnection('Welcome! Connect your MEGA S4 bucket to load your albums and import photos.');
      } else {
        handleSyncNow();
      }
    });

    // Drag and drop listeners for OS files (§4.12)
    const handleDragEnter = (e: DragEvent) => {
      e.preventDefault();
      importDragging = true;
    };

    let unlistens: Array<() => void> = [];
    if (isTauri) {
      import('@tauri-apps/api/event').then(({ listen }) => {
        listen('album-synced', async () => {
          await loadLibraryData();
        }).then((un) => unlistens.push(un));

        listen('scan-finished', async () => {
          await loadLibraryData();
        }).then((un) => unlistens.push(un));
      });
    }

    return () => {
      cleanupEngine();
      cleanupGovernor();
      window.removeEventListener('dragenter', handleDragEnter);
      for (const un of unlistens) {
        un();
      }
    };
  });

  // Route navigation helper
  function navigate(route: string, extraId = '') {
    navigationHistory.push({ route, albumId: extraId, subView: librarySubView });
    historyIndex = navigationHistory.length - 1;
    currentRoute = route;
    currentAlbumId = extraId;
    selectedIds = new Set();
    handleViewedPhotoChange(null, route === 'album-detail' ? extraId : null);
  }

  function goBack() {
    if (historyIndex > 0) {
      historyIndex--;
      const target = navigationHistory[historyIndex];
      currentRoute = target.route;
      currentAlbumId = target.albumId;
      librarySubView = target.subView;
      handleViewedPhotoChange(null, target.route === 'album-detail' ? target.albumId : null);
    }
  }

  function goForward() {
    if (historyIndex < navigationHistory.length - 1) {
      historyIndex++;
      const target = navigationHistory[historyIndex];
      currentRoute = target.route;
      currentAlbumId = target.albumId;
      librarySubView = target.subView;
      handleViewedPhotoChange(null, target.route === 'album-detail' ? target.albumId : null);
    }
  }

  // Drill-down handlers (§4.3)
  function handleDrillYear(year: number) {
    selectedYear = year;
    librarySubView = 'months';
  }

  function handleDrillMonth(month: number) {
    selectedMonth = month;
    librarySubView = 'all';
  }

  // Photo viewer open/close
  function openPhoto(asset: Asset) {
    activeViewerAsset = asset;
    viewerOpen = true;
  }

  function closePhoto() {
    viewerOpen = false;
    handleViewedPhotoChange(null, currentRoute === 'album-detail' ? currentAlbumId : null);
  }

  // Edit mode
  function openEdit(asset: Asset) {
    activeViewerAsset = asset;
    editModalOpen = true;
  }

  // Selection actions (§4.2, §4.5)
  function handleShare() {
    toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: 'Sharing sheet opened' }];
  }

  function handleFavorite() {
    if (viewerOpen && activeViewerAsset) {
      activeViewerAsset.favorite = !activeViewerAsset.favorite;
      activeRepo.updateFavorite(activeViewerAsset.id, activeViewerAsset.favorite);
    } else {
      selectedIds.forEach((id) => {
        activeRepo.updateFavorite(id, true);
      });
      toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: `Added ${selectedIds.size} items to Favorites` }];
    }
  }

  function handleRotate() {
    toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: 'Rotated 90° clockwise' }];
  }

  function handleDeletePrompt() {
    deleteSheetOpen = true;
  }

  function handleDeleteConfirmed() {
    selectedIds.forEach((id) => {
      activeRepo.updateDeleted(id, true);
    });
    toasts = [
      ...toasts,
      { id: Date.now().toString(), type: 'info', text: `Moved ${selectedIds.size} items to Recently Deleted` }
    ];
    selectedIds = new Set();
    gridKey++;
  }

  function openConnection(hint = '') {
    connectionHint = hint;
    settingsOpen = false;
    connectionOpen = true;
  }

  // Real Backend Bridge Handlers (M6)
  async function handlePickFolderAndImport() {
    if (!isTauri) {
      toasts = [
        ...toasts,
        {
          id: Date.now().toString(),
          type: 'info',
          text: 'Running in browser dev server. To access native folder dialog and SQLite, launch via "npm run tauri dev".'
        }
      ];
      return;
    }
    if (tauriRepo.settings && !tauriRepo.settings.s4_configured) {
      openConnection('Importing uploads your photos to MEGA S4, so add your S4 keys first.');
      return;
    }
    toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: 'Opening system folder dialog…' }];
    const res = await tauriRepo.pickFolderAndImport();
    if (res.error) {
      toasts = [...toasts, { id: Date.now().toString(), type: 'error', text: res.error }];
    } else if (res.cancelled) {
      // user dismissed the folder dialog
    } else {
      await loadLibraryData();
      toasts = [
        ...toasts,
        {
          id: Date.now().toString(),
          type: res.count > 0 ? 'info' : 'warning',
          text: `Imported ${res.count} item(s)${res.note ? ` — ${res.note}` : ''}`
        }
      ];
    }
  }

  async function handleSeedDemo() {
    if (!isTauri) {
      toasts = [
        ...toasts,
        {
          id: Date.now().toString(),
          type: 'info',
          text: 'Seeding SQLite store requires running inside the Tauri 2 desktop window.'
        }
      ];
      return;
    }
    toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: 'Writing sample records & WebP thumbnails to SQLite…' }];
    const count = await tauriRepo.seedDemoLibrary();
    await loadLibraryData();
    toasts = [
      ...toasts,
      { id: Date.now().toString(), type: 'info', text: `Seeded ${count} items with real EXIF & thumbnails into SQLite!` }
    ];
  }

  // Global Keyboard Shortcuts (§12)
  function handleGlobalKeyDown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.altKey && e.key.toLowerCase() === 's') {
      e.preventDefault();
      // ⌥⌘S sidebar toggle
      isSidebarCollapsed = !isSidebarCollapsed;
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'i') {
      e.preventDefault();
      // ⌘I info panel toggle
      infoPanelOpen = !infoPanelOpen;
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      // ⌘F focus search
      searchQuery = ' ';
      setTimeout(() => (searchQuery = ''), 10);
    } else if ((e.metaKey || e.ctrlKey) && (e.key === '=' || e.key === '+')) {
      e.preventDefault();
      // ⌘+ zoom in
      zoomLevelIndex = Math.min(5, zoomLevelIndex + 1);
    } else if ((e.metaKey || e.ctrlKey) && e.key === '-') {
      e.preventDefault();
      // ⌘- zoom out
      zoomLevelIndex = Math.max(0, zoomLevelIndex - 1);
    } else if (e.key === '.' && hasSelectionOrOpen) {
      e.preventDefault();
      handleFavorite();
    } else if ((e.metaKey || e.ctrlKey) && e.key === 'Backspace' && selectedIds.size > 0) {
      e.preventDefault();
      handleDeletePrompt();
    }
  }
</script>

<svelte:window onkeydown={handleGlobalKeyDown} />

<!-- SVG Filter definitions for Liquid Glass Tier 2 (§8.3) -->
<LensFilterSvg />

<!-- Adaptive Ambient Backdrop (§7.5) -->
<AmbientBackdrop />

<div class="frames-application appearance-{theme.appearance}" data-appearance={theme.appearance}>
  <!-- TitleBar (macOS Traffic Lights or Windows Caption buttons, §2, §3) -->
  <TitleBar />

  <div class="frames-window-layout">
    <!-- Sidebar (240px, docked or floating glass panel, §3, §4.1) -->
    <Sidebar
      bind:currentRoute
      bind:currentAlbumId
      bind:sidebarWidth
      bind:isCollapsed={isSidebarCollapsed}
      {albums}
      {isSyncing}
      {outboxPending}
      onnavigate={navigate}
      onsync={handleSyncNow}
    />

    <!-- Main Content Area -->
    <main class="frames-main-stage">
      <!-- Toolbar (52px macOS, 48px Win; single bar or floating glass clusters, §3, §4.2) -->
      <Toolbar
        {currentRoute}
        bind:librarySubView
        {pageTitle}
        canGoBack={historyIndex > 0}
        canGoForward={historyIndex < navigationHistory.length - 1}
        bind:zoomLevelIndex
        bind:isFill
        bind:filter={activeFilter}
        bind:searchQuery
        {hasSelectionOrOpen}
        isFavorite={isFavoriteActive}
        bind:infoPanelOpen
        bind:isSidebarCollapsed
        {isScrolled}
        onback={goBack}
        onforward={goForward}
        onshare={handleShare}
        onfavorite={handleFavorite}
        onrotate={handleRotate}
        onopenSettings={() => (settingsOpen = true)}
        onimport={handlePickFolderAndImport}
        onsync={handleSyncNow}
      />

      <!-- Scroll edge blur effect (§8.1 #3) -->
      <div class="scroll-edge-fade"></div>

      <!-- Route Content Router -->
      <div class="route-viewport" onscroll={() => (isScrolled = true)}>
        {#if searchQuery}
          <!-- Search Results View (§4.10) -->
          <SearchOverlay
            bind:searchQuery
            onopenphoto={openPhoto}
            onclosesearch={() => (searchQuery = '')}
          />
        {:else if currentRoute === 'library'}
          {#if librarySubView === 'years'}
            <!-- Years Cards 16:10 (§4.3) -->
            <YearsView ondrillyear={handleDrillYear} />
          {:else if librarySubView === 'months'}
            <!-- Months Mosaic 1 large + 3 small (§4.3) -->
            <MonthsView year={selectedYear} ondrillmonth={handleDrillMonth} />
          {:else}
            <!-- All Continuous Virtualized Grid (§4.3, §4.4) -->
            {#key gridKey}
              <VirtualGrid
                repo={activeRepo}
                query={currentQuery}
                bind:zoomLevelIndex
                bind:isFill
                bind:selectedIds
                onopenphoto={openPhoto}
              />
            {/key}
          {/if}
        {:else if currentRoute === 'collections'}
          <!-- Collections Landing Rails (§4.9) -->
          <CollectionsView
            {albums}
            onnavigatealbum={(id) => navigate('album-detail', id)}
            onnavigateroute={(r) => navigate(r)}
          />
        {:else if currentRoute === 'album-detail'}
          <!-- Album Detail with 0.4 Parallax Ambient Hero (§4.9) -->
          <AlbumDetailView
            repo={activeRepo}
            album={activeAlbum}
            bind:zoomLevelIndex
            bind:isFill
            bind:selectedIds
            onopenphoto={openPhoto}
          />
        {:else if currentRoute === 'people-pets'}
          <PeoplePetsView onselectperson={() => navigate('library')} />
        {:else if currentRoute === 'memories'}
          <MemoriesView onselectmemory={() => navigate('library')} />
        {:else if currentRoute === 'trips'}
          <TripsView onselecttrip={() => navigate('library')} />
        {:else if currentRoute === 'trash'}
          <!-- Recently Deleted with days-remaining labels (§4.12) -->
          <TrashView />
        {:else if currentRoute === 'imports'}
          <EmptyLibraryView
            {isTauri}
            onimport={handlePickFolderAndImport}
            onsync={handleSyncNow}
            onseed={handleSeedDemo}
          />
        {:else}
          <!-- Generic Filtered Grid for Favorites, Videos, Hidden, Captured by Me -->
          {#key gridKey}
            <VirtualGrid
              repo={activeRepo}
              query={currentQuery}
              bind:zoomLevelIndex
              bind:isFill
              bind:selectedIds
              onopenphoto={openPhoto}
            />
          {/key}
        {/if}
      </div>

      <!-- Floating Selection Action Bar (§4.5) -->
      <SelectionBar
        selectedCount={selectedIds.size}
        onshare={handleShare}
        onfavorite={handleFavorite}
        onaddtoalbum={() => handleShare()}
        ondelete={handleDeletePrompt}
        onclear={() => (selectedIds = new Set())}
      />
    </main>

    <!-- Info Panel (320px, pushes in Standard/Adaptive, floats in Glass, §4.7) -->
    {#if infoPanelOpen}
      {@const sampleAsset = activeViewerAsset || tauriRepo.getAssets()[0]}
      {#if sampleAsset}
        <InfoPanel
          asset={sampleAsset}
          bind:open={infoPanelOpen}
          onclose={() => (infoPanelOpen = false)}
        />
      {/if}
    {/if}
  </div>

  <!-- Photo Viewer (Full-window modal with shared element zoom, §4.6) -->
  {#if viewerOpen && activeViewerAsset}
    <PhotoViewer
      bind:asset={activeViewerAsset}
      bind:open={viewerOpen}
      albumId={currentRoute === 'album-detail' ? currentAlbumId : null}
      onclose={closePhoto}
      onedit={(a) => openEdit(a)}
      oninfo={() => (infoPanelOpen = true)}
    />
  {/if}

  <!-- Edit Mode (Full-window modal, Light, Color, B&W, §4.8) -->
  {#if editModalOpen && activeViewerAsset}
    <EditModal
      asset={activeViewerAsset}
      bind:open={editModalOpen}
      oncancel={() => (editModalOpen = false)}
      ondone={() => (editModalOpen = false)}
    />
  {/if}

  <!-- Settings → Appearance Pane (§4.11) -->
  {#if settingsOpen}
    <AppearancePane
      onclose={() => (settingsOpen = false)}
      onopenConnection={() => openConnection()}
      ontoggleGallery={() => {
        settingsOpen = false;
        galleryOpen = true;
      }}
    />
  {/if}

  {#if connectionOpen}
    <ConnectionPane
      hint={connectionHint}
      onclose={() => (connectionOpen = false)}
      onsaved={async (view) => {
        connectionOpen = false;
        toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: 'Connection saved.' }];
        if (view.s4_configured) {
          await handleSyncNow();
        }
      }}
    />
  {/if}

  <!-- Component Kit Gallery (§13 M0) -->
  {#if galleryOpen}
    <KitGallery onclose={() => (galleryOpen = false)} />
  {/if}

  <!-- Delete Confirmation Sheet (§4.12) -->
  <DeleteSheet
    bind:open={deleteSheetOpen}
    count={selectedIds.size}
    onconfirm={handleDeleteConfirmed}
  />

  <!-- OS Drag-and-Drop Import Overlay (§4.12) -->
  <ImportOverlay
    bind:isDragging={importDragging}
    ondropfiles={async (files) => {
      const paths: string[] = [];
      for (let i = 0; i < files.length; i++) {
        const p = (files[i] as any).path;
        if (p) paths.push(p);
      }
      if (paths.length > 0 && isTauri) {
        toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: `Importing ${paths.length} items into SQLite…` }];
        try {
          const res = await invoke<any>('import_from_paths', { paths, albumId: null });
          await loadLibraryData();
          toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: `Successfully imported ${res.imported_count || paths.length} items!` }];
        } catch (e: any) {
          toasts = [...toasts, { id: Date.now().toString(), type: 'error', text: `Import failed: ${e?.toString() || e}` }];
        }
      } else {
        toasts = [...toasts, { id: Date.now().toString(), type: 'info', text: `Received ${files.length} items for import.` }];
      }
    }}
  />

  <!-- Toast Stack & Offline Chip (§4.12) -->
  <Toast bind:toasts {isOffline} />
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
    overflow: hidden;
    font-family: var(--font-stack);
    background: var(--bg-canvas);
    color: var(--text-1);
    -webkit-font-smoothing: antialiased;
    user-select: none;
    -webkit-user-select: none;
  }

  .frames-application {
    position: relative;
    width: 100vw;
    height: 100vh;
    min-width: 900px;
    min-height: 600px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    background: var(--bg-canvas);
    color: var(--text-1);
    transition: background var(--dur-ambient) var(--ease-standard);
  }

  .frames-window-layout {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    overflow: hidden;
    z-index: 10;
  }

  .frames-main-stage {
    position: relative;
    flex: 1;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: transparent;
  }

  .route-viewport {
    position: relative;
    flex: 1;
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
  }
</style>
