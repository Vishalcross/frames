<script lang="ts">
  import Surface from './Surface.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';

  export interface MenuItem {
    id: string;
    label: string;
    icon?: string;
    checked?: boolean;
    disabled?: boolean;
    danger?: boolean;
    shortcut?: string;
    separator?: boolean;
  }

  let {
    items,
    open = $bindable(false),
    onselect,
    class: klass = '',
    style = ''
  }: {
    items: MenuItem[];
    open: boolean;
    onselect?: (item: MenuItem) => void;
    class?: string;
    style?: string;
  } = $props();

  function selectItem(item: MenuItem) {
    if (item.disabled || item.separator) return;
    onselect?.(item);
    open = false;
  }
</script>

{#if open}
  <!-- Backdrop click catcher -->
  <div class="menu-backdrop" onclick={() => (open = false)} role="presentation"></div>

  <Surface kind="popover" class="menu-popover {klass}" {style}>
    <div class="menu-list" role="menu">
      {#each items as item}
        {#if item.separator}
          <div class="menu-separator" role="separator"></div>
        {:else}
          <button
            type="button"
            role="menuitem"
            class="menu-item {item.checked ? 'is-checked' : ''} {item.danger ? 'is-danger' : ''}"
            disabled={item.disabled}
            onclick={() => selectItem(item)}
          >
            <span class="item-check">
              {#if item.checked}
                <Icon name="check" size={12} stroke={2.2} />
              {/if}
            </span>

            {#if item.icon}
              <span class="item-icon">
                <Icon name={item.icon} size={15} />
              </span>
            {/if}

            <span class="item-label">{item.label}</span>

            {#if item.shortcut}
              <span class="item-shortcut">{item.shortcut}</span>
            {/if}
          </button>
        {/if}
      {/each}
    </div>
  </Surface>
{/if}

<style>
  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 998;
    background: transparent;
  }

  :global(.menu-popover) {
    position: absolute;
    z-index: 999;
    min-width: 180px;
    padding: 5px;
    border-radius: var(--r-md);
  }

  .menu-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .menu-item {
    display: flex;
    align-items: center;
    width: 100%;
    min-height: 26px;
    padding: 4px 8px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-1);
    font-family: var(--font-stack);
    font-size: var(--text-size-body);
    font-weight: 500;
    line-height: var(--text-lh-body);
    text-align: left;
    cursor: pointer;
    user-select: none;
    gap: 8px;
    transition: background var(--dur-fast) var(--ease-standard);
  }

  .menu-item:hover:not(:disabled) {
    background: var(--fill-1);
  }

  .menu-item:disabled {
    opacity: 0.38;
    cursor: default;
  }

  .item-check {
    width: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
  }

  .item-icon {
    display: flex;
    align-items: center;
    color: var(--text-2);
  }

  .item-label {
    flex: 1;
  }

  .item-shortcut {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-3);
  }

  .is-danger {
    color: var(--danger);
  }

  .menu-separator {
    height: 1px;
    background: var(--separator);
    margin: 4px 6px;
  }
</style>
