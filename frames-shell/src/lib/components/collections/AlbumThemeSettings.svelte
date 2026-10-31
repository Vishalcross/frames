<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {
    resolvePalette,
    applyPalette,
    type BackendAlbumTheme
  } from '$lib/adaptive';

  let {
    albumId,
    onchange,
    close
  }: {
    albumId: string;
    onchange?: (theme: BackendAlbumTheme) => void;
    close?: () => void;
  } = $props();

  let mode = $state<'dynamic' | 'album'>('dynamic');
  let source = $state<'auto' | 'manual'>('auto');

  let autoBg = $state('#141416');
  let autoSidebar = $state('#1E1E22');
  let autoAccent = $state('#0A84FF');

  let manualBg = $state('#1A2B3C');
  let manualSidebar = $state('#2B3C4D');
  let manualAccent = $state('#FF5500');

  let isRegenerating = $state(false);
  let hexErrors = $state({
    bg: false,
    sidebar: false,
    accent: false
  });

  const hexRegex = /^#[0-9A-Fa-f]{6}$/;

  function validateHex(val: string): boolean {
    return hexRegex.test(val);
  }

  function relativeLuminance(hex: string): number {
    const clean = hex.startsWith('#') ? hex.slice(1) : hex;
    if (clean.length !== 6) return 0;
    const r = parseInt(clean.slice(0, 2), 16) / 255;
    const g = parseInt(clean.slice(2, 4), 16) / 255;
    const b = parseInt(clean.slice(4, 6), 16) / 255;
    const toLinear = (c: number) => (c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4));
    return 0.2126 * toLinear(r) + 0.7152 * toLinear(g) + 0.0722 * toLinear(b);
  }

  function contrastRatio(l1: number, l2: number): number {
    const light = Math.max(l1, l2);
    const dark = Math.min(l1, l2);
    return (light + 0.05) / (dark + 0.05);
  }

  function hasLowContrast(hex: string): boolean {
    if (!validateHex(hex)) return false;
    const L = relativeLuminance(hex);
    const crWhite = contrastRatio(1.0, L);
    const crBlack = contrastRatio(L, 0.0);
    return Math.max(crWhite, crBlack) < 4.5;
  }

  const showContrastWarning = $derived(
    mode === 'album' && source === 'manual' && (hasLowContrast(manualBg) || hasLowContrast(manualAccent))
  );

  onMount(() => {
    let unlisten: (() => void) | undefined;

    // Load initial album theme from backend
    invoke<BackendAlbumTheme>('get_album_theme', { albumId })
      .then((theme) => {
        if (!theme) return;
        mode = theme.mode || 'dynamic';
        source = theme.source || 'auto';

        if (theme.auto_bg) autoBg = theme.auto_bg;
        if (theme.auto_sidebar) autoSidebar = theme.auto_sidebar;
        if (theme.auto_accent) autoAccent = theme.auto_accent;

        if (theme.bg) manualBg = theme.bg;
        if (theme.sidebar) manualSidebar = theme.sidebar;
        if (theme.accent) manualAccent = theme.accent;
      })
      .catch((err) => {
        console.warn('Failed to load album theme:', err);
      });

    // Listen for album palette ready event
    import('@tauri-apps/api/event').then(({ listen }) => {
      listen<{ album_id: string }>('album-palette-ready', async (ev) => {
        if (ev.payload?.album_id === albumId) {
          isRegenerating = false;
          const updated = await invoke<BackendAlbumTheme>('get_album_theme', { albumId });
          if (updated) {
            if (updated.auto_bg) autoBg = updated.auto_bg;
            if (updated.auto_sidebar) autoSidebar = updated.auto_sidebar;
            if (updated.auto_accent) autoAccent = updated.auto_accent;

            if (mode === 'album' && source === 'auto') {
              const pal = await resolvePalette(null, albumId);
              if (pal) applyPalette(pal);
            }
          }
        }
      }).then((un) => {
        unlisten = un;
      });
    });

    return () => {
      if (unlisten) unlisten();
    };
  });

  async function save(newMode = mode, newSource = source) {
    mode = newMode;
    source = newSource;

    if (newSource === 'manual') {
      hexErrors.bg = !validateHex(manualBg);
      hexErrors.sidebar = !validateHex(manualSidebar);
      hexErrors.accent = !validateHex(manualAccent);

      // Reject invalid format
      if (hexErrors.bg || hexErrors.sidebar || hexErrors.accent) {
        return;
      }
    }

    try {
      const saved = await invoke<BackendAlbumTheme>('set_album_theme', {
        albumId,
        mode: newMode,
        source: newSource,
        bg: newSource === 'manual' ? manualBg : undefined,
        sidebar: newSource === 'manual' ? manualSidebar : undefined,
        accent: newSource === 'manual' ? manualAccent : undefined
      });

      if (saved) {
        // Re-apply palette at once if the album is open
        const pal = await resolvePalette(null, albumId);
        if (pal) {
          applyPalette(pal);
        }
        onchange?.(saved);
      }
    } catch (err) {
      console.warn('Failed to set album theme:', err);
    }
  }

  function handleManualColorInput(type: 'bg' | 'sidebar' | 'accent', value: string) {
    if (type === 'bg') manualBg = value;
    if (type === 'sidebar') manualSidebar = value;
    if (type === 'accent') manualAccent = value;
    save(mode, source);
  }

  async function handleRegenerate() {
    isRegenerating = true;
    try {
      await invoke('regenerate_album_palette', { albumId });
    } catch (err) {
      console.warn('Failed to regenerate album palette:', err);
      isRegenerating = false;
    }
  }
