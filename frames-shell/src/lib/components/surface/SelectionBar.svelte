<script lang="ts">
  import Surface from './Surface.svelte';
  import Button from './Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import Menu, { type MenuItem } from './Menu.svelte';

  let {
    selectedCount = 0,
    onshare,
    onfavorite,
    onaddtoalbum,
    ondelete,
    onclear
  }: {
    selectedCount: number;
    onshare?: () => void;
    onfavorite?: () => void;
    onaddtoalbum?: () => void;
    ondelete?: () => void;
    onclear?: () => void;
  } = $props();

  let moreMenuOpen = $state(false);

  const moreItems: MenuItem[] = [
    { id: 'hide', label: 'Hide Photos', icon: 'eye-off' },
    { id: 'adjust-date', label: 'Adjust Date & Time…', icon: 'calendar' },
    { id: 'duplicate', label: 'Duplicate 1 Photo', icon: 'copy' },
    { separator: true, id: 'sep1', label: '' },
    { id: 'delete', label: 'Delete from Library', icon: 'trash', danger: true }
  ];

  function handleMoreSelect(item: MenuItem) {
    if (item.id === 'delete') {
      ondelete?.();
    }
  }
</script>

{#if selectedCount > 0}
  <div class="selection-bar-wrapper">
    <Surface kind="floating" class="selection-bar-surface">
      <span class="selection-count-pill">{selectedCount} selected</span>

      <div class="bar-divider"></div>

      <Button variant="ghost" title="Share" onclick={onshare}>
        <Icon name="share" size={15} />
        <span>Share</span>
      </Button>

      <Button variant="ghost" title="Favorite" onclick={onfavorite}>
        <Icon name="heart" size={15} />
        <span>Favorite</span>
      </Button>

      <Button variant="ghost" title="Add to Album" onclick={onaddtoalbum}>
        <Icon name="album" size={15} />
        <span>Add to Album</span>
      </Button>

      <Button variant="ghost" title="Delete" onclick={ondelete}>
        <Icon name="trash" size={15} />
        <span>Delete</span>
      </Button>

      <div class="relative-wrap">
        <Button variant="icon" title="More options" onclick={() => (moreMenuOpen = !moreMenuOpen)}>
          <Icon name="more-horizontal" size={15} />
        </Button>
        <Menu
          items={moreItems}
          bind:open={moreMenuOpen}
          onselect={handleMoreSelect}
          style="bottom: 44px; right: 0;"
        />
      </div>

      <Button variant="icon" title="Clear selection (Esc)" onclick={onclear}>
        <Icon name="x" size={14} />
      </Button>
    </Surface>
  </div>
{/if}

<style>
  .selection-bar-wrapper {
    position: fixed;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 120;
    pointer-events: auto;
    animation: barSlideUp var(--dur-base) var(--ease-overshoot);
  }

  @keyframes barSlideUp {
    from {
      opacity: 0;
      transform: translate(-50%, 16px);
    }
    to {
      opacity: 1;
      transform: translate(-50%, 0);
    }
  }

  :global(.selection-bar-surface) {
    display: inline-flex;
    align-items: center;
    height: 42px;
    padding: 0 10px;
    gap: 6px;
    border-radius: var(--r-pill);
    box-shadow: var(--shadow-2);
  }

  .selection-count-pill {
    font-size: var(--text-size-body);
    font-weight: 600;
    padding: 0 8px;
    color: var(--text-1);
    white-space: nowrap;
  }

  .bar-divider {
    width: 1px;
    height: 18px;
    background: var(--separator);
    margin: 0 2px;
  }

  .relative-wrap {
    position: relative;
  }
</style>
