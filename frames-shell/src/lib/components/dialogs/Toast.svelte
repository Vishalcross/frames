<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';

  export interface ToastMessage {
    id: string;
    type: 'error' | 'warning' | 'info';
    text: string;
  }

  let {
    toasts = $bindable<ToastMessage[]>([]),
    isOffline = false
  }: {
    toasts?: ToastMessage[];
    isOffline?: boolean;
  } = $props();

  function dismiss(id: string) {
    toasts = toasts.filter((t) => t.id !== id);
  }
</script>

<div class="toast-stack-container" aria-live="polite">
  <!-- Offline status chip (§4.12) -->
  {#if isOffline}
    <Surface kind="floating" class="offline-chip">
      <Icon name="cloud" size={14} class="offline-icon" />
      <span>Working Offline · S4 changes queued</span>
    </Surface>
  {/if}

  <!-- Error & Status toasts (§4.12) -->
  {#each toasts as t}
    <Surface kind="floating" class="toast-item {t.type}">
      <Icon name={t.type === 'error' ? 'trash' : 'info'} size={15} />
      <span class="toast-text">{t.text}</span>
      <button type="button" class="toast-dismiss-btn" onclick={() => dismiss(t.id)}>
        <Icon name="x" size={12} />
      </button>
    </Surface>
  {/each}
</div>

<style>
  .toast-stack-container {
    position: fixed;
    top: calc(var(--toolbar-h) + 12px);
    right: 20px;
    z-index: 900;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }

  :global(.offline-chip) {
    pointer-events: auto;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border-radius: var(--r-pill);
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: var(--text-2);
    box-shadow: var(--shadow-2);
  }

  :global(.offline-icon) {
    opacity: 0.5;
  }

  :global(.toast-item) {
    pointer-events: auto;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-radius: var(--r-sm);
    font-size: var(--text-size-body);
    font-weight: 500;
    box-shadow: var(--shadow-2);
  }

  :global(.toast-item.error) {
    border-left: 3px solid var(--danger);
    color: var(--danger);
  }

  .toast-text {
    color: var(--text-1);
  }

  .toast-dismiss-btn {
    border: none;
    background: transparent;
    color: var(--text-3);
    cursor: pointer;
    display: flex;
    align-items: center;
    padding: 2px;
  }
  .toast-dismiss-btn:hover {
    color: var(--text-1);
  }
</style>
