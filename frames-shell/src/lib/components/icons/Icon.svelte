<script lang="ts">
  /**
   * Lucide Icons with stroke 1.75 (§2)
   */
  let {
    name,
    size = 18,
    stroke = 1.75,
    class: klass = ''
  }: {
    name: string;
    size?: number;
    stroke?: number;
    class?: string;
  } = $props();

  const filledIcons = new Set(['heart-filled', 'play-filled']);
</script>

<svg
  class="lucide-icon {klass}"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={filledIcons.has(name) ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === 'photo' || name === 'image'}
    <rect width="18" height="18" x="3" y="3" rx="4" ry="4" />
    <circle cx="9" cy="9" r="2" />
    <path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21" />
  {:else if name === 'album' || name === 'folder'}
    <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
  {:else if name === 'folder-open'}
    <path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2" />
  {:else if name === 'calendar'}
    <path d="M8 2v4" />
    <path d="M16 2v4" />
    <rect width="18" height="18" x="3" y="4" rx="3" />
    <path d="M3 10h18" />
  {:else if name === 'sparkles'}
    <path d="m12 3-1.9 5.8a2 2 0 0 1-1.3 1.3L3 12l5.8 1.9a2 2 0 0 1 1.3 1.3L12 21l1.9-5.8a2 2 0 0 1 1.3-1.3L21 12l-5.8-1.9a2 2 0 0 1-1.3-1.3Z" />
  {:else if name === 'users'}
    <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
    <circle cx="9" cy="7" r="4" />
    <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
    <path d="M16 3.13a4 4 0 0 1 0 7.75" />
  {:else if name === 'compass'}
    <circle cx="12" cy="12" r="10" />
    <polygon points="16.24 7.76 14.12 14.12 7.76 16.24 9.88 9.88 16.24 7.76" />
  {:else if name === 'video'}
    <path d="m16 13 5.223 3.482a.5.5 0 0 0 .777-.416V7.934a.5.5 0 0 0-.777-.416L16 11" />
    <rect width="14" height="12" x="2" y="6" rx="2" />
  {:else if name === 'arrow-down-to-line' || name === 'import'}
    <path d="M12 17V3" />
    <path d="m6 11 6 6 6-6" />
    <path d="M19 21H5" />
  {:else if name === 'copy'}
    <rect width="14" height="14" x="8" y="8" rx="2" ry="2" />
    <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
  {:else if name === 'eye-off'}
    <path d="M10.733 5.076a10.744 10.744 0 0 1 11.205 6.575 1 1 0 0 1 0 .698 10.747 10.747 0 0 1-5.187 5.76" />
    <path d="M14.084 14.158a3 3 0 0 1-4.242-4.242" />
    <path d="M17.479 17.499A10.75 10.75 0 0 1 2.062 12.35a1 1 0 0 1 0-.698 10.75 10.75 0 0 1 2.883-4.32" />
    <line x1="2" x2="22" y1="2" y2="22" />
  {:else if name === 'trash'}
    <path d="M3 6h18" />
    <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
    <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
    <line x1="10" x2="10" y1="11" y2="17" />
    <line x1="14" x2="14" y1="11" y2="17" />
  {:else if name === 'camera'}
    <path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z" />
    <circle cx="12" cy="13" r="3" />
  {:else if name === 'cloud'}
    <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />
  {:else if name === 'cloud-check'}
    <path d="m9 15 2 2 4-4" />
    <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />
  {:else if name === 'cloud-upload'}
    <path d="M12 13v6" />
    <path d="m8 16 4-4 4 4" />
    <path d="M17.5 19H9a7 7 0 1 1 6.71-9h1.79a4.5 4.5 0 1 1 0 9Z" />
  {:else if name === 'panel-left' || name === 'sidebar'}
    <rect width="18" height="18" x="3" y="3" rx="3" />
    <path d="M9 3v18" />
  {:else if name === 'chevron-left'}
    <path d="m15 18-6-6 6-6" />
  {:else if name === 'chevron-right'}
    <path d="m9 18 6-6-6-6" />
  {:else if name === 'chevron-down'}
    <path d="m6 9 6 6 6-6" />
  {:else if name === 'chevron-up'}
    <path d="m18 15-6-6-6 6" />
  {:else if name === 'zoom-in'}
    <circle cx="11" cy="11" r="8" />
    <line x1="21" x2="16.65" y1="21" y2="16.65" />
    <line x1="11" x2="11" y1="8" y2="14" />
    <line x1="8" x2="14" y1="11" y2="11" />
  {:else if name === 'zoom-out'}
    <circle cx="11" cy="11" r="8" />
    <line x1="21" x2="16.65" y1="21" y2="16.65" />
    <line x1="8" x2="14" y1="11" y2="11" />
  {:else if name === 'filter'}
    <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
  {:else if name === 'fill' || name === 'square'}
    <rect width="18" height="18" x="3" y="3" rx="2" />
  {:else if name === 'fit' || name === 'maximize-2'}
    <polyline points="15 3 21 3 21 9" />
    <polyline points="9 21 3 21 3 15" />
    <line x1="21" x2="14" y1="3" y2="10" />
    <line x1="3" x2="10" y1="21" y2="14" />
  {:else if name === 'info'}
    <circle cx="12" cy="12" r="10" />
    <path d="M12 16v-4" />
    <path d="M12 8h.01" />
  {:else if name === 'share'}
    <path d="M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8" />
    <polyline points="16 6 12 2 8 6" />
    <line x1="12" x2="12" y1="2" y2="15" />
  {:else if name === 'heart'}
    <path d="M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z" />
  {:else if name === 'heart-filled'}
    <path d="M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z" />
  {:else if name === 'rotate-cw'}
    <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
    <path d="M21 3v5h-5" />
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="8" />
    <path d="m21 21-4.3-4.3" />
  {:else if name === 'edit'}
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
  {:else if name === 'more-horizontal'}
    <circle cx="12" cy="12" r="1" />
    <circle cx="19" cy="12" r="1" />
    <circle cx="5" cy="12" r="1" />
  {:else if name === 'play'}
    <polygon points="6 3 20 12 6 21 6 3" />
  {:else if name === 'play-filled'}
    <polygon points="6 3 20 12 6 21 6 3" />
  {:else if name === 'pause'}
    <rect width="4" height="16" x="6" y="4" />
    <rect width="4" height="16" x="14" y="4" />
  {:else if name === 'volume-2'}
    <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
    <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
    <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
  {:else if name === 'volume-x'}
    <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5" />
    <line x1="22" x2="16" y1="9" y2="15" />
    <line x1="16" x2="22" y1="9" y2="15" />
  {:else if name === 'repeat'}
    <path d="m17 2 4 4-4 4" />
    <path d="M3 11v-1a4 4 0 0 1 4-4h14" />
    <path d="m7 22-4-4 4-4" />
    <path d="M21 13v1a4 4 0 0 1-4 4H3" />
  {:else if name === 'check'}
    <polyline points="20 6 9 17 4 12" />
  {:else if name === 'x'}
    <line x1="18" x2="6" y1="6" y2="18" />
    <line x1="6" x2="18" y1="6" y2="18" />
  {:else if name === 'minus'}
    <line x1="5" x2="19" y1="12" y2="12" />
  {:else if name === 'plus'}
    <path d="M5 12h14" />
    <path d="M12 5v14" />
  {:else if name === 'sun'}
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2" />
    <path d="M12 20v2" />
    <path d="m4.93 4.93 1.41 1.41" />
    <path d="m17.66 17.66 1.41 1.41" />
    <path d="M2 12h2" />
    <path d="M20 12h2" />
    <path d="m6.34 17.66-1.41 1.41" />
    <path d="m19.07 4.93-1.41 1.41" />
  {:else if name === 'moon'}
    <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />
  {:else if name === 'laptop'}
    <path d="M20 16V7a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v9m16 0H4m16 0 1.28 2.55a1 1 0 0 1-.9 1.45H3.62a1 1 0 0 1-.9-1.45L4 16" />
  {:else if name === 'settings'}
    <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === 'layers'}
    <polygon points="12 2 2 7 12 12 22 7 12 2" />
    <polyline points="2 17 12 22 22 17" />
    <polyline points="2 12 12 17 22 12" />
  {:else}
    <!-- Fallback circle -->
    <circle cx="12" cy="12" r="8" />
  {/if}
</svg>
