<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';

  let {
    open = $bindable(false),
    count = 1,
    onconfirm,
    oncancel
  }: {
    open: boolean;
    count?: number;
    onconfirm?: () => void;
    oncancel?: () => void;
  } = $props();

  function handleConfirm() {
    onconfirm?.();
    open = false;
  }

  function handleCancel() {
    oncancel?.();
    open = false;
  }
</script>

{#if open}
  <div class="sheet-modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) handleCancel(); }} role="presentation">
    <div class="sheet-container" role="dialog" aria-modal="true" tabindex="-1">
      <Surface kind="sheet" class="delete-sheet-surface">
        <div class="delete-sheet-icon">
          <Icon name="trash" size={32} />
        </div>

        <h3 class="delete-sheet-title">
          Delete {count} {count === 1 ? 'Photo' : 'Photos'}?
        </h3>

        <p class="delete-sheet-desc">
          This item will be deleted from all your synced devices and moved to Recently Deleted, where it will remain for 30 days before being permanently removed from MEGA S4.
        </p>

        <div class="delete-sheet-actions">
          <Button variant="danger" onclick={handleConfirm}>
            Delete from All Devices
          </Button>
          <Button variant="ghost" onclick={handleCancel}>
            Cancel
          </Button>
        </div>
      </Surface>
    </div>
  </div>
{/if}

<style>
  .sheet-modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 850;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(8px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
  }

  .sheet-container {
    width: 360px;
    max-width: 90vw;
  }

  :global(.delete-sheet-surface) {
    padding: 24px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    border-radius: var(--r-xl);
    box-shadow: var(--shadow-3);
  }

  .delete-sheet-icon {
    color: var(--danger);
    margin-bottom: 12px;
  }

  .delete-sheet-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0 0 8px;
  }

  .delete-sheet-desc {
    font-size: var(--text-size-body);
    line-height: var(--text-lh-body);
    color: var(--text-2);
    margin: 0 0 20px;
  }

  .delete-sheet-actions {
    display: flex;
    flex-direction: column;
    width: 100%;
    gap: 8px;
  }
</style>
