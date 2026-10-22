<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { TeamRemovePreview, TeamSummary, TeamSyncReport } from "$lib/types";
  import { raiseAttention, resolveAttention, TEAM_ATTENTION } from "$lib/stores/attention.svelte";
  import { plural } from "./teams";

  let {
    team,
    email,
    onClose,
    onRemoved,
  }: {
    team: TeamSummary;
    email: string;
    onClose: () => void;
    onRemoved: (report: TeamSyncReport) => void;
  } = $props();

  let preview = $state.raw<TeamRemovePreview | null>(null);
  let previewError = $state<string | null>(null);
  let typed = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  /** teams_remove's error when the key rotation itself stopped (unapproved device). */
  const rotationFailed = $derived(!!error && /rotat/i.test(error));
  const blocked = $derived((preview?.blocking.length ?? 0) > 0);
  const blocker = $derived.by(() => {
    if (!preview) return previewError ? "The preview didn't load" : "Loading what removal would change…";
    if (blocked) return "Unapproved devices block the key rotation";
    if (typed.trim().toLowerCase() !== email.toLowerCase()) return `Type ${email} to confirm`;
    return null;
  });

  async function loadPreview() {
    previewError = null;
    try {
      preview = await invoke<TeamRemovePreview>("teams_remove_preview", { team: team.team_id, email });
    } catch (err) {
      previewError = String(err);
    }
  }

  async function remove() {
    if (blocker || busy) return;
    busy = true;
    error = null;
    try {
      const report = await invoke<TeamSyncReport>("teams_remove", { team: team.team_id, email });
      resolveAttention(TEAM_ATTENTION.rotationBlocked(team.team_id));
      onRemoved(report);
    } catch (err) {
      error = String(err);
      if (/rotat/i.test(error)) {
        raiseAttention({
          id: TEAM_ATTENTION.rotationBlocked(team.team_id),
          severity: "error",
          title: `${team.name}: key rotation stopped`,
          body: `Removing ${email} didn't finish: the team key wasn't rotated. Check the waiting devices' fingerprints, then remove again.`,
          action: { label: "Open team", tab: "teams", teamId: team.team_id },
        });
      }
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void loadPreview();
  });
</script>

<ModalShell open={true} title="Remove {email} from {team.name}?" {onClose} width="form">
  <form
    onsubmit={(e) => {
      e.preventDefault();
      void remove();
    }}
  >
    <div class="modal-header">
      <h2 class="modal-title break-words">Remove {email} from {team.name}?</h2>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close"><X size={16} /></button>
    </div>
    <div class="modal-body flex flex-col gap-3 text-sm text-secondary">
      {#if previewError}
        <div class="alert alert-error" role="alert">
          <div class="flex flex-col gap-2">
            <span>Couldn't work out what removal would change: {previewError}</span>
            <div><button type="button" class="btn btn-secondary btn-sm" onclick={loadPreview}>Try again</button></div>
          </div>
        </div>
      {:else if !preview}
        <div class="flex flex-col gap-2" aria-busy="true" aria-label="Loading the removal preview">
          <div class="skeleton h-4 w-2/3 rounded-sm"></div>
          <div class="skeleton h-4 w-full rounded-sm"></div>
          <div class="skeleton h-4 w-1/2 rounded-sm"></div>
        </div>
      {:else}
        <p class="m-0">This rotates the team key to <span class="font-medium text-primary">v{preview.next_key_version}</span>. After that:</p>
        <ul class="m-0 flex list-disc flex-col gap-1.5 pl-5">
          <li>
            {#if preview.removed_devices.length}
              Their {preview.removed_devices.length === 1 ? "device" : "devices"}
              ({preview.removed_devices.map((d) => d.name || "device").join(", ")}) can't sync or read anything new.
            {:else}
              They can't sync or read anything new.
            {/if}
          </li>
          <li>
            They keep copies of {plural(preview.shared_hosts, "host")} already synced: rotate credentials that matter.
          </li>
          <li>
            The new key goes to the devices this machine approved:
            {#if preview.recipients.length}
              <ul class="mt-1 flex list-none flex-col gap-0.5 p-0">
                {#each preview.recipients as d (d.fingerprint)}
                  <li class="text-primary">{d.email} · {d.name || "device"}</li>
                {/each}
              </ul>
            {:else}
              <span class="text-primary">none besides yours.</span>
            {/if}
          </li>
        </ul>
        <p class="m-0 text-xs text-muted">
          Lost a laptop? Devices can't be revoked one by one yet: remove the member, then invite them again.
        </p>

        {#if blocked}
          <div class="alert alert-error" role="alert">
            <div class="flex flex-col gap-1.5">
              <span class="alert-title">Rotation can't run: the server lists devices this machine never approved.</span>
              <ul class="m-0 flex list-none flex-col gap-0.5 p-0">
                {#each preview.blocking as d (d.fingerprint)}
                  <li>{d.email} · {d.name || "device"}</li>
                {/each}
              </ul>
              <span>
                Grant each one after checking its fingerprint with its owner (Members &amp; devices tab), or remove
                that member first. {email} isn't removed.
              </span>
            </div>
          </div>
        {:else}
          <div class="field">
            <label class="field-label" for="remove-confirm">Type {email} to confirm</label>
            <input id="remove-confirm" class="input" bind:value={typed} autocomplete="off" spellcheck="false" />
          </div>
        {/if}
      {/if}

      {#if error}
        <div class="alert alert-error" role="alert">
          <div class="flex flex-col gap-1">
            <span class="alert-title">{rotationFailed ? `Rotation stopped. ${email} isn't fully removed yet.` : "Couldn't remove the member."}</span>
            <span>{error}</span>
          </div>
        </div>
      {/if}

      <p class="m-0 text-xs text-muted">CLI: <code class="select-text">bssh teams remove {team.name} {email}</code></p>
    </div>
    <div class="modal-footer">
      {#if blocker}<span id="remove-reason" class="mr-auto text-xs text-muted">{blocker}</span>{/if}
      <button type="button" class="btn btn-secondary" onclick={onClose} data-autofocus>Cancel</button>
      <button
        type="submit"
        class="btn btn-danger"
        disabled={busy || !!blocker}
        aria-describedby={blocker ? "remove-reason" : undefined}
      >
        {#if busy}<span class="spinner size-3"></span>{/if}
        Remove and rotate key
      </button>
    </div>
  </form>
</ModalShell>
