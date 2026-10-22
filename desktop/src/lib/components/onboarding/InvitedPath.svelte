<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ArrowLeft } from "lucide-svelte";
  import type { TeamJoin, TeamsStatus } from "$lib/types";
  import Fingerprint from "$lib/components/teams/Fingerprint.svelte";
  import { normalizeFingerprint, parseInvite, teamIdFromJoinError } from "$lib/components/teams/teams";

  let {
    busy,
    onBack,
    onFinish,
  }: {
    /** True while the parent is completing onboarding. */
    busy: boolean;
    onBack: () => void;
    onFinish: (join: TeamJoin) => void;
  } = $props();

  const STEPS = ["Invite", "Passphrase", "Fingerprint"] as const;

  let step = $state(0);
  let message = $state("");
  let token = $state("");
  let fingerprint = $state("");
  let server = $state("");
  let email = $state("");
  let pass1 = $state("");
  let pass2 = $state("");
  let working = $state(false);
  let error = $state<string | null>(null);
  /** The device is set up; only the join can still fail and be retried. */
  let initialised = $state(false);
  /** Set after a fingerprint mismatch: the token is spent, finish by team id. */
  let retryTeamId = $state<string | null>(null);
  let joined = $state.raw<TeamJoin | null>(null);

  function parseMessage() {
    const parsed = parseInvite(message);
    token = parsed.token ?? "";
    fingerprint = parsed.fingerprint ?? "";
    server = parsed.server ?? "";
    email = parsed.email ?? "";
  }

  const serverOk = $derived(/^https?:\/\/\S+$/i.test(server.trim()));

  const inviteBlocker = $derived.by(() => {
    if (!message.trim()) return "Paste the message your admin sent you";
    if (!token.trim()) return "The message has no invite token";
    if (!serverOk) return "The message has no sync server";
    if (!/^\S+@\S+$/.test(email.trim())) return "The message has no email address";
    if (!fingerprint.trim()) return "The message has no admin fingerprint";
    return null;
  });

  const passBlocker = $derived.by(() => {
    if (initialised) return fingerprint.trim() ? null : "Enter the admin's fingerprint from the invite";
    if (pass1.length < 8) return "The passphrase needs at least 8 characters";
    if (pass1 !== pass2) return "The passphrases don't match";
    return null;
  });

  async function joinTeam() {
    if (passBlocker || working) return;
    working = true;
    error = null;
    try {
      if (!initialised) {
        await invoke<TeamsStatus>("teams_init", {
          server: server.trim(),
          email: email.trim(),
          deviceName: null,
          passphrase: pass1,
          inviteToken: token.trim(),
        });
        initialised = true;
        pass1 = pass2 = "";
      }
      joined = await invoke<TeamJoin>("teams_join", {
        token: retryTeamId ? null : token.trim(),
        teamId: retryTeamId,
        fingerprint: normalizeFingerprint(fingerprint),
      });
      step = 2;
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
      working = false;
    }
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (step === 0) {
      if (!inviteBlocker) step = 1;
    } else if (step === 1) {
      void joinTeam();
    } else if (joined && !busy) {
      onFinish(joined);
    }
  }

  function back() {
    if (step === 0) onBack();
    else if (step === 1 && !working) step = 0;
  }

  function focusStep(node: HTMLElement) {
    requestAnimationFrame(() => {
      const target =
        node.querySelector<HTMLElement>("[data-autofocus]") ?? document.getElementById("ob-invite-primary");
      target?.focus({ preventScroll: true });
    });
  }
</script>

