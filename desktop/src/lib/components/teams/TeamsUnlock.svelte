<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Lock } from "lucide-svelte";
  import type { TeamsIdentity, TeamsStatus } from "$lib/types";

  let {
    identity,
    onUnlocked,
  }: { identity: TeamsIdentity; onUnlocked: (status: TeamsStatus) => void } = $props();

  let passphrase = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function unlock() {
    if (!passphrase || busy) return;
    busy = true;
    error = null;
    try {
      const next = await invoke<TeamsStatus>("teams_unlock", { passphrase });
      passphrase = "";
      onUnlocked(next);
    } catch (err) {
      const text = String(err);
      error = /passphrase/i.test(text) ? "That passphrase didn't unlock this device's keys." : `Couldn't unlock: ${text}`;
    } finally {
      busy = false;
    }
  }
</script>

<form
  class="panel mx-auto flex max-w-md flex-col gap-4 p-5"
  onsubmit={(e) => {
    e.preventDefault();
    void unlock();
  }}
>
  <div class="flex items-center gap-2">
    <Lock size={16} />
    <h2 class="m-0 text-base font-semibold text-primary">Unlock Teams</h2>
  </div>
  <p class="m-0 type-body text-secondary">{identity.email} on {identity.server_url}</p>
  <p class="m-0 type-meta">
    Team hosts still connect while locked; session events are signed at the next unlock.
  </p>
  <div class="field">
    <label class="field-label" for="teams-unlock-pass">Teams passphrase</label>
    <input
      id="teams-unlock-pass"
      class="input"
      class:input-invalid={!!error}
      type="password"
      bind:value={passphrase}
      oninput={() => (error = null)}
      autocomplete="current-password"
      aria-invalid={!!error}
      aria-describedby={error ? "teams-unlock-error" : undefined}
    />
    {#if error}
      <span id="teams-unlock-error" class="field-error" role="alert">{error}</span>
    {/if}
  </div>
  <div class="flex flex-wrap items-center gap-3">
    <button type="submit" class="btn btn-primary" disabled={busy || !passphrase}>
      {#if busy}<span class="spinner size-3"></span>{/if}
      Unlock
    </button>
    {#if !passphrase}<span class="type-meta">Enter your passphrase</span>{/if}
  </div>
</form>
