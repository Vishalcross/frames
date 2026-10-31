<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';
  import { isTauriAvailable } from '$lib/services/tauriRepo';

  let {
    isMac = true,
    title = '',
    onclose,
    onminimize,
    onmaximize
  }: {
    isMac?: boolean;
    title?: string;
    onclose?: () => void;
    onminimize?: () => void;
    onmaximize?: () => void;
  } = $props();

  // Auto-detect OS platform if in browser/tauri
  let platformIsMac = $state(true);

  $effect(() => {
    if (typeof navigator !== 'undefined') {
      platformIsMac = isMac !== undefined ? isMac : navigator.platform.toUpperCase().indexOf('MAC') >= 0;
    }
  });

  async function handleWinClose() {
    if (onclose) return onclose();
    if (isTauriAvailable()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      getCurrentWindow().close();
    }
  }

  async function handleWinMinimize() {
    if (onminimize) return onminimize();
    if (isTauriAvailable()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      getCurrentWindow().minimize();
    }
  }

  async function handleWinMaximize() {
    if (onmaximize) return onmaximize();
    if (isTauriAvailable()) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      getCurrentWindow().toggleMaximize();
    }
  }
</script>

<div class="window-titlebar {platformIsMac ? 'is-mac' : 'is-win'}" data-tauri-drag-region>
  {#if platformIsMac}
    <!-- macOS Inset Traffic Lights Spacer (§2, §3):
         Native macOS traffic lights are positioned by the OS via titleBarStyle: Overlay.
         We provide an empty 78px spacer with drag region so the window stays draggable
         and no duplicate synthetic buttons are drawn over or under native buttons. -->
    <div class="mac-traffic-spacer" data-tauri-drag-region></div>
  {:else}
    <!-- Windows Title & 32px Drag Region with Custom Caption Buttons (§2) -->
    <div class="win-drag-region" data-tauri-drag-region>
      <span class="win-app-title">{title || 'Frames'}</span>
    </div>

    <div class="win-caption-buttons">
      <button
        type="button"
        class="win-caption-btn"
        aria-label="Minimize"
        onclick={handleWinMinimize}
        tabindex="-1"
      >
        <Icon name="minus" size={10} stroke={2} />
      </button>
      <button
        type="button"
        class="win-caption-btn"
        aria-label="Maximize"
        onclick={handleWinMaximize}
        tabindex="-1"
      >
        <Icon name="square" size={9} stroke={2} />
      </button>
      <button
        type="button"
        class="win-caption-btn win-close"
        aria-label="Close"
        onclick={handleWinClose}
        tabindex="-1"
      >
        <Icon name="x" size={11} stroke={2} />
      </button>
    </div>
  {/if}
</div>

<style>
  .window-titlebar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 32px;
    z-index: 1000;
    pointer-events: none;
    display: flex;
    align-items: center;
    user-select: none;
    -webkit-app-region: drag;
  }

  /* macOS Traffic Lights Spacer */
  .mac-traffic-spacer {
    width: 78px;
    height: 100%;
    pointer-events: auto;
    -webkit-app-region: drag;
  }

  /* Windows Caption Buttons */
  .win-drag-region {
    flex: 1;
    height: 32px;
    display: flex;
    align-items: center;
    padding-left: 12px;
  }

  .win-app-title {
    font-size: 12px;
    color: var(--text-2);
    font-weight: 500;
  }

  .win-caption-buttons {
    display: flex;
    align-items: center;
    height: 32px;
    pointer-events: auto;
    -webkit-app-region: no-drag;
  }

  .win-caption-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 46px;
    height: 32px;
    border: none;
    background: transparent;
    color: var(--text-2);
    cursor: pointer;
    outline: none;
  }

  .win-caption-btn:hover {
    background: var(--fill-1);
    color: var(--text-1);
  }

  .win-caption-btn.win-close:hover {
    background: #e81123;
    color: #ffffff;
  }
</style>