<ol class="m-0 grid list-none grid-cols-3 gap-2 p-0" aria-label="Setup progress">
  {#each STEPS as label, i (label)}
    <li class="flex min-w-0 flex-col gap-1.5" aria-current={i === step ? "step" : undefined}>
      <span class="h-0.5 rounded-full {i <= step ? 'bg-accent' : 'bg-surface-active'}"></span>
      <span class="truncate text-xs {i === step ? 'text-primary' : i < step ? 'text-secondary' : 'text-muted'}">
        {label}
      </span>
    </li>
  {/each}
</ol>

<form class="flex flex-col rounded-xl border border-border bg-panel" novalidate onsubmit={submit}>
  {#key step}
    <div class="flex flex-col gap-5 p-6" use:focusStep>
      {#if step === 0}
        <div class="flex flex-col gap-1">
          <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Paste your invite</h1>
          <p class="m-0 text-sm text-muted">
            Paste the whole message your team admin sent you. Only use a message you got from them directly.
          </p>
        </div>
        <div class="field">
          <label class="field-label" for="ob-invite-message">Invite message</label>
          <textarea
            id="ob-invite-message"
            class="input input-mono h-32 py-2 text-xs"
            bind:value={message}
            oninput={parseMessage}
            placeholder="Join the “…” team on Bayesian SSH."
            data-autofocus
          ></textarea>
          {#if message.trim() && !inviteBlocker}
            <span class="field-hint">Invite for {email} on {server}.</span>
          {:else if message.trim()}
            <span class="field-error">{inviteBlocker}</span>
          {/if}
        </div>
      {:else if step === 1}
        <div class="flex flex-col gap-1">
          <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Choose a passphrase</h1>
          <p class="m-0 text-sm text-muted">It protects this device's team keys. Then you join {email}'s team.</p>
        </div>
        {#if !initialised}
          <div class="field">
            <label class="field-label" for="ob-pass1">Passphrase</label>
            <input
              id="ob-pass1"
              class="input"
              type="password"
              bind:value={pass1}
              autocomplete="new-password"
              aria-describedby="ob-pass-hint"
              data-autofocus
            />
            <span id="ob-pass-hint" class="field-hint">
              Encrypts this device's Teams keys. It never leaves this machine and nobody can reset it. If you forget
              it, set this device up again and ask an admin to grant the key to the new device.
            </span>
          </div>
          <div class="field">
            <label class="field-label" for="ob-pass2">Repeat passphrase</label>
            <input id="ob-pass2" class="input" type="password" bind:value={pass2} autocomplete="new-password" />
          </div>
        {:else}
          <div class="field">
            <label class="field-label" for="ob-admin-fp">Admin's fingerprint (from the invite)</label>
            <input
              id="ob-admin-fp"
              class="input input-mono text-sm"
              class:input-invalid={error === "mismatch"}
              bind:value={fingerprint}
              placeholder="SHA256:…"
              aria-invalid={error === "mismatch"}
              data-autofocus
            />
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

        <p class="m-0 text-xs text-muted">
          CLI:
          <code class="select-text">
            {retryTeamId
              ? `bssh teams join --team ${retryTeamId} --fingerprint ${fingerprint.trim() ? normalizeFingerprint(fingerprint) : "<FINGERPRINT>"}`
              : `bssh teams join ${token.trim()} --fingerprint ${normalizeFingerprint(fingerprint)}`}
          </code>
        </p>
      {:else if joined}
        <div class="flex flex-col gap-1">
          <h1 class="m-0 text-lg font-semibold tracking-[-0.01em] text-primary">Joined {joined.name}</h1>
          <p class="m-0 text-sm text-muted">One step left, and it's on your admin.</p>
        </div>
        <div class="alert alert-info" role="status">
          <div class="flex flex-col gap-2">
            <span class="alert-title">Send this to your admin; they grant the key after checking it.</span>
            <span>Use a channel where they know it's you. Until they do, the team's hosts stay locked.</span>
            <Fingerprint value={joined.my_fingerprint} label="your fingerprint" />
          </div>
        </div>
        <p class="m-0 text-xs text-muted">
          Teams checks for the key while it's open and syncs as soon as it arrives.
        </p>
      {/if}
    </div>
  {/key}

  <div class="flex items-center gap-2 border-t border-border px-6 py-3">
    {#if step < 2}
      <button type="button" class="btn btn-ghost" onclick={back} disabled={working}>
        <ArrowLeft size={14} />
        Back
      </button>
    {/if}
    {#if step === 1 && passBlocker}<span class="ml-2 text-xs text-muted">{passBlocker}</span>{/if}
    <span class="ml-auto text-xs tabular-nums text-muted">Step {step + 1} of {STEPS.length}</span>
    <button
      id="ob-invite-primary"
      type="submit"
      class="btn btn-primary"
      disabled={working || busy || (step === 0 && !!inviteBlocker) || (step === 1 && !!passBlocker)}
    >
      {#if working || busy}<span class="spinner"></span>{/if}
      {step === 0 ? "Continue" : step === 1 ? (retryTeamId ? "Try again" : "Join team") : "Open Teams"}
    </button>
  </div>
</form>
