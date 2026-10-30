<script lang="ts">
  /**
   * Minimal stroke-icon set (24px grid, SF-Symbols-like weight).
   * Usage: <Icon name="photo" size={18} />
   */
  let {
    name,
    size = 18,
    stroke = 1.7,
    class: klass = ""
  }: { name: string; size?: number; stroke?: number; class?: string } = $props();

  const FILLED = new Set(["play", "live-dot", "pause"]);
</script>

<svg
  class="icon {klass}"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill={FILLED.has(name) ? "currentColor" : "none"}
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "photo"}
    <rect x="3" y="3" width="18" height="18" rx="4" />
    <circle cx="8.6" cy="8.6" r="1.6" />
    <path d="m21 15-4.6-4.6a1.5 1.5 0 0 0-2.1 0L5 19.7" />
  {:else if name === "album"}
    <rect x="7" y="3" width="14" height="14" rx="3" />
    <path d="M4 7.5V18a3 3 0 0 0 3 3h10.5" />
    <path d="m21 13-3.2-3.2a1.2 1.2 0 0 0-1.7 0L11 15" />
  {:else if name === "import"}
    <path d="M21 15v3.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 18.5V15" />
    <path d="m7.5 10.5 4.5 4.5 4.5-4.5" />
    <path d="M12 15V3" />
  {:else if name === "share"}
    <path d="M4 12v6.5A2.5 2.5 0 0 0 6.5 21h11a2.5 2.5 0 0 0 2.5-2.5V12" />
    <path d="m16 7-4-4-4 4" />
    <path d="M12 3v12" />
  {:else if name === "trash"}
    <path d="M3.5 6h17" />
    <path d="M18.5 6v13a2 2 0 0 1-2 2h-9a2 2 0 0 1-2-2V6" />
    <path d="M8.5 6V4.5A1.5 1.5 0 0 1 10 3h4a1.5 1.5 0 0 1 1.5 1.5V6" />
    <path d="M10 10.5v6M14 10.5v6" />
  {:else if name === "settings"}
    <path d="M12.2 2h-.4a2 2 0 0 0-2 2v.2a2 2 0 0 1-1 1.7l-.4.3a2 2 0 0 1-2 0l-.2-.1a2 2 0 0 0-2.7.7l-.2.4a2 2 0 0 0 .7 2.7l.2.1a2 2 0 0 1 1 1.7v.5a2 2 0 0 1-1 1.7l-.2.1a2 2 0 0 0-.7 2.7l.2.4a2 2 0 0 0 2.7.7l.2-.1a2 2 0 0 1 2 0l.4.3a2 2 0 0 1 1 1.7v.2a2 2 0 0 0 2 2h.4a2 2 0 0 0 2-2v-.2a2 2 0 0 1 1-1.7l.4-.3a2 2 0 0 1 2 0l.2.1a2 2 0 0 0 2.7-.7l.2-.4a2 2 0 0 0-.7-2.7l-.2-.1a2 2 0 0 1-1-1.7v-.5a2 2 0 0 1 1-1.7l.2-.1a2 2 0 0 0 .7-2.7l-.2-.4a2 2 0 0 0-2.7-.7l-.2.1a2 2 0 0 1-2 0l-.4-.3a2 2 0 0 1-1-1.7V4a2 2 0 0 0-2-2Z" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === "search"}
    <circle cx="11" cy="11" r="7" />
    <path d="m20.5 20.5-4.6-4.6" />
  {:else if name === "refresh"}
    <path d="M3 12a9 9 0 0 1 15.3-6.4L21 8" />
    <path d="M21 3v5h-5" />
    <path d="M21 12a9 9 0 0 1-15.3 6.4L3 16" />
    <path d="M3 21v-5h5" />
  {:else if name === "cloud"}
    <path d="M17.5 19H8a6 6 0 1 1 5.6-8.2 4.6 4.6 0 0 1 3.9 8.2Z" />
  {:else if name === "cloud-off"}
    <path d="M8 19a6 6 0 0 1-1-11.9" />
    <path d="M12.6 7.2A6 6 0 0 1 17.7 11a4.6 4.6 0 0 1 2.7 7.2" />
    <path d="M17.5 19H9" />
    <path d="m3 3 18 18" />
  {:else if name === "check"}
    <path d="m4.5 12.5 5 5 10-11" />
  {:else if name === "play"}
    <path d="M7 4.6a1 1 0 0 1 1.5-.86l11 7.4a1 1 0 0 1 0 1.72l-11 7.4A1 1 0 0 1 7 19.4Z" stroke-width="1" />
  {:else if name === "pause"}
    <rect x="6" y="4" width="4.2" height="16" rx="1.4" stroke-width="0" />
    <rect x="13.8" y="4" width="4.2" height="16" rx="1.4" stroke-width="0" />
  {:else if name === "live"}
    <circle cx="12" cy="12" r="2.2" fill="currentColor" stroke="none" />
    <circle cx="12" cy="12" r="5.6" />
    <circle cx="12" cy="12" r="9.4" stroke-dasharray="1.6 2.6" />
  {:else if name === "info"}
    <circle cx="12" cy="12" r="9.5" />
    <path d="M12 16.5v-5" />
    <path d="M12 7.7h.01" stroke-width="2.4" />
  {:else if name === "x"}
    <path d="M18 6 6 18M6 6l12 12" />
  {:else if name === "chevron-left"}
    <path d="m14.5 18-6-6 6-6" />
  {:else if name === "chevron-right"}
    <path d="m9.5 18 6-6-6-6" />
  {:else if name === "chevron-down"}
    <path d="m6 9.5 6 6 6-6" />
  {:else if name === "plus"}
    <path d="M5 12h14M12 5v14" />
  {:else if name === "minus"}
    <path d="M5 12h14" />
  {:else if name === "zoom-in"}
    <circle cx="11" cy="11" r="7" />
    <path d="m20.5 20.5-4.6-4.6M8 11h6M11 8v6" />
  {:else if name === "zoom-out"}
    <circle cx="11" cy="11" r="7" />
    <path d="m20.5 20.5-4.6-4.6M8 11h6" />
  {:else if name === "sun"}
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
  {:else if name === "moon"}
    <path d="M20.5 14.2A8.5 8.5 0 0 1 9.8 3.5a8.5 8.5 0 1 0 10.7 10.7Z" />
  {:else if name === "monitor"}
    <rect x="2.5" y="3.5" width="19" height="13" rx="2.2" />
    <path d="M8 21h8M12 16.5V21" />
  {:else if name === "palette"}
    <path d="M12 2.5a9.5 9.5 0 1 0 0 19c1 0 1.7-.8 1.7-1.7 0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2 0-.9.8-1.7 1.7-1.7h2c3 0 5.5-2.4 5.5-5.4 0-4.8-4.6-7.8-9.9-7.8Z" />
    <circle cx="7.6" cy="11.6" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="10.2" cy="7.4" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="15.2" cy="7.4" r="1.1" fill="currentColor" stroke="none" />
  {:else if name === "sparkles"}
    <path d="m12 3 1.7 4.6L18.3 9.3l-4.6 1.7L12 15.6 10.3 11 5.7 9.3l4.6-1.7Z" />
    <path d="m18.5 15 .8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8Z" />
  {:else if name === "warning"}
    <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" />
    <path d="M12 9.5v4" />
    <path d="M12 17.2h.01" stroke-width="2.4" />
  {:else if name === "restore"}
    <path d="M3 12a9 9 0 1 0 2.9-6.6L3 8" />
    <path d="M3 3v5h5" />
  {:else if name === "database"}
    <ellipse cx="12" cy="5.5" rx="8" ry="3" />
    <path d="M4 5.5v13c0 1.7 3.6 3 8 3s8-1.3 8-3v-13" />
    <path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3" />
  {:else if name === "move"}
    <path d="M8 3 4 7l4 4M4 7h16" />
    <path d="m16 21 4-4-4-4M20 17H4" />
  {:else if name === "folder"}
    <path d="M20.5 19.5a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.6a2 2 0 0 1-1.7-.9L10.1 3.5A2 2 0 0 0 8.4 2.6H3.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" transform="translate(0 .4)" />
  {:else if name === "calendar"}
    <rect x="3" y="4.5" width="18" height="16.5" rx="3" />
    <path d="M3 10h18M8 2.5v4M16 2.5v4" />
  {:else if name === "camera"}
    <path d="M3 8.5A2.5 2.5 0 0 1 5.5 6h1.2a2 2 0 0 0 1.7-1l.4-.7A2 2 0 0 1 10.5 3.3h3a2 2 0 0 1 1.7 1l.4.7a2 2 0 0 0 1.7 1h1.2A2.5 2.5 0 0 1 21 8.5v9a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5Z" />
    <circle cx="12" cy="13" r="3.8" />
  {:else if name === "film"}
    <rect x="3" y="3.5" width="18" height="17" rx="3" />
    <path d="M7 3.5v17M17 3.5v17M3 9h4M3 15h4M17 9h4M17 15h4" />
  {:else if name === "file"}
    <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z" />
    <path d="M14 3v5h5" />
  {:else if name === "image-off"}
    <rect x="3" y="3" width="18" height="18" rx="4" />
    <path d="m3 3 18 18" />
  {:else if name === "wifi-off"}
    <path d="M2 8.8a15 15 0 0 1 4.2-2.7M22 8.8A15 15 0 0 0 11 5.1M5 12.9a10 10 0 0 1 3.2-2M19 12.9a10 10 0 0 0-3.4-2.1M8.5 16.4a5 5 0 0 1 7 0" />
    <path d="M12 20h.01" stroke-width="2.4" />
    <path d="m2 2 20 20" />
  {:else if name === "wifi"}
    <path d="M2 8.8a15 15 0 0 1 20 0M5 12.9a10 10 0 0 1 14 0M8.5 16.4a5 5 0 0 1 7 0" />
    <path d="M12 20h.01" stroke-width="2.4" />
  {:else if name === "ellipsis"}
    <circle cx="5.5" cy="12" r="1.2" fill="currentColor" />
    <circle cx="12" cy="12" r="1.2" fill="currentColor" />
    <circle cx="18.5" cy="12" r="1.2" fill="currentColor" />
  {:else if name === "pencil"}
    <path d="M12 20h9" />
    <path d="M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z" />
  {:else if name === "inbox"}
    <path d="M3 13.5 6 5.5A2 2 0 0 1 7.9 4h8.2A2 2 0 0 1 18 5.5l3 8" />
    <path d="M3 13.5V18a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-4.5h-5.2a1.5 1.5 0 0 0-1.3.8l-.3.5a1.5 1.5 0 0 1-1.3.7h-1.8a1.5 1.5 0 0 1-1.3-.7l-.3-.5a1.5 1.5 0 0 0-1.3-.8Z" />
  {/if}
</svg>

<style>
  .icon {
    flex: none;
    display: inline-block;
    vertical-align: middle;
  }
</style>
