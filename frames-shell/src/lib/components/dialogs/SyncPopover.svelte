<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import type { SyncQueueItem } from '$lib/types';
  import { tauriRepo } from '$lib/services/tauriRepo';

  let {
    open = $bindable(false),
    isPaused = $bindable(false),
    style = ''
  }: {
    open: boolean;
    isPaused: boolean;
    style?: string;
  } = $props();

  let queue = $state<SyncQueueItem[]>([]);

  function togglePause() {
    isPaused = !isPaused;
  }
</script>

{#if open}
  <div class="sync-backdrop" onclick={() => (open = false)} role="presentation"></div>

  <Surface kind="popover" class="sync-popover" {style}>
    <div class="sync-popover-header">
      <div class="sync-title-wrap">
        <Icon name={isPaused ? 'cloud' : 'cloud-upload'} size={16} />
        <span class="sync-title">{isPaused ? 'Sync Paused' : tauriRepo.isSyncing ? 'Syncing with S4' : 'Up to Date'}</span>
      </div>
      <Button
        variant="ghost"
        class="pause-toggle-btn"
        onclick={togglePause}
      >
        <Icon name={isPaused ? 'play' : 'pause'} size={13} />
        <span>{isPaused ? 'Resume' : 'Pause'}</span>
      </Button>
    </div>

    <div class="sync-queue-list">
      {#if queue.length === 0}
        <div class="empty-sync-notice">
          <Icon name="check-circle" size={18} />
          <span>All photos and albums are synced with MEGA S4</span>
        </div>
      {:else}
        {#each queue as item}
          <div class="sync-queue-item">
            <div class="item-meta">
              <span class="item-filename">{item.filename}</span>
              <span class="item-progress-text">{item.progress}%</span>
            </div>
            <div class="progress-bar-bg">
              <div class="progress-bar-fill" style="width: {item.progress}%;"></div>
            </div>
          </div>
        {/each}
      {/if}
    </div>

    <div class="sync-footer">
      <span class="sync-status-sub">
        {tauriRepo.outboxPending > 0 ? `${tauriRepo.outboxPending} items pending sync` : 'MEGA S4 Object Storage Connected'}
      </span>
    </div>
  </Surface>
{/if}

<style>
  .sync-backdrop {
    position: fixed;
    inset: 0;
    z-index: 99;
  }

  :global(.sync-popover) {
    position: absolute;
    bottom: calc(var(--toolbar-h) + 12px);
    left: 12px;
    width: 290px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 16px;
    box-sizing: border-box;
    z-index: 100;
  }

  .sync-popover-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .sync-title-wrap {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--text-size-body);
    font-weight: 600;
    color: var(--text-1);
  }

  :global(.pause-toggle-btn) {
    font-size: var(--text-size-caption);
    height: 24px;
    padding: 0 8px;
  }

  .sync-queue-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-height: 180px;
    overflow-y: auto;
  }

  .empty-sync-notice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 4px;
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }

  .sync-queue-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .item-meta {
    display: flex;
    justify-content: space-between;
    font-size: var(--text-size-caption);
  }

  .item-filename {
    color: var(--text-1);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 190px;
  }

  .item-progress-text {
    color: var(--text-2);
  }

  .progress-bar-bg {
    width: 100%;
    height: 4px;
    background: var(--separator);
    border-radius: var(--r-full);
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: var(--r-full);
    transition: width 200ms ease;
  }

  .sync-footer {
    padding-top: 8px;
    border-top: 1px solid var(--separator);
  }

  .sync-status-sub {
    font-size: 11px;
    color: var(--text-2);
  }
</style>