</script>

<div class="album-theme-settings">
  <div class="settings-header">
    <h3 class="section-title">Colour theme</h3>
    {#if close}
      <button type="button" class="close-btn" onclick={close} aria-label="Close">✕</button>
    {/if}
  </div>

  <!-- Segmented control: "Dynamic per photo" vs "One theme for this album" (Task 4-i #1) -->
  <div class="segmented-control" role="tablist">
    <button
      type="button"
      role="tab"
      aria-selected={mode === 'dynamic'}
      class="segmented-btn"
      class:active={mode === 'dynamic'}
      onclick={() => save('dynamic', source)}
    >
      Dynamic per photo
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={mode === 'album'}
      class="segmented-btn"
      class:active={mode === 'album'}
      onclick={() => save('album', source)}
    >
      One theme for this album
    </button>
  </div>

  <!-- If "One theme for this album" (Task 4-i #2) -->
  {#if mode === 'album'}
    <!-- Radio: "Generate from this album's photos" vs "Choose 3 colours" -->
    <div class="radio-group" role="radiogroup" aria-label="Album palette source">
      <label class="radio-label">
        <input
          type="radio"
          name="album-palette-source"
          value="auto"
          checked={source === 'auto'}
          onchange={() => save('album', 'auto')}
        />
        <span>Generate from this album's photos</span>
      </label>
      <label class="radio-label">
        <input
          type="radio"
          name="album-palette-source"
          value="manual"
          checked={source === 'manual'}
          onchange={() => save('album', 'manual')}
        />
        <span>Choose 3 colours</span>
      </label>
    </div>

    <!-- If "Auto": Show the 3 derived colour swatches (read-only) + Regenerate button + spinner -->
    {#if source === 'auto'}
      <div class="palette-swatches-section">
        <div class="swatches-row">
          <div class="swatch-item">
            <div class="swatch-preview" style="background-color: {autoBg};"></div>
            <span class="swatch-label">Background</span>
            <span class="swatch-hex">{autoBg}</span>
          </div>

          <div class="swatch-item">
            <div class="swatch-preview" style="background-color: {autoSidebar};"></div>
            <span class="swatch-label">Sidebar</span>
            <span class="swatch-hex">{autoSidebar}</span>
          </div>

          <div class="swatch-item">
            <div class="swatch-preview" style="background-color: {autoAccent};"></div>
            <span class="swatch-label">Accent</span>
            <span class="swatch-hex">{autoAccent}</span>
          </div>
        </div>

        <div class="actions-row">
          <button
            type="button"
            class="regenerate-btn"
            disabled={isRegenerating}
            onclick={handleRegenerate}
          >
            {#if isRegenerating}
              <span class="spinner" aria-hidden="true"></span>
              Deriving…
            {:else}
              Regenerate
            {/if}
          </button>
        </div>
      </div>
    {/if}

    <!-- If "Manual": Three colour pickers: Background, Sidebar, Accent -->
    {#if source === 'manual'}
      <div class="palette-swatches-section manual-section">
        <div class="manual-color-row">
          <div class="manual-color-item">
            <label for="color-bg" class="swatch-label">Background</label>
            <div class="picker-group">
              <input
                id="color-bg"
                type="color"
                class="color-picker-input"
                value={validateHex(manualBg) ? manualBg.toLowerCase() : '#000000'}
                oninput={(e) => handleManualColorInput('bg', (e.target as HTMLInputElement).value)}
              />
              <input
                type="text"
                class="hex-text-input"
                class:has-error={hexErrors.bg}
                value={manualBg}
                maxlength="7"
                placeholder="#1A2B3C"
                oninput={(e) => handleManualColorInput('bg', (e.target as HTMLInputElement).value)}
              />
            </div>
          </div>

          <div class="manual-color-item">
            <label for="color-sidebar" class="swatch-label">Sidebar</label>
            <div class="picker-group">
              <input
                id="color-sidebar"
                type="color"
                class="color-picker-input"
                value={validateHex(manualSidebar) ? manualSidebar.toLowerCase() : '#000000'}
                oninput={(e) => handleManualColorInput('sidebar', (e.target as HTMLInputElement).value)}
              />
              <input
                type="text"
                class="hex-text-input"
                class:has-error={hexErrors.sidebar}
                value={manualSidebar}
                maxlength="7"
                placeholder="#2B3C4D"
                oninput={(e) => handleManualColorInput('sidebar', (e.target as HTMLInputElement).value)}
              />
            </div>
          </div>

          <div class="manual-color-item">
            <label for="color-accent" class="swatch-label">Accent</label>
            <div class="picker-group">
              <input
                id="color-accent"
                type="color"
                class="color-picker-input"
                value={validateHex(manualAccent) ? manualAccent.toLowerCase() : '#000000'}
                oninput={(e) => handleManualColorInput('accent', (e.target as HTMLInputElement).value)}
              />
              <input
                type="text"
                class="hex-text-input"
                class:has-error={hexErrors.accent}
                value={manualAccent}
                maxlength="7"
                placeholder="#0A84FF"
                oninput={(e) => handleManualColorInput('accent', (e.target as HTMLInputElement).value)}
              />
            </div>
          </div>
        </div>

        <!-- Warning badge if low text contrast -->
        {#if showContrastWarning}
          <div class="warning-badge" role="alert">
            <span class="warning-icon">⚠️</span>
            <span>Low text contrast</span>
          </div>
        {/if}
      </div>
    {/if}
  {/if}

  <!-- Hint (Task 4-i #6) -->
  <p class="theme-hint">Applies when the Adaptive theme is on.</p>
</div>

<style>
  .album-theme-settings {
    background: var(--surface-overlay, #252528);
    border: 1px solid var(--separator, rgba(255, 255, 255, 0.12));
    border-radius: var(--r-md, 12px);
    padding: 16px;
    width: 320px;
    box-sizing: border-box;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
    color: var(--text-1, #ffffff);
    font-family: var(--font, system-ui, sans-serif);
    font-size: 13px;
  }

  .settings-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }

  .section-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-1, #ffffff);
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-3, rgba(255, 255, 255, 0.5));
    font-size: 14px;
    cursor: pointer;
    padding: 4px 6px;
    border-radius: 4px;
  }

  .close-btn:hover {
    color: var(--text-1, #ffffff);
    background: rgba(255, 255, 255, 0.1);
  }

  .segmented-control {
    display: flex;
    gap: 4px;
    background: rgba(0, 0, 0, 0.25);
    border-radius: 8px;
    padding: 3px;
    margin-bottom: 12px;
  }

  .segmented-btn {
    flex: 1;
    background: transparent;
    border: none;
    border-radius: 6px;
    padding: 6px 8px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-2, rgba(255, 255, 255, 0.7));
    cursor: pointer;
    transition: background 150ms ease, color 150ms ease;
    text-align: center;
  }

  .segmented-btn.active {
    background: var(--surface-raised, rgba(255, 255, 255, 0.15));
    color: var(--text-1, #ffffff);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
  }

  .radio-group {
    display: flex;
    gap: 16px;
    margin-bottom: 12px;
    padding: 0 4px;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    cursor: pointer;
    color: var(--text-1, #ffffff);
  }

  .radio-label input {
    cursor: pointer;
    accent-color: var(--accent, #0a84ff);
  }

  .palette-swatches-section {
    margin-bottom: 12px;
    padding: 10px;
    background: rgba(0, 0, 0, 0.15);
    border-radius: 8px;
  }

  .swatches-row {
    display: flex;
    justify-content: space-between;
    gap: 8px;
  }

  .swatch-item {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .swatch-preview {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.2);
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.3);
  }

  .swatch-label {
    font-size: 10px;
    color: var(--text-2, rgba(255, 255, 255, 0.6));
  }

  .swatch-hex {
    font-size: 10px;
    font-family: var(--font-mono, monospace);
    color: var(--text-1, #ffffff);
  }

  .actions-row {
    margin-top: 10px;
    display: flex;
    justify-content: flex-end;
  }

  .regenerate-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 12px;
    background: var(--surface-raised, rgba(255, 255, 255, 0.12));
    border: 1px solid var(--separator, rgba(255, 255, 255, 0.16));
    border-radius: 6px;
    font-size: 11px;
    color: var(--text-1, #ffffff);
    cursor: pointer;
  }

  .regenerate-btn:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.2);
  }

  .regenerate-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .spinner {
    width: 10px;
    height: 10px;
    border: 2px solid rgba(255, 255, 255, 0.3);
    border-top-color: #ffffff;
    border-radius: 50%;
    animation: spin 700ms linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .manual-color-row {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .manual-color-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .picker-group {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .color-picker-input {
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    background: none;
  }

  .hex-text-input {
    width: 76px;
    padding: 3px 6px;
    background: rgba(0, 0, 0, 0.3);
    border: 1px solid var(--separator, rgba(255, 255, 255, 0.16));
    border-radius: 4px;
    color: var(--text-1, #ffffff);
    font-size: 11px;
    font-family: var(--font-mono, monospace);
  }

  .hex-text-input.has-error {
    border-color: #ff453a;
    background: rgba(255, 69, 58, 0.15);
  }

  .warning-badge {
    margin-top: 8px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    background: rgba(255, 159, 10, 0.18);
    border: 1px solid rgba(255, 159, 10, 0.4);
    border-radius: 6px;
    color: #ff9f0a;
    font-size: 11px;
    font-weight: 500;
  }

  .warning-icon {
    font-size: 12px;
  }

  .theme-hint {
    margin: 8px 0 0;
    font-size: 11px;
    color: var(--text-3, rgba(255, 255, 255, 0.6));
    line-height: 1.4;
  }
</style>
