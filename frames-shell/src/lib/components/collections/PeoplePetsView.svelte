<script lang="ts">
  import Icon from '$lib/components/icons/Icon.svelte';

  let {
    onselectperson
  }: {
    onselectperson?: (id: string) => void;
  } = $props();

  let people = $state<any[]>([]);
</script>

<div class="people-view-container">
  <header class="people-header">
    <h2 class="people-title">People & Pets</h2>
    <span class="people-count">{people.length} individuals identified</span>
  </header>

  {#if people.length === 0}
    <div class="people-empty">
      <div class="empty-icon-wrap">
        <Icon name="users" size={32} />
      </div>
      <p class="empty-title">No People or Pets Yet</p>
      <p class="empty-sub">Import photos to automatically recognize recurring faces and pets.</p>
    </div>
  {:else}
    <div class="people-grid">
      {#each people as person}
        <button
          type="button"
          class="person-card"
          onclick={() => onselectperson?.(person.id)}
        >
          <div class="avatar-ring">
            <img src={person.avatar} alt={person.title} loading="lazy" />
          </div>
          <div class="person-details">
            <span class="person-name">{person.title}</span>
            <span class="person-photos">{person.subtitle}</span>
          </div>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .people-view-container {
    width: 100%;
    height: 100%;
    overflow-y: auto;
    padding: calc(var(--toolbar-h) + 20px) 24px 32px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .people-header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    border-bottom: 1px solid var(--separator);
    padding-bottom: 12px;
  }

  .people-title {
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
    margin: 0;
  }

  .people-count {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }

  .people-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 80px 20px;
    text-align: center;
    gap: 12px;
  }

  .empty-icon-wrap {
    width: 64px;
    height: 64px;
    border-radius: 50%;
    background: var(--fill-2);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
  }

  .empty-title {
    font-size: var(--text-size-title);
    font-weight: 600;
    color: var(--text-1);
    margin: 0;
  }

  .empty-sub {
    font-size: var(--text-size-body);
    color: var(--text-2);
    margin: 0;
    max-width: 380px;
  }

  .people-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 20px;
  }

  .person-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    background: transparent;
    border: none;
    padding: 8px;
    cursor: pointer;
    border-radius: var(--r-lg);
    transition: background var(--dur-fast) var(--ease-standard);
  }

  .person-card:hover {
    background: var(--fill-1);
  }

  .avatar-ring {
    width: 96px;
    height: 96px;
    border-radius: 50%;
    overflow: hidden;
    background: var(--fill-2);
    box-shadow: var(--shadow-1);
  }

  .avatar-ring img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .person-details {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }

  .person-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-1);
  }

  .person-photos {
    font-size: 12px;
    color: var(--text-2);
  }
</style>
