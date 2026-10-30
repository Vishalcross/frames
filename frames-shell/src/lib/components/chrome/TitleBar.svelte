<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';

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
</script>

<div class="window-titlebar {platformIsMac ? 'is-mac' : 'is-win'}" data-tauri-drag-region>
  {#if platformIsMac}
    <!-- macOS Inset Traffic Lights (§2, §3) -->
    <div class="mac-traffic-lights" aria-label="Window controls">
      <button
        type="button"
        class="traffic-light close"
        aria-label="Close"
        onclick={onclose}
        tabindex="-1"
      ></button>
      <button
        type="button"
        class="traffic-light minimize"
        aria-label="Minimize"
        onclick={onminimize}
        tabindex="-1"
      ></button>
      <button
        type="button"
        class="traffic-light zoom"
        aria-label="Zoom"
        onclick={onmaximize}
        tabindex="-1"
      ></button>
    </div>
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
        onclick={onminimize}
        tabindex="-1"
      >
        <Icon name="minus" size={10} stroke={2} />
      </button>
      <button
        type="button"
        class="win-caption-btn"
        aria-label="Maximize"
        onclick={onmaximize}
        tabindex="-1"
      >
        <Icon name="square" size={9} stroke={2} />
      </button>
      <button
        type="button"
        class="win-caption-btn win-close"
        aria-label="Close"
        onclick={onclose}
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

  /* macOS Traffic Lights */
  .mac-traffic-lights {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-left: 14px;
    height: 100%;
    pointer-events: auto;
    -webkit-app-region: no-drag;
  }

  .traffic-light {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 0.5px solid rgba(0, 0, 0, 0.15);
    padding: 0;
    cursor: pointer;
    outline: none;
    transition: filter var(--dur-fast) var(--ease-standard);
  }

  .traffic-light.close {
    background: #ff5f56;
  }
  .traffic-light.minimize {
    background: #ffbd2e;
  }
  .traffic-light.zoom {
    background: #27c93f;
  }

  .traffic-light:hover {
    filter: brightness(0.9);
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
