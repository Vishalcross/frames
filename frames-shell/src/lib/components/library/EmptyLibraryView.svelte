<script lang="ts">
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';

  let {
    isTauri = false,
    useRealBackend = true,
    onimport,
    onseed,
    ontogglebenchmark
  }: {
    isTauri?: boolean;
    useRealBackend?: boolean;
    onimport?: () => void;
    onseed?: () => void;
    ontogglebenchmark?: () => void;
  } = $props();
</script>

<div class="empty-library-container">
  <div class="empty-dashed-dropzone">
    <div class="empty-icon-circle">
      <Icon name="photo" size={40} stroke={1.5} />
    </div>

    <h2 class="empty-title">No Photos or Videos</h2>
    <p class="empty-desc">
      {#if isTauri}
        Connected to Rust SQLite Core. Import local photos or folders from your computer, or seed sample data with real EXIF & thumbnails.
      {:else}
        Your library is empty. Drag and drop image files or folders from your computer to begin.
      {/if}
    </p>

    <div class="actions-row">
      <Button variant="accent" onclick={onimport}>
        <Icon name="arrow-down-to-line" size={15} />
        <span>Import Photos…</span>
      </Button>

      {#if isTauri && onseed}
        <Button variant="standard" onclick={onseed}>
          <Icon name="sparkles" size={15} />
          <span>Seed SQLite Library</span>
        </Button>
      {/if}

      {#if ontogglebenchmark}
        <Button variant="ghost" onclick={ontogglebenchmark}>
          <Icon name="layers" size={15} />
          <span>{useRealBackend ? 'View 100k Benchmark' : 'Back to SQLite Core'}</span>
        </Button>
      {/if}
    </div>
  </div>
</div>

<style>
  .empty-library-container {
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: calc(var(--toolbar-h) + 32px) 32px 32px;
    box-sizing: border-box;
  }

  /* Empty library with dashed drop zone (§4.12) */
  .empty-dashed-dropzone {
    width: 560px;
    max-width: 90%;
    border: 2px dashed var(--separator);
    border-radius: var(--r-xl);
    padding: 48px 32px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 14px;
    background: var(--fill-1);
  }

  .empty-icon-circle {
    width: 72px;
    height: 72px;
    border-radius: 50%;
    background: var(--fill-2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
  }

  .empty-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0;
  }

  .empty-desc {
    font-size: var(--text-size-body);
    line-height: var(--text-lh-body);
    color: var(--text-2);
    margin: 0 0 12px;
    max-width: 440px;
  }

  .actions-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 10px;
  }
</style>
