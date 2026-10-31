<script lang="ts">
  import Surface from '$lib/components/surface/Surface.svelte';
  import Button from '$lib/components/surface/Button.svelte';
  import Icon from '$lib/components/icons/Icon.svelte';
  import { tauriRepo, type BackendSettingsView } from '$lib/services/tauriRepo';

  let {
    onclose,
    onsaved,
    hint = ''
  }: {
    onclose?: () => void;
    onsaved?: (view: BackendSettingsView) => void;
    hint?: string;
  } = $props();

  const initial = tauriRepo.settings;

  let endpoint = $state(initial?.endpoint ?? 'https://s3.eu-central-1.s4.mega.io');
  let region = $state(initial?.region ?? 'eu-central-1');
  let bucket = $state(initial?.bucket ?? 'frames-library');
  let accessKeyId = $state(initial?.access_key_id ?? '');
  let secret = $state('');
  let hasSecret = $state(initial?.has_secret_in_keychain ?? false);
  let useRealS4 = $state(initial?.use_real_s4 ?? true);
  let saving = $state(false);
  let error = $state('');

  async function save() {
    error = '';
    saving = true;
    try {
      const cur = tauriRepo.settings;
      const view = await tauriRepo.saveSettings({
        endpoint,
        region,
        bucket,
        access_key_id: accessKeyId,
        secret_access_key: secret.trim() ? secret : null,
        space_limit_gb: cur?.space_limit_gb ?? 750,
        cache_location: cur?.cache_location ?? '',
        cache_limit_gb: cur?.cache_limit_gb ?? 10,
        use_real_s4: useRealS4
      });
      secret = '';
      hasSecret = view.has_secret_in_keychain;
      onsaved?.(view);
    } catch (e: any) {
      error = e?.toString?.() ?? 'Failed to save settings';
    } finally {
      saving = false;
    }
  }
</script>

<div class="conn-backdrop">
  <Surface kind="sheet" class="conn-sheet">
    <div class="conn-header">
      <div>
        <h2 class="conn-title">MEGA S4 Connection</h2>
        <span class="conn-sub">Your library lives in a MEGA S4 bucket. Keys stay in the OS keychain.</span>
      </div>
      <Button variant="icon" title="Close" onclick={onclose}><Icon name="x" size={15} /></Button>
    </div>

    <div class="conn-body">
      {#if hint}<div class="conn-hint">{hint}</div>{/if}

      <label class="conn-field">
        <span>Access Key ID</span>
        <input type="text" bind:value={accessKeyId} autocomplete="off" spellcheck="false" />
      </label>
      <label class="conn-field">
        <span>Secret Access Key {hasSecret ? '(saved — leave blank to keep)' : ''}</span>
        <input
          type="password"
          bind:value={secret}
          autocomplete="off"
          placeholder={hasSecret ? '••••••••••••' : ''}
        />
      </label>
      <label class="conn-field">
        <span>Bucket</span>
        <input type="text" bind:value={bucket} spellcheck="false" />
      </label>
      <label class="conn-field">
        <span>Endpoint</span>
        <input type="text" bind:value={endpoint} spellcheck="false" />
      </label>
      <label class="conn-field">
        <span>Region</span>
        <input type="text" bind:value={region} spellcheck="false" />
      </label>
      <label class="conn-check">
        <input type="checkbox" bind:checked={useRealS4} />
        <span>Use real MEGA S4 (turn off for an in-memory local sandbox)</span>
      </label>

      {#if error}<div class="conn-error">{error}</div>{/if}
    </div>

    <div class="conn-footer">
      <Button variant="ghost" onclick={onclose}>Cancel</Button>
      <Button variant="accent" disabled={saving} onclick={save}>
        {saving ? 'Saving…' : 'Save & Connect'}
      </Button>
    </div>
  </Surface>
</div>

<style>
  .conn-backdrop {
    position: fixed;
    inset: 0;
    z-index: 750;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(10px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
  }
  :global(.conn-sheet) {
    width: 520px;
    max-width: 95vw;
    max-height: 90vh;
    border-radius: var(--r-xl);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .conn-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 20px;
    border-bottom: 1px solid var(--separator);
  }
  .conn-title {
    margin: 0;
    font-size: var(--text-size-title);
    font-weight: 700;
    color: var(--text-1);
  }
  .conn-sub {
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }
  .conn-body {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    overflow-y: auto;
    user-select: text;
    -webkit-user-select: text;
  }
  .conn-field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: var(--text-size-caption);
    font-weight: 600;
    color: var(--text-2);
  }
  .conn-field input[type='text'],
  .conn-field input[type='password'] {
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid var(--separator);
    background: var(--fill-1);
    color: var(--text-1);
    font: inherit;
    font-weight: 400;
    outline: none;
  }
  .conn-field input:focus {
    border-color: var(--accent, #007aff);
  }
  .conn-check {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: var(--text-size-caption);
    color: var(--text-2);
  }
  .conn-hint {
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--fill-1);
    color: var(--text-1);
    font-size: var(--text-size-caption);
  }
  .conn-error {
    color: #ff3b30;
    font-size: var(--text-size-caption);
    word-break: break-word;
  }
  .conn-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px;
    border-top: 1px solid var(--separator);
  }
</style>
