<script lang="ts">
  import { tick, onMount } from 'svelte';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    options,
    items,
    value = $bindable(),
    selected = $bindable(),
    onchange,
    class: klass = ''
  }: {
    options?: { value?: string; id?: string; label: string }[];
    items?: { value?: string; id?: string; label: string }[];
    value?: string;
    selected?: string;
    onchange?: (val: string) => void;
    class?: string;
  } = $props();

  const controlItems = $derived(
    (items || options || []).map((it) => ({
      id: it.id ?? it.value ?? '',
      label: it.label
    }))
  );

  const usesItems = $derived(items !== undefined || (selected !== undefined && value === undefined));
  const activeId = $derived(usesItems ? (selected ?? '') : (value ?? selected ?? ''));

  let buttons: HTMLButtonElement[] = $state([]);
  let left = $state(0);
  let width = $state(0);
  let ready = $state(false);

  async function measure() {
    await tick();
    const i = controlItems.findIndex((it) => it.id === activeId);
    const el = buttons[i];
    if (!el) return;
    left = el.offsetLeft;
    width = el.offsetWidth;
  }

  function select(id: string) {
    if (usesItems || value === undefined) {
      selected = id;
    }
    if (!usesItems || options !== undefined || value !== undefined) {
      value = id;
    }
    onchange?.(id);
  }

  function handleKeydown(e: KeyboardEvent, currentIndex: number) {
    if (controlItems.length === 0) return;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault();
      const nextIndex = (currentIndex + 1) % controlItems.length;
      const nextItem = controlItems[nextIndex];
      if (nextItem) {
        select(nextItem.id);
        buttons[nextIndex]?.focus();
      }
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault();
      const prevIndex = (currentIndex - 1 + controlItems.length) % controlItems.length;
      const prevItem = controlItems[prevIndex];
      if (prevItem) {
        select(prevItem.id);
        buttons[prevIndex]?.focus();
      }
    }
  }

  onMount(() => {
    if (typeof document !== 'undefined' && document.fonts) {
      document.fonts.ready.then(async () => {
        await measure();
        ready = true;
      });
    } else {
      measure().then(() => {
        ready = true;
      });
    }
    const ro = new ResizeObserver(measure);
    buttons.forEach((b) => b && ro.observe(b));

    const onWindowResize = () => measure();
    if (typeof window !== 'undefined') {
      window.addEventListener('resize', onWindowResize);
    }

    return () => {
      ro.disconnect();
      if (typeof window !== 'undefined') {
        window.removeEventListener('resize', onWindowResize);
      }
    };
  });

  $effect(() => {
    activeId;
    measure();
  });
</script>

<div
  class="seg segmented-control appearance-{theme.appearance} {klass}"
  role="tablist"
  aria-label="View mode selector"
>
  <div class="pill segmented-thumb" class:ready style="transform: translateX({left}px); width: {width}px"></div>
  {#each controlItems as it, i}
    {@const isSelected = it.id === activeId}
    <button
      bind:this={buttons[i]}
      type="button"
      role="tab"
      aria-selected={isSelected}
      tabindex={isSelected || (i === 0 && !controlItems.some((x) => x.id === activeId)) ? 0 : -1}
      class="segmented-btn"
      class:active={isSelected}
      class:selected={isSelected}
      onclick={() => select(it.id)}
      onkeydown={(e) => handleKeydown(e, i)}
    >
      {it.label}
    </button>
  {/each}
</div>

<style>
  .seg {
    position: relative;
    display: inline-flex;
    padding: 3px;
    background: var(--fill-1);
    border-radius: 999px;
    box-sizing: border-box;
    user-select: none;
    align-items: center;
  }

  .pill {
    position: absolute;
    top: 3px;
    bottom: 3px;
    left: 0;
    border-radius: 999px;
    background: var(--surface-raised);
    box-shadow: var(--shadow-1);
    pointer-events: none;
    z-index: 0;
  }

  .pill.ready {
    transition: transform 260ms cubic-bezier(.3, .7, .2, 1), width 260ms cubic-bezier(.3, .7, .2, 1);
  }

  .seg button {
    position: relative;
    z-index: 1;
    padding: 6px 16px;
    white-space: nowrap;
    border: none;
    background: transparent;
    color: var(--text-2);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    font-weight: 500;
    line-height: var(--text-lh-body);
    cursor: pointer;
    border-radius: 999px;
    outline: none;
    transition: color var(--dur-fast) var(--ease-standard);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .seg button:focus-visible {
    box-shadow: 0 0 0 2px var(--focus-ring);
  }

  .seg button.active,
  .seg button.selected {
    color: var(--text-1);
    font-weight: 600;
  }

  /* Glass look */
  .appearance-glass.seg {
    background: rgba(0, 0, 0, 0.08);
  }
  .appearance-glass .pill {
    background: rgba(255, 255, 255, 0.28);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
  }
  .appearance-glass.seg button.active,
  .appearance-glass.seg button.selected {
    color: var(--glass-fg, #ffffff);
  }
</style>
