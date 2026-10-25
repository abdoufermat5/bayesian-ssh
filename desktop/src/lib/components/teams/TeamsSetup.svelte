<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Users } from "lucide-svelte";
  import type { EmailCodeResult, TeamsStatus } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import {
    emailCodeNotice,
    parseInvite,
    teamsInitError,
    type ParsedInvite,
  } from "./teams";

  let {
    onDone,
  }: {
    /** `invite` is set when the user pasted an invite here, to finish the join next. */
    onDone: (status: TeamsStatus, invite: ParsedInvite | null) => void;
  } = $props();

  let server = $state("");
  let email = $state("");
  let deviceName = $state("");
  let pass1 = $state("");
  let pass2 = $state("");
  let inviteText = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  /** The server asked to verify the email: show the code field and ask again. */
  let codeNeeded = $state(false);
  let emailCode = $state("");
  let codeNotice = $state<string | null>(null);
  let resending = $state(false);

  const invite = $derived(inviteText.trim() ? parseInvite(inviteText) : null);

  function prefillFromInvite() {
    if (!invite) return;
    if (invite.server) server = invite.server;
    if (invite.email) email = invite.email;
  }

  const serverOk = $derived(/^https?:\/\/\S+$/i.test(server.trim()));
  const blocker = $derived.by(() => {
    if (!server.trim()) return "Enter the sync server";
    if (!serverOk) return "The server starts with https://";
    if (!/^\S+@\S+$/.test(email.trim())) return "Enter your email";
    if (pass1.length < 8) return "The passphrase needs at least 8 characters";
    if (pass1 !== pass2) return "The passphrases don't match";
    if (codeNeeded && !emailCode.trim()) return "Enter the verification code";
    return null;
  });

  async function setUp() {
    if (blocker) return;
    busy = true;
    error = null;
    try {
      const next = await invoke<TeamsStatus>("teams_init", {
        server: server.trim(),
        email: email.trim(),
        deviceName: deviceName.trim() || null,
        passphrase: pass1,
        // Servers that only accept invited accounts check the pasted invite.
        inviteToken: invite?.token ?? null,
        // Sent only once the server asked for it.
        emailCode: codeNeeded ? emailCode.trim() : null,
      });
      pass1 = pass2 = "";
      notify("Teams is set up on this device", "success");
      onDone(next, invite?.token || invite?.fingerprint ? invite : null);
    } catch (err) {
      const init = teamsInitError(err);
      if (init?.kind === "email_code_sent" || init?.kind === "email_code_rejected") {
        codeNeeded = true;
        emailCode = "";
        codeNotice = emailCodeNotice(init);
      } else if (init?.kind === "failed") {
        error = init.message;
      } else {
        error = String(err);
      }
    } finally {
      busy = false;
    }
  }

  async function sendNewCode() {
    if (resending || busy) return;
    resending = true;
    error = null;
    try {
      const sent = await invoke<EmailCodeResult>("teams_request_email_code", {
        server: server.trim(),
        email: email.trim(),
      });
      codeNeeded = true;
      emailCode = "";
      codeNotice = emailCodeNotice({ kind: "email_code_sent", email: email.trim(), expires_in_secs: sent.expires_in_secs });
    } catch (err) {
      error = String(err);
    } finally {
      resending = false;
    }
  }
</script>

<form
  class="panel mx-auto flex max-w-xl flex-col gap-4 p-5"
  onsubmit={(e) => {
    e.preventDefault();
    void setUp();
  }}
>
  <div class="flex items-center gap-2">
    <Users size={16} />
    <h2 class="m-0 text-base font-semibold text-primary">Share connections with your team</h2>
  </div>
  <p class="m-0 type-body text-secondary">
    Team connections are encrypted on your devices; the sync server only ever stores ciphertext. Set this
    device up once, then create a team or join one you were invited to.
  </p>

  <div class="field">
    <label class="field-label" for="teams-invite-paste">Got an invite? Paste it here (optional)</label>
    <textarea
      id="teams-invite-paste"
      class="input input-mono h-20 py-2 text-xs"
      bind:value={inviteText}
      oninput={prefillFromInvite}
      placeholder="Paste the whole invite message to fill in the server and your email"
    ></textarea>
    {#if invite && (invite.server || invite.email)}
      <span class="field-hint">Filled in from the invite. You'll finish joining after setup.</span>
    {/if}
  </div>

  <div class="field">
    <label class="field-label" for="teams-server">Sync server</label>
    <input
      id="teams-server"
      class="input input-mono"
      class:input-invalid={!!server.trim() && !serverOk}
      bind:value={server}
      placeholder="https://teams.example.com"
    />
  </div>
  <div class="field">
    <label class="field-label" for="teams-email">Your email</label>
    <input id="teams-email" class="input" type="email" bind:value={email} placeholder="you@example.com" />
  </div>
  <div class="field">
    <label class="field-label" for="teams-device">Device name</label>
    <input id="teams-device" class="input" bind:value={deviceName} placeholder="laptop" aria-describedby="teams-device-hint" />
    <span id="teams-device-hint" class="field-hint">Shown to team admins. Defaults to this machine's name.</span>
  </div>
  <div class="field">
    <label class="field-label" for="teams-pass1">Passphrase</label>
    <input
      id="teams-pass1"
      class="input"
      type="password"
      bind:value={pass1}
      autocomplete="new-password"
      aria-describedby="teams-pass-hint"
    />
    <span id="teams-pass-hint" class="field-hint">
      Encrypts this device's Teams keys. It never leaves this machine and nobody can reset it. If you forget it,
      set this device up again and ask an admin to grant the key to the new device.
    </span>
  </div>
  <div class="field">
    <label class="field-label" for="teams-pass2">Repeat passphrase</label>
    <input id="teams-pass2" class="input" type="password" bind:value={pass2} autocomplete="new-password" />
  </div>

  {#if codeNeeded}
    <div class="field">
      <label class="field-label" for="teams-setup-code">Verification code</label>
      <input
        id="teams-setup-code"
        class="input input-mono"
        bind:value={emailCode}
        autocomplete="one-time-code"
        inputmode="numeric"
        aria-describedby="teams-setup-code-hint"
        data-autofocus
      />
      <span id="teams-setup-code-hint" class="field-hint">
        {codeNotice ?? `Enter the code the server emailed to ${email.trim()}.`}
      </span>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <button type="button" class="btn btn-secondary btn-sm" onclick={sendNewCode} disabled={resending || busy}>
        {#if resending}<span class="spinner size-3"></span>{/if}
        Send a new code
      </button>
      <span class="type-meta">or wait for the last one to arrive.</span>
    </div>
  {/if}

  {#if error}
    <div class="alert alert-error" role="alert">Setup failed: {error}</div>
  {/if}

  <div class="flex flex-wrap items-center gap-3">
    <button type="submit" class="btn btn-primary" disabled={busy || !!blocker} aria-describedby="teams-setup-reason">
      {#if busy}<span class="spinner size-3"></span>{/if}
      Set up this device
    </button>
    {#if blocker}
      <span id="teams-setup-reason" class="type-meta">{blocker}</span>
    {/if}
  </div>
</form>
