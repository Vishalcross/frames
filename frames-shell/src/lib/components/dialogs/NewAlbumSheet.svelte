<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import { portal } from '$lib/actions/portal';

  let {
    open = $bindable(false),
    count = 0,
    oncreate
  }: {
    open: boolean;
    /** Selected photos that go into the new album. */
    count?: number;
    oncreate?: (name: string) => void;
  } = $props();

  let name = $state('');
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (open) {
      name = '';
      queueMicrotask(() => input?.focus());
    }
  });

  function submit(e?: Event) {
    e?.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    open = false;
    oncreate?.(trimmed);
  }
</script>

{#if open}
  <div
    use:portal
    class="sheet-modal-backdrop"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) open = false;
    }}
    onkeydown={(e) => {
      if (e.key === 'Escape') open = false;
    }}
  >
    <div class="sheet-container" role="dialog" aria-modal="true" aria-labelledby="new-album-title" tabindex="-1">
      <Surface kind="sheet" class="new-album-surface">
        <form onsubmit={submit}>
          <h3 id="new-album-title" class="new-album-title">New Album</h3>
          {#if count > 0}
            <p class="new-album-desc">{count} {count === 1 ? 'item' : 'items'} will be added to it.</p>
          {/if}
          <input
            bind:this={input}
            bind:value={name}
            class="new-album-input"
            placeholder="Album name"
            maxlength="120"
            aria-label="Album name"
          />
          <div class="new-album-actions">
            <Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
            <Button variant="accent" disabled={!name.trim()} onclick={() => submit()}>Create</Button>
          </div>
        </form>
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

  :global(.new-album-surface) {
    padding: 22px;
  }

  .new-album-title {
    margin: 0 0 6px;
    font-size: var(--text-size-title);
    line-height: var(--text-lh-title);
    color: var(--text-1);
  }

  .new-album-desc {
    margin: 0 0 12px;
    font-size: var(--text-size-body);
    color: var(--text-2);
  }

  .new-album-input {
    box-sizing: border-box;
    width: 100%;
    margin: 6px 0 18px;
    padding: 8px 10px;
    border: 1px solid var(--separator);
    border-radius: var(--r-sm);
    background: var(--fill-1);
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
  }

  .new-album-input:focus {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }

  .new-album-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
