<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { CircleCheck, CircleX, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { TeamPendingDevice, TeamSummary } from "$lib/types";
  import Fingerprint from "./Fingerprint.svelte";
  import { fingerprintLooksComplete, fingerprintsMatch, normalizeFingerprint } from "./teams";

  /**
   * The only path to `teams_grant`. The fingerprint the server reports for the
   * waiting device (`device.device.fingerprint`) is compared against what the
   * admin pastes, and is never rendered before it matches: showing it would
   * let a tired admin copy the server's answer instead of the member's.
   */
  let {
    team,
    device,
    onClose,
    onGranted,
  }: {
    team: TeamSummary;
    device: TeamPendingDevice;
    onClose: () => void;
    onGranted: (granted: TeamPendingDevice) => void;
  } = $props();

  let pasted = $state("");
  let attested = $state(false);
  let touched = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const deviceName = $derived(device.device.name || "this device");
  const matches = $derived(!!pasted.trim() && fingerprintsMatch(pasted, device.device.fingerprint));
  const mismatch = $derived(!!pasted.trim() && !matches && (touched || fingerprintLooksComplete(pasted)));

  const blocker = $derived.by(() => {
    if (!pasted.trim()) return `Paste the fingerprint ${device.email} sent you`;
    if (!matches) return mismatch ? "The fingerprint doesn't match" : "Paste the whole fingerprint";
    if (!attested) return "Confirm where the fingerprint came from";
    return null;
  });

  async function grant() {
    if (blocker || busy) return;
    busy = true;
    error = null;
    try {
      const result = await invoke<TeamPendingDevice>("teams_grant", {
        team: team.team_id,
        fingerprint: device.device.fingerprint,
      });
      onGranted(result);
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<ModalShell open={true} title="Grant the {team.name} key to a new device" {onClose} width="form">
  <form
    onsubmit={(e) => {
      e.preventDefault();
      void grant();
    }}
  >
    <div class="modal-header">
      <h2 class="modal-title">Grant the {team.name} key to a new device</h2>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close"><X size={16} /></button>
    </div>
    <div class="modal-body flex flex-col gap-4 text-sm text-secondary">
      <p class="m-0">
        <span class="font-medium text-primary">{device.email}</span> is waiting for the key on
        <span class="font-medium text-primary">"{deviceName}"</span>.
      </p>

      <ol class="m-0 flex list-none flex-col gap-4 p-0">
        <li class="flex gap-2.5">
          <span class="step" aria-hidden="true">1</span>
          <span>
            Ask {device.email} for this device's fingerprint over a channel you trust: a call, or a chat you already
            use. They find it under Teams → This device, or with <code>bssh teams whoami</code>.
          </span>
        </li>
        <li class="flex gap-2.5">
          <span class="step" aria-hidden="true">2</span>
          <div class="field flex-1">
            <label class="field-label" for="grant-fp">Fingerprint {device.email} sent you</label>
            <input
              id="grant-fp"
              class="input input-mono text-sm"
              class:input-invalid={mismatch}
              bind:value={pasted}
              onblur={() => (touched = true)}
              autocomplete="off"
              spellcheck="false"
              placeholder="SHA256:…"
              aria-invalid={mismatch}
              aria-describedby="grant-fp-result"
              data-autofocus
            />
            <div id="grant-fp-result" aria-live="polite">
              {#if matches}
                <div class="alert alert-success mt-1">
                  <CircleCheck size={14} class="mt-0.5 shrink-0" />
                  <div class="flex min-w-0 flex-col gap-1.5">
                    <span>Matches the device waiting on the server.</span>
                    <span class="text-secondary">Read it back to {device.email} to be sure:</span>
                    <Fingerprint value={normalizeFingerprint(pasted)} copyable={false} />
                  </div>
                </div>
              {:else if mismatch}
                <div class="alert alert-error mt-1">
                  <CircleX size={14} class="mt-0.5 shrink-0" />
                  <span>
                    Doesn't match. Don't grant. If {device.email} sent it, ask them to read it again; otherwise someone
                    may be impersonating this device.
                  </span>
                </div>
              {/if}
            </div>
          </div>
        </li>
        <li class="flex gap-2.5">
          <span class="step" aria-hidden="true">3</span>
          <label class="flex cursor-pointer items-start gap-2">
            <input type="checkbox" class="checkbox mt-0.5" bind:checked={attested} />
            <span>I got this fingerprint from {device.email} directly, not from this screen.</span>
          </label>
        </li>
      </ol>

      <p class="m-0 text-xs text-muted">
        Granting lets this device read every {team.name} host and write to it.
      </p>
      {#if error}<div class="alert alert-error" role="alert">Couldn't grant the key: {error}</div>{/if}
      <p class="m-0 text-xs text-muted">
        CLI: <code class="select-text">bssh teams grant {team.name} --fingerprint {matches ? normalizeFingerprint(pasted) : "<FINGERPRINT>"}</code>
      </p>
    </div>
    <div class="modal-footer">
      {#if blocker}<span id="grant-reason" class="mr-auto text-xs text-muted">{blocker}</span>{/if}
      <button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
      <button
        type="submit"
        class="btn btn-primary"
        disabled={busy || !!blocker}
        aria-describedby={blocker ? "grant-reason" : undefined}
      >
        {#if busy}<span class="spinner size-3"></span>{/if}
        Grant key to {deviceName}
      </button>
    </div>
  </form>
</ModalShell>

<style>
  .step {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    width: 1.25rem;
    height: 1.25rem;
    border-radius: 9999px;
    border: 1px solid var(--color-border-strong);
    font-size: var(--text-2xs);
    color: var(--color-secondary);
  }
</style>
