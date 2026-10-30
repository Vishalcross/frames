<script lang="ts">
  import type { Snippet } from 'svelte';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    variant = 'standard',
    active = false,
    disabled = false,
    title = '',
    ariaLabel = '',
    class: klass = '',
    style = '',
    onclick,
    children,
    ...rest
  }: {
    variant?: 'standard' | 'icon' | 'capsule' | 'ghost' | 'accent' | 'danger';
    active?: boolean;
    disabled?: boolean;
    title?: string;
    ariaLabel?: string;
    class?: string;
    style?: string;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
    [key: string]: any;
  } = $props();
</script>

<button
  type="button"
  class="btn btn-{variant} appearance-{theme.appearance} {active ? 'is-active' : ''} {klass}"
  {disabled}
  {title}
  aria-label={ariaLabel || title}
  {style}
  {onclick}
  {...rest}
>
  {@render children?.()}
</button>

<style>
  .btn {
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 28px;
    height: 28px;
    min-width: 28px;
    padding: 0 10px;
    border-radius: 7px;
    border: none;
    background: transparent;
    color: inherit;
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    font-weight: 500;
    line-height: var(--text-lh-body);
    cursor: pointer;
    user-select: none;
    outline: none;
    position: relative;
    transition: background var(--dur-fast) var(--ease-standard),
                color var(--dur-fast) var(--ease-standard),
                transform var(--dur-fast) var(--ease-standard),
                opacity var(--dur-fast) var(--ease-standard);
  }

  .btn:focus-visible {
    box-shadow: 0 0 0 2px var(--focus-ring);
    outline: 2px solid transparent;
    outline-offset: 2px;
  }

  .btn:disabled {
    opacity: 0.38;
    pointer-events: none;
  }

  .btn-icon {
    padding: 0 6px;
    width: 28px;
  }

  .btn-capsule {
    border-radius: var(--r-pill);
    padding: 0 14px;
  }

  /* Standard & Adaptive variants */
  .appearance-standard.btn-standard:hover,
  .appearance-adaptive.btn-standard:hover,
  .appearance-standard.btn-icon:hover,
  .appearance-adaptive.btn-icon:hover,
  .appearance-standard.btn-ghost:hover,
  .appearance-adaptive.btn-ghost:hover {
    background: var(--fill-1);
  }

  .appearance-standard.btn-standard:active,
  .appearance-adaptive.btn-standard:active,
  .appearance-standard.btn-icon:active,
  .appearance-adaptive.btn-icon:active {
    background: var(--fill-2);
  }

  .appearance-standard.is-active,
  .appearance-adaptive.is-active {
    background: var(--fill-2);
    color: var(--accent);
  }

  .btn-accent {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .btn-accent:hover {
    filter: brightness(1.06);
  }
  .btn-accent:active {
    filter: brightness(0.95);
  }

  .btn-danger {
    background: var(--danger);
    color: #ffffff;
  }
  .btn-danger:hover {
    filter: brightness(1.08);
  }

  /* Glass press response: scale .97 springing back (§8.4) */
  .appearance-glass.btn {
    border-radius: var(--r-pill);
    transition: transform 300ms var(--ease-overshoot),
                background var(--dur-fast) var(--ease-standard),
                filter var(--dur-fast) var(--ease-standard);
  }

  .appearance-glass.btn:hover {
    background: rgba(255, 255, 255, 0.12);
  }

  .appearance-glass.btn:active {
    transform: scale(0.97);
    filter: brightness(1.1);
  }

  .appearance-glass.is-active {
    background: rgba(255, 255, 255, 0.22);
    color: var(--accent);
  }
</style>
