<script lang="ts">
  import { theme } from '$lib/theme/engine.svelte';
</script>

{#if theme.appearance === 'adaptive'}
  <div class="ambient-backdrop" aria-hidden="true">
    <div class="ambient-layer ambient-1"></div>
    <div class="ambient-layer ambient-2"></div>
    <!-- 2% noise layer against banding (§7.5) -->
    <div class="noise-layer"></div>
  </div>
{/if}

<style>
  .ambient-backdrop {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 0;
    overflow: hidden;
  }

  .ambient-layer {
    position: absolute;
    border-radius: 50%;
    filter: blur(80px);
    transition: background var(--dur-ambient) var(--ease-standard),
                transform var(--dur-ambient) var(--ease-standard);
  }

  .ambient-1 {
    top: -15%;
    left: -10%;
    width: 65vw;
    height: 55vh;
    background: var(--ad-ambient-1, rgba(0, 122, 255, 0.35));
  }

  .ambient-2 {
    bottom: -15%;
    right: -10%;
    width: 70vw;
    height: 60vh;
    background: var(--ad-ambient-2, rgba(88, 86, 214, 0.28));
  }

  .noise-layer {
    position: absolute;
    inset: 0;
    opacity: 0.02;
    background-image: url("data:image/svg+xml,%3Csvg viewBox='0 0 200 200' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noiseFilter'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.8' numOctaves='3' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noiseFilter)'/%3E%3C/svg%3E");
    background-repeat: repeat;
  }
</style>
