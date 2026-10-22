<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Check, Copy, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { TeamInvite, TeamSummary } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";
  import { formatUtc, inviteMessage } from "./teams";

  let { team, onClose }: { team: TeamSummary; onClose: () => void } = $props();

  let email = $state("");
  let admin = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let invite = $state.raw<TeamInvite | null>(null);
  let copied = $state(false);

  const message = $derived(invite ? inviteMessage(team.name, invite) : "");

  async function send() {
    const address = email.trim();
    if (!address || busy) return;
    busy = true;
    error = null;
    try {
      invite = await invoke<TeamInvite>("teams_invite", { team: team.team_id, email: address, admin });
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  function copyMessage() {
    copyTextWithFallback(message);
    copied = true;
    notify("Invite copied", "success");
    setTimeout(() => (copied = false), 2000);
  }
</script>

<ModalShell open={true} title="Invite someone to {team.name}" {onClose} width="form">
  <div class="modal-header">
    <h2 class="modal-title">Invite someone to {team.name}</h2>
    <button type="button" class="modal-close" onclick={onClose} aria-label="Close"><X size={16} /></button>
  </div>
  {#if invite}
    <div class="modal-body flex flex-col gap-3 type-body text-secondary">
      <p class="m-0">
        Send {invite.email} this message. The token is shown only once and works until {formatUtc(invite.expires_at)}.
      </p>
      <pre class="code-block m-0 max-h-72 whitespace-pre-wrap break-all select-text">{message}</pre>
      <p class="m-0 type-meta">
        Send it over a channel where you know it's really {invite.email}. The fingerprint in it is what stops the
        server from impersonating you.
      </p>
    </div>
    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={onClose}>Done</button>
      <button type="button" class="btn btn-primary" onclick={copyMessage} data-autofocus>
        {#if copied}<Check size={14} />{:else}<Copy size={14} />{/if}
        Copy message
      </button>
    </div>
  {:else}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void send();
      }}
    >
      <div class="modal-body flex flex-col gap-3">
        <div class="field">
          <label class="field-label" for="invite-email">Their email</label>
          <input id="invite-email" class="input" type="email" bind:value={email} placeholder="ana@example.com" />
        </div>
        <label class="flex cursor-pointer items-start gap-2 type-body text-secondary">
          <input type="checkbox" class="checkbox mt-0.5" bind:checked={admin} />
          <span>Make them an admin (they can invite, grant keys, remove members and read the audit log)</span>
        </label>
        {#if error}<div class="alert alert-error" role="alert">Couldn't invite: {error}</div>{/if}
        <p class="m-0 type-meta">
          CLI: <code class="select-text">bssh teams invite {team.name} {email.trim() || "EMAIL"}{admin ? " --admin" : ""}</code>
        </p>
      </div>
      <div class="modal-footer">
        {#if !email.trim()}<span class="mr-auto text-xs text-muted">Enter their email</span>{/if}
        <button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
        <button type="submit" class="btn btn-primary" disabled={busy || !email.trim()}>
          {#if busy}<span class="spinner size-3"></span>{/if}
          Create invite
        </button>
      </div>
    </form>
  {/if}
</ModalShell>
