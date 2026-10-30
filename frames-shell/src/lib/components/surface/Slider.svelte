<script lang="ts">
  import { theme } from '$lib/theme/engine.svelte';

  let {
    label = '',
    value = $bindable(0),
    min = 0,
    max = 100,
    step = 1,
    defaultValue = 0,
    unit = '',
    formatValue,
    width = '100%',
    class: klass = '',
    onchange
  }: {
    label?: string;
    value: number;
    min?: number;
    max?: number;
    step?: number;
    defaultValue?: number;
    unit?: string;
    formatValue?: (val: number) => string;
    width?: string;
    class?: string;
    onchange?: (val: number) => void;
  } = $props();

  const isEdited = $derived(value !== defaultValue);
  const displayVal = $derived(
    formatValue ? formatValue(value) : `${value > 0 && min < 0 ? '+' : ''}${value}${unit}`
  );

  function handleReset() {
    value = defaultValue;
    onchange?.(defaultValue);
  }

  function handleInput(e: Event) {
    const target = e.target as HTMLInputElement;
    const num = parseFloat(target.value);
    value = num;
    onchange?.(num);
  }
</script>

<div
  role="presentation"
  class="slider-field appearance-{theme.appearance} {klass}"
  style="width: {width};"
  ondblclick={handleReset}
  title="Double-click to reset"
>
  {#if label}
    <div class="slider-header">
      <span class="slider-label">
        {#if isEdited}
          <span class="edited-dot" title="Modified value"></span>
        {/if}
        {label}
      </span>
      <span class="slider-value">{displayVal}</span>
    </div>
  {/if}

  <div class="slider-track-wrap">
    <input
      type="range"
      {min}
      {max}
      {step}
      {value}
      oninput={handleInput}
      class="slider-input"
    />
  </div>
</div>

<style>
  .slider-field {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 4px;
    user-select: none;
  }

  .slider-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: var(--text-size-caption);
    line-height: var(--text-lh-caption);
    color: var(--text-2);
  }

  .slider-label {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-weight: 500;
  }

  .edited-dot {
    width: 5px;
    height: 5px;
    border-radius: var(--r-pill);
    background: var(--accent);
  }

  .slider-value {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-3);
  }

  .slider-track-wrap {
    display: flex;
    align-items: center;
    height: 20px;
  }

  .slider-input {
    -webkit-appearance: none;
    appearance: none;
    width: 100%;
    height: 4px;
    border-radius: var(--r-pill);
    background: var(--fill-2);
    outline: none;
    cursor: pointer;
    margin: 0;
  }

  .slider-input::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 14px;
    height: 14px;
    border-radius: var(--r-pill);
    background: var(--surface-raised);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
    border: 1px solid var(--separator);
    transition: transform var(--dur-fast) var(--ease-standard);
  }

  .slider-input::-webkit-slider-thumb:hover {
    transform: scale(1.15);
  }

  .slider-input::-webkit-slider-thumb:active {
    transform: scale(0.95);
    background: var(--accent);
  }

  /* Glass thumb (§8.1 #1) */
  .appearance-glass .slider-input::-webkit-slider-thumb {
    background: rgba(255, 255, 255, 0.85);
    backdrop-filter: blur(8px);
    border: 1px solid rgba(255, 255, 255, 0.6);
  }
</style>
