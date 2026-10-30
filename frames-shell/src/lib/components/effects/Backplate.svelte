<script lang="ts">
  import { theme, resolved } from '$lib/theme/engine.svelte';

  const c1 = $derived(
    resolved.palette ? `oklch(${resolved.palette.seed.l * 100}% ${resolved.palette.seed.c} ${resolved.palette.seed.h})` : 'rgba(0, 122, 255, 0.4)'
  );
  const c2 = $derived(
    resolved.palette ? `oklch(${resolved.palette.secondary.l * 100}% ${resolved.palette.secondary.c} ${resolved.palette.secondary.h})` : 'rgba(88, 86, 214, 0.35)'
  );
</script>

{#if theme.appearance === 'glass'}
  <div class="sidebar-backplate" aria-hidden="true">
    <div class="backplate-mosaic" style="--bc1: {c1}; --bc2: {c2};"></div>
  </div>
{/if}

<style>
  .sidebar-backplate {
    position: absolute;
    top: 8px;
    left: 8px;
    width: var(--sidebar-w, 240px);
    height: calc(100vh - 16px);
    border-radius: 20px;
    overflow: hidden;
    pointer-events: none;
    z-index: 5;
    opacity: 0.22;
  }

  .backplate-mosaic {
    width: 100%;
    height: 100%;
    background:
      radial-gradient(circle at 20% 30%, var(--bc1) 0%, transparent 60%),
      radial-gradient(circle at 80% 70%, var(--bc2) 0%, transparent 60%),
      radial-gradient(circle at 50% 90%, var(--bc1) 0%, transparent 50%);
    filter: blur(40px);
    transform: scale(1.2);
  }
</style>
