<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { theme } from '$lib/theme/engine.svelte';

  let {
    variant = 'pill',
    icon = '',
    text = '',
    class: klass = '',
    children
  }: {
    variant?: 'pill' | 'cloud' | 'video' | 'favorite' | 'count';
    icon?: string;
    text?: string;
    class?: string;
    children?: Snippet;
  } = $props();
</script>

<span class="badge badge-{variant} appearance-{theme.appearance} {klass}">
  {#if icon}
    <Icon name={icon} size={12} />
  {/if}
  {#if text}
    <span>{text}</span>
  {/if}
  {@render children?.()}
</span>

<style>
  .badge {
    box-sizing: border-box;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    border-radius: var(--r-pill);
    font-family: var(--font-stack);
    font-size: var(--text-size-caption);
    font-weight: 500;
    line-height: var(--text-lh-caption);
    user-select: none;
    pointer-events: none;
  }

  .badge-video {
    background: rgba(0, 0, 0, 0.65);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    color: #ffffff;
    font-family: var(--font-mono);
    font-size: 10px;
    padding: 2px 5px;
  }

  .badge-cloud {
    background: rgba(0, 0, 0, 0.50);
    backdrop-filter: blur(8px);
    -webkit-backdrop-filter: blur(8px);
    color: rgba(255, 255, 255, 0.9);
    padding: 3px;
  }

  .badge-favorite {
    color: var(--favorite);
    filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.4));
    padding: 0;
  }

  .badge-count {
    background: var(--fill-2);
    color: var(--text-2);
    font-size: 11px;
    font-weight: 600;
    padding: 1px 6px;
  }

  .badge-pill {
    background: var(--fill-2);
    color: var(--text-1);
  }
</style>
