<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';

  let {
    isDragging = $bindable(false),
    ondropfiles
  }: {
    isDragging: boolean;
    ondropfiles?: (files: FileList) => void;
  } = $props();

  function handleDragOver(e: DragEvent) {
    e.preventDefault();
  }

  function handleDragLeave(e: DragEvent) {
    if (e.relatedTarget === null) {
      isDragging = false;
    }
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    if (e.dataTransfer && e.dataTransfer.files) {
      ondropfiles?.(e.dataTransfer.files);
    }
  }
</script>

{#if isDragging}
  <div
    class="import-drag-overlay"
    ondragover={handleDragOver}
    ondragleave={handleDragLeave}
    ondrop={handleDrop}
    role="region"
    aria-label="Import files drop area"
  >
    <div class="import-dashed-box">
      <div class="import-icon-wrap">
        <Icon name="arrow-down-to-line" size={48} stroke={1.5} />
      </div>
      <h2 class="import-heading">Drop Photos and Videos to Import</h2>
      <p class="import-sub">
        Files will be indexed and synced with your MEGA S4 storage
      </p>
    </div>
  </div>
{/if}

<style>
  .import-drag-overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: rgba(0, 122, 255, 0.12);
    backdrop-filter: blur(14px);
    -webkit-backdrop-filter: blur(14px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 32px;
    box-sizing: border-box;
  }

  .import-dashed-box {
    width: 100%;
    height: 100%;
    border: 3px dashed var(--accent);
    border-radius: var(--r-xl);
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    background: color-mix(in oklab, var(--bg-canvas) 85%, transparent);
    box-sizing: border-box;
  }

  .import-icon-wrap {
    color: var(--accent);
  }

  .import-heading {
    font-size: var(--text-size-large-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0;
  }

  .import-sub {
    font-size: var(--text-size-body);
    color: var(--text-2);
    margin: 0;
  }
</style>
