<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    type = 'text',
    placeholder = '',
    value = $bindable(''),
    icon = '',
    expandOnFocus = false,
    baseWidth = '240px',
    expandedWidth = '400px',
    class: klass = '',
    onchange,
    oninput,
    onfocus,
    onblur,
    onkeydown
  }: {
    type?: string;
    placeholder?: string;
    value?: string;
    icon?: string;
    expandOnFocus?: boolean;
    baseWidth?: string;
    expandedWidth?: string;
    class?: string;
    onchange?: (val: string) => void;
    oninput?: (e: Event) => void;
    onfocus?: (e: FocusEvent) => void;
    onblur?: (e: FocusEvent) => void;
    onkeydown?: (e: KeyboardEvent) => void;
  } = $props();

  let isFocused = $state(false);
  let inputRef: HTMLInputElement | null = $state(null);

  export function focus() {
    inputRef?.focus();
  }

  function handleFocus(e: FocusEvent) {
    isFocused = true;
    onfocus?.(e);
  }

  function handleBlur(e: FocusEvent) {
    isFocused = false;
    onblur?.(e);
  }

  function handleInput(e: Event) {
    const target = e.target as HTMLInputElement;
    value = target.value;
    oninput?.(e);
  }

  function handleClear() {
    value = '';
    onchange?.('');
    inputRef?.focus();
  }
</script>

<div
  class="field-wrap appearance-{theme.appearance} {isFocused ? 'is-focused' : ''} {expandOnFocus ? 'can-expand' : ''} {klass}"
  style="--base-w: {baseWidth}; --expanded-w: {expandedWidth};"
>
  {#if icon}
    <span class="field-icon">
      <Icon name={icon} size={14} />
    </span>
  {/if}

  <input
    bind:this={inputRef}
    {type}
    {placeholder}
    {value}
    onfocus={handleFocus}
    onblur={handleBlur}
    oninput={handleInput}
    {onkeydown}
    class="field-input"
  />

  {#if value}
    <button type="button" class="clear-btn" onclick={handleClear} aria-label="Clear text">
      <Icon name="x" size={12} />
    </button>
  {/if}
</div>

<style>
  .field-wrap {
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    position: relative;
    height: 28px;
    width: var(--base-w, 240px);
    border-radius: var(--r-sm);
    background: var(--fill-1);
    padding: 0 8px;
    gap: 6px;
    transition: width var(--dur-base) var(--ease-standard),
                background var(--dur-fast) var(--ease-standard),
                box-shadow var(--dur-fast) var(--ease-standard);
  }

  .field-wrap.can-expand.is-focused {
    width: var(--expanded-w, 400px);
  }

  .field-wrap.is-focused {
    background: var(--surface-raised);
    box-shadow: 0 0 0 2px var(--focus-ring);
  }

  .field-icon {
    display: flex;
    align-items: center;
    color: var(--text-3);
    pointer-events: none;
  }

  .field-input {
    flex: 1;
    height: 100%;
    border: none;
    background: transparent;
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    line-height: var(--text-lh-body);
    outline: none;
    min-width: 0;
  }

  .field-input::placeholder {
    color: var(--text-3);
  }

  .clear-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: var(--r-pill);
    border: none;
    background: var(--fill-2);
    color: var(--text-2);
    cursor: pointer;
    padding: 0;
  }

  .clear-btn:hover {
    background: var(--fill-3);
    color: var(--text-1);
  }

  /* Glass Mode (§8.1 #2) */
  .appearance-glass.field-wrap {
    border-radius: var(--r-pill);
    background: rgba(0, 0, 0, 0.08);
  }
  .appearance-glass.field-wrap.is-focused {
    background: rgba(255, 255, 255, 0.25);
    box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.8), 0 0 0 4px rgba(0, 0, 0, 0.2);
  }
</style>
