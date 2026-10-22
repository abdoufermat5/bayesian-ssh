<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { TeamJoin, TeamsIdentity } from "$lib/types";
  import Fingerprint from "./Fingerprint.svelte";
  import { normalizeFingerprint, parseInvite, teamIdFromJoinError, type ParsedInvite } from "./teams";

  let {
    identity,
    initial = null,
    onClose,
    onJoined,
  }: {
    identity: TeamsIdentity;
    initial?: ParsedInvite | null;
    onClose: () => void;
    onJoined: (join: TeamJoin) => void;
  } = $props();

  // Seeded once from an invite pasted during setup; editable afterwards.
  const seed = untrack(() => initial);
  let message = $state("");
  let token = $state(seed?.token ?? "");
  let fingerprint = $state(seed?.fingerprint ?? "");
  let inviteServer = $state(seed?.server ?? "");
  let inviteEmail = $state(seed?.email ?? "");
  /** Set after a fingerprint mismatch: the token is spent, finish by team id. */
  let retryTeamId = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let joined = $state.raw<TeamJoin | null>(null);

  function parseMessage() {
    const parsed = parseInvite(message);
    if (parsed.token) token = parsed.token;
    if (parsed.fingerprint) fingerprint = parsed.fingerprint;
    inviteServer = parsed.server ?? "";
    inviteEmail = parsed.email ?? "";
  }

  const serverKey = (url: string) => url.trim().replace(/\/+$/, "").toLowerCase();
  const serverMismatch = $derived(!!inviteServer && serverKey(inviteServer) !== serverKey(identity.server_url));
  const emailMismatch = $derived(!!inviteEmail && inviteEmail.toLowerCase() !== identity.email.toLowerCase());

  const blocker = $derived.by(() => {
    if (!retryTeamId && !token.trim()) return "Paste the invite or its token";
    if (!fingerprint.trim()) return "Enter the admin's fingerprint from the invite";
    if (serverMismatch) return "This invite is for another server";
    return null;
  });

  async function join() {
    if (blocker || busy) return;
    busy = true;
    error = null;
    try {
      const result = await invoke<TeamJoin>("teams_join", {
        token: retryTeamId ? null : token.trim(),
        teamId: retryTeamId,
        fingerprint: normalizeFingerprint(fingerprint),
      });
      joined = result;
      onJoined(result);
    } catch (err) {
      const text = String(err);
      const teamId = teamIdFromJoinError(text);
      if (teamId) {
        retryTeamId = teamId;
        error = "mismatch";
      } else {
        error = text;
      }
    } finally {
      busy = false;
    }
  }
</script>

<ModalShell open={true} title={joined ? `Joined ${joined.name}` : "Join a team"} {onClose} width="form">
  <div class="modal-header">
    <h2 class="modal-title">{joined ? `Joined ${joined.name}` : "Join a team"}</h2>
    <button type="button" class="modal-close" onclick={onClose} aria-label="Close"><X size={16} /></button>
  </div>

  {#if joined}
    <div class="modal-body flex flex-col gap-3">
      <div class="alert alert-info" role="status">
        <div class="flex flex-col gap-2">
          <span class="alert-title">Waiting for an admin to grant this device the key.</span>
          <span>Send them your fingerprint over a channel where they know it's you:</span>
          <Fingerprint value={joined.my_fingerprint} label="your fingerprint" />
        </div>
      </div>
      <p class="m-0 type-meta">
        This page checks every 30 seconds while it's open and syncs as soon as the key arrives.
      </p>
    </div>
    <div class="modal-footer">
      <button type="button" class="btn btn-primary" onclick={onClose} data-autofocus>Done</button>
    </div>
  {:else}
    <form
      onsubmit={(e) => {
        e.preventDefault();
        void join();
      }}
    >
      <div class="modal-body flex flex-col gap-3">
        {#if !retryTeamId}
          <div class="field">
            <label class="field-label" for="teams-join-message">Invite message</label>
            <textarea
              id="teams-join-message"
              class="input input-mono h-28 py-2 text-xs"
              bind:value={message}
              oninput={parseMessage}
              placeholder="Paste the whole message your admin sent you"
              data-autofocus
            ></textarea>
            <span class="field-hint">
              Paste only a message you got from your admin directly. The fingerprint in it is what proves the team is
              theirs and not the server's.
            </span>
          </div>
          <div class="field">
            <label class="field-label" for="teams-join-token">Invite token</label>
            <input id="teams-join-token" class="input input-mono text-xs" bind:value={token} />
          </div>
        {/if}
        <div class="field">
          <label class="field-label" for="teams-join-fp">Admin's fingerprint (from the invite)</label>
          <input
            id="teams-join-fp"
            class="input input-mono type-body"
            class:input-invalid={error === "mismatch"}
            bind:value={fingerprint}
            placeholder="SHA256:…"
            aria-invalid={error === "mismatch"}
          />
        </div>

        {#if serverMismatch}
          <div class="alert alert-warning" role="alert">
            This invite is for {inviteServer}, but this device is set up with {identity.server_url}. Ask for an invite
            on {identity.server_url}.
          </div>
        {:else if emailMismatch}
          <div class="alert alert-info">
            The invite was written for {inviteEmail}; you're signed in as {identity.email}. It only works if the
            admin invited this address.
          </div>
        {/if}

        {#if error === "mismatch"}
          <div class="alert alert-error" role="alert">
            <div class="flex flex-col gap-1">
              <span class="alert-title">That fingerprint doesn't belong to an admin of this team.</span>
              <span>
                Nothing was trusted. Check the fingerprint with your admin (a mismatch can also mean the server isn't
                the one you think), fix it above, then try again. The invite token is used up; trying again finishes
                the join for team <code class="select-all">{retryTeamId}</code>.
              </span>
            </div>
          </div>
        {:else if error}
          <div class="alert alert-error" role="alert">Couldn't join: {error}</div>
        {/if}

        <p class="m-0 type-meta">
          CLI:
          <code class="select-text">
            {retryTeamId
              ? `bssh teams join --team ${retryTeamId} --fingerprint ${fingerprint.trim() ? normalizeFingerprint(fingerprint) : "<FINGERPRINT>"}`
              : `bssh teams join ${token.trim() || "<TOKEN>"} --fingerprint ${fingerprint.trim() ? normalizeFingerprint(fingerprint) : "<FINGERPRINT>"}`}
          </code>
        </p>
      </div>
      <div class="modal-footer">
        {#if blocker}<span class="mr-auto text-xs text-muted">{blocker}</span>{/if}
        <button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
        <button type="submit" class="btn btn-primary" disabled={busy || !!blocker}>
          {#if busy}<span class="spinner size-3"></span>{/if}
          {retryTeamId ? "Try again" : "Join"}
        </button>
      </div>
    </form>
  {/if}
</ModalShell>
