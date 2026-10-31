<script lang="ts">
  import type { Snippet } from 'svelte';
  import { theme, resolved } from '$lib/theme/engine.svelte';
  import { setupGlassSpecular } from '$lib/theme/glass';

  let {
    kind = 'control',
    clarity = 'regular',
    dimmed = false,
    radius,
    class: klass = '',
    style = '',
    children,
    ...rest
  }: {
    kind?: 'sidebar' | 'toolbar' | 'cluster' | 'floating' | 'panel' | 'popover' | 'sheet' | 'control';
    clarity?: 'regular' | 'clear';
    dimmed?: boolean;
    radius?: string;
    class?: string;
    style?: string;
    children?: Snippet;
    [key: string]: any;
  } = $props();

  let element: HTMLElement | null = $state(null);

  $effect(() => {
    if (theme.appearance === 'glass' && element) {
      const specular = setupGlassSpecular(element);
      return () => {
        specular.destroy();
      };
    }
  });

  // Radii selection based on kind and mode (§8.1 #2)
  const resolvedRadius = $derived.by(() => {
    if (radius) return radius;
    if (kind === 'sidebar') return '0px';
    if (theme.appearance === 'glass') {
      if (kind === 'panel' || kind === 'popover' || kind === 'sheet') return '22px';
      if (kind === 'toolbar' || kind === 'cluster' || kind === 'floating' || kind === 'control') return '999px';
    }
    if (kind === 'toolbar') return '0px';
    if (kind === 'popover' || kind === 'panel') return 'var(--r-md)';
    if (kind === 'sheet') return 'var(--r-lg)';
    if (kind === 'control') return 'var(--r-sm)';
    return 'var(--r-md)';
  });
</script>

<div
  bind:this={element}
  class="surface surface-{kind} appearance-{theme.appearance} {theme.appearance === 'glass' && kind !== 'sidebar' ? 'glass-material' : ''} {clarity === 'clear' ? 'glass-clear' : ''} {dimmed ? 'dimmed' : ''} {klass}"
  style="--r: {resolvedRadius}; {style}"
  {...rest}
>
  {@render children?.()}
</div>

<style>
  .surface {
    box-sizing: border-box;
    transition: background var(--dur-base) var(--ease-standard),
                box-shadow var(--dur-base) var(--ease-standard),
                border-color var(--dur-base) var(--ease-standard);
  }

  /* Shared sidebar rule across all themes */
  .surface-sidebar {
    background: var(--sidebar-bg);
    border-right: 1px solid var(--sidebar-border);
    backdrop-filter: var(--sidebar-blur);
    -webkit-backdrop-filter: var(--sidebar-blur);
  }

  .appearance-standard.surface-toolbar,
  .appearance-adaptive.surface-toolbar {
    background: var(--surface-overlay);
    border-bottom: 1px solid var(--separator);
  }

  .appearance-standard.surface-panel,
  .appearance-adaptive.surface-panel {
    background: var(--surface-raised);
    border-left: 1px solid var(--separator);
    box-shadow: var(--shadow-1);
  }

  .appearance-standard.surface-floating,
  .appearance-adaptive.surface-floating,
  .appearance-standard.surface-popover,
  .appearance-adaptive.surface-popover {
    background: var(--surface-overlay);
    backdrop-filter: blur(40px);
    -webkit-backdrop-filter: blur(40px);
    box-shadow: var(--shadow-2);
    border: 1px solid var(--separator);
  }

  .appearance-standard.surface-sheet,
  .appearance-adaptive.surface-sheet {
    background: var(--surface-raised);
    box-shadow: var(--shadow-3);
    border: 1px solid var(--separator);
  }

  .appearance-standard.surface-control,
  .appearance-adaptive.surface-control {
    background: var(--fill-1);
  }

  /* ---------------- LIQUID GLASS STYLES ---------------- */


  .appearance-glass.surface-cluster,
  .appearance-glass.surface-floating {
    box-shadow: var(--shadow-2);
  }

  .appearance-glass.surface-control {
    /* Capsules for controls (§8.1 #2) */
    border-radius: 999px;
  }
</style>
