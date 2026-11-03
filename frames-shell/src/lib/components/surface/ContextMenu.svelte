<script lang="ts">
  import Menu, { type MenuItem } from './Menu.svelte';
  import { portal } from '$lib/actions/portal';

  let {
    items,
    open = $bindable(false),
    x = 0,
    y = 0,
    align = 'left',
    onselect
  }: {
    items: MenuItem[];
    open: boolean;
    /** Viewport point the menu opens at (a click, or the corner of a button). */
    x?: number;
    y?: number;
    /** 'right' puts the menu's right edge at `x`, for buttons near the right side. */
    align?: 'left' | 'right';
    onselect?: (item: MenuItem) => void;
  } = $props();

  const WIDTH = 220;
  const ROW = 30;

  // Keep the whole menu on screen.
  const left = $derived(
    Math.max(8, Math.min(align === 'right' ? x - WIDTH : x, window.innerWidth - WIDTH - 8))
  );
  const top = $derived(Math.max(8, Math.min(y, window.innerHeight - items.length * ROW - 24)));
</script>

{#if open}
  <div use:portal>
    <Menu
      {items}
      bind:open
      {onselect}
      style="position: fixed; top: {top}px; left: {left}px; min-width: {WIDTH}px;"
    />
  </div>
{/if}
