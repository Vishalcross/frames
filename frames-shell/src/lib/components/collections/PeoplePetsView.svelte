<script lang="ts">
  import { SAMPLE_PEOPLE } from '$lib/mock/sampleData';
  import { getThumbnailDataUrl, getAssetByIndex } from '$lib/mock/mockRepo';

  let {
    onselectperson
  }: {
    onselectperson?: (id: string) => void;
  } = $props();

  function getAvatar(keyAssetId: string) {
    const idx = parseInt(keyAssetId.replace('asset_', ''), 10) || 0;
    return getThumbnailDataUrl(getAssetByIndex(idx), 'thumb');
  }
</script>

<div class="people-view-container">
  <header class="people-header">
    <h2 class="people-title">People & Pets</h2>
    <span class="people-count">{SAMPLE_PEOPLE.length} individuals identified</span>
  </header>

  <div class="people-grid">
    {#each SAMPLE_PEOPLE as person}
      <button
        type="button"
        class="person-card"
        onclick={() => onselectperson?.(person.id)}
      >
        <div class="avatar-ring">
          <img src={getAvatar(person.keyAssetId)} alt={person.title} loading="lazy" />
        </div>
        <div class="person-details">
          <span class="person-name">{person.title}</span>
          <span class="person-photos">{person.subtitle}</span>
        </div>
      </button>
    {/each}
  </div>
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

  .people-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 24px;
  }

  .person-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    border: none;
    background: transparent;
    cursor: pointer;
    gap: 10px;
    padding: 8px;
    border-radius: var(--r-md);
    transition: background var(--dur-fast) var(--ease-standard),
                transform var(--dur-fast) var(--ease-standard);
  }
  .person-card:hover {
    background: var(--fill-1);
    transform: translateY(-2px);
  }

  .avatar-ring {
    width: 110px;
    height: 110px;
    border-radius: 50%;
    overflow: hidden;
    box-shadow: var(--shadow-1);
    background: var(--fill-2);
  }
  .avatar-ring img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .person-details {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .person-name {
    font-size: var(--text-size-body);
    font-weight: 600;
    color: var(--text-1);
  }

  .person-photos {
    font-size: 11px;
    color: var(--text-3);
  }
</style>
