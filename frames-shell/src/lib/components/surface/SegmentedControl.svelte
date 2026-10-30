<script lang="ts">
  import { theme } from '$lib/theme/engine.svelte';

  let {
    options,
    value = $bindable(),
    onchange,
    class: klass = ''
  }: {
    options: { value: string; label: string }[];
    value: string;
    onchange?: (val: string) => void;
    class?: string;
  } = $props();

  const selectedIndex = $derived(
    Math.max(0, options.findIndex((opt) => opt.value === value))
  );

  function select(val: string) {
    value = val;
    onchange?.(val);
  }

  function handleKeydown(e: KeyboardEvent, index: number) {
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault();
      const next = (index + 1) % options.length;
      select(options[next].value);
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault();
      const prev = (index - 1 + options.length) % options.length;
      select(options[prev].value);
    }
  }
</script>

<div
  class="segmented-control appearance-{theme.appearance} {klass}"
  role="radiogroup"
  aria-label="View mode selector"
>
  <!-- Raised sliding thumb (§6) -->
  <div
    class="segmented-thumb"
    style="
      width: calc((100% - 6px) / {options.length});
      transform: translateX(calc({selectedIndex} * 100%));
    "
  ></div>

  {#each options as opt, i}
    {@const isSelected = opt.value === value}
    <button
      type="button"
      role="radio"
      aria-checked={isSelected}
      tabindex={isSelected ? 0 : -1}
      class="segmented-btn {isSelected ? 'selected' : ''}"
      onclick={() => select(opt.value)}
      onkeydown={(e) => handleKeydown(e, i)}
    >
      {opt.label}
    </button>
  {/each}
</div>

<style>
  .segmented-control {
    position: relative;
    display: inline-flex;
    align-items: center;
    background: var(--fill-1);
    border-radius: 9px;
    padding: 3px;
    height: 28px;
    box-sizing: border-box;
    user-select: none;
  }

  .segmented-thumb {
    position: absolute;
    top: 3px;
    bottom: 3px;
    left: 3px;
    border-radius: 7px;
    background: var(--surface-raised);
    box-shadow: var(--shadow-1);
    transition: transform var(--dur-base) var(--ease-standard);
    pointer-events: none;
    z-index: 1;
  }

  .segmented-btn {
    position: relative;
    z-index: 2;
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 0 12px;
    border: none;
    background: transparent;
    color: var(--text-2);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    font-weight: 500;
    line-height: var(--text-lh-body);
    cursor: pointer;
    border-radius: 7px;
    outline: none;
    white-space: nowrap;
    transition: color var(--dur-fast) var(--ease-standard);
  }

  .segmented-btn:focus-visible {
    box-shadow: 0 0 0 2px var(--focus-ring);
  }

  .segmented-btn.selected {
    color: var(--text-1);
    font-weight: 600;
  }

  /* Glass mode capsules (§8.1 #2) */
  .appearance-glass.segmented-control {
    border-radius: var(--r-pill);
    background: rgba(0, 0, 0, 0.08);
  }
  .appearance-glass .segmented-thumb {
    border-radius: var(--r-pill);
    background: rgba(255, 255, 255, 0.28);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
  }
  .appearance-glass .segmented-btn.selected {
    color: var(--glass-fg, #ffffff);
  }
</style>
