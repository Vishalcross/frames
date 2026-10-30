<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { SAMPLE_SYNC_QUEUE } from '$lib/mock/sampleData';
  import type { SyncQueueItem } from '$lib/types';

  let {
    open = $bindable(false),
    isPaused = $bindable(false),
    style = ''
  }: {
    open: boolean;
    isPaused: boolean;
    style?: string;
  } = $props();

  let queue = $state<SyncQueueItem[]>(SAMPLE_SYNC_QUEUE);

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
        <span class="sync-title">{isPaused ? 'Sync Paused' : 'Syncing with S4'}</span>
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
    </div>

    <div class="sync-footer">
      <span class="sync-status-sub">750 GB MEGA S4 · 548 GB used</span>
    </div>
  </Surface>
{/if}

<style>
  .sync-backdrop {
    position: fixed;
    inset: 0;
    z-index: 998;
  }

  :global(.sync-popover) {
    position: absolute;
    bottom: 48px;
    left: 12px;
    width: 280px;
    padding: 12px;
    z-index: 999;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .sync-popover-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .sync-title-wrap {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    font-size: var(--text-size-body);
    color: var(--text-1);
  }

  .sync-queue-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
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
    color: var(--text-2);
  }

  .item-filename {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-1);
  }

  .progress-bar-bg {
    height: 4px;
    border-radius: var(--r-pill);
    background: var(--fill-2);
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: var(--r-pill);
    transition: width var(--dur-base) var(--ease-standard);
  }

  .sync-footer {
    padding-top: 4px;
    border-top: 1px solid var(--separator);
    font-size: var(--text-size-caption);
    color: var(--text-3);
  }
</style>
