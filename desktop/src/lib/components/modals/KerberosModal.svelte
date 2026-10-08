<script lang="ts">
  import { Eye, EyeOff, X, ChevronRight } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { KerberosStatus } from "$lib/stores/kerberos.svelte";
  import { formatKerberosRemaining } from "$lib/stores/kerberos.svelte";

  interface Props {
    status: KerberosStatus;
    remainingSeconds: number | null;
    ticketLifetimeSeconds: number | null;
    loading: boolean;
    error: string | null;
    onClose: () => void;
    onRenew: (password?: string) => void | Promise<void>;
    onAcquire: (
      principal: string,
      password: string,
      forwardable: boolean,
      proxiable: boolean,
      lifetime: string,
      renewLifetime: string,
    ) => void | Promise<void>;
  }

  let {
    status,
    remainingSeconds,
    ticketLifetimeSeconds,
    loading,
    error,
    onClose,
    onRenew,
    onAcquire,
  }: Props = $props();

  let password = $state("");
  let principal = $state("");
  let showPassword = $state(false);
  let needsPassword = $state(false);

  let showAdvanced = $state(false);
  let forwardable = $state(true);
  let proxiable = $state(false);
  let lifetime = $state("");
  let renewLifetime = $state("");

  type Health = "unavailable" | "missing" | "expired" | "warning" | "valid";

  const health = $derived.by((): Health => {
    if (!status.tools_available) return "unavailable";
    if (!status.has_ticket) return "missing";
    if (remainingSeconds !== null && remainingSeconds <= 0) return "expired";
    if (!status.valid) return "expired";
    if (remainingSeconds !== null && remainingSeconds <= 15 * 60) return "warning";
    return "valid";
  });

  const HEALTH_UI: Record<Health, { label: string; dot: string; text: string; bar: string }> = {
    unavailable: { label: "Unavailable", dot: "status-dot-offline", text: "text-muted", bar: "bg-faint" },
    missing: { label: "No ticket", dot: "status-dot-offline", text: "text-secondary", bar: "bg-faint" },
    expired: { label: "Expired", dot: "status-dot-error", text: "text-error", bar: "bg-error" },
    warning: { label: "Expiring soon", dot: "status-dot-warning", text: "text-warning", bar: "bg-warning" },
    valid: { label: "Valid", dot: "status-dot-success", text: "text-success", bar: "bg-success" },
  };
  const ui = $derived(HEALTH_UI[health]);

  const progressPercent = $derived.by(() => {
    if (remainingSeconds === null || !ticketLifetimeSeconds || ticketLifetimeSeconds <= 0) return 0;
    return Math.max(0, Math.min(100, (remainingSeconds / ticketLifetimeSeconds) * 100));
  });

  // Renew uses kinit -R only when a ticket exists; otherwise acquire a new one.
  const renewing = $derived(status.has_ticket);
  const showPasswordField = $derived(!renewing || needsPassword);
  const canSubmit = $derived(
    !loading && (renewing ? !needsPassword || password.trim() !== "" : password.trim() !== ""),
  );

  $effect(() => {
    const candidate = status.principal ?? status.suggested_principal;
    if (candidate && !principal) {
      principal = candidate;
    }
  });

  async function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (!canSubmit) return;
    if (renewing) {
      await onRenew(needsPassword ? password : undefined);
    } else {
      await onAcquire(principal, password, forwardable, proxiable, lifetime, renewLifetime);
    }
  }
</script>

<ModalShell open={true} title="Kerberos ticket" {onClose} width="md">
  <form class="flex min-h-0 flex-1 flex-col" novalidate onsubmit={handleSubmit}>
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Kerberos ticket</h2>
        <p class="modal-subtitle">A valid ticket keeps GSSAPI sessions signed in.</p>
      </div>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-4">
      {#if !status.tools_available}
        <div class="alert alert-warning">
          <div class="flex flex-col gap-0.5">
            <span class="alert-title">Kerberos tools not found</span>
            <span class="text-secondary">
              Install <code class="font-mono">krb5-user</code> (or your platform's Kerberos client) to use GSSAPI
              authentication.
            </span>
          </div>
        </div>
      {:else}
        <!-- Status summary -->
        <div class="flex flex-col gap-3 rounded-lg border border-border bg-panel p-4">
          <div class="flex items-center justify-between gap-3">
            <span class="flex items-center gap-2">
              <span class="status-dot {ui.dot}" aria-hidden="true"></span>
              <span class="text-sm font-medium {ui.text}">{ui.label}</span>
            </span>
            {#if status.has_ticket}
              <span class="font-mono text-sm tabular-nums text-primary">{formatKerberosRemaining(remainingSeconds)}</span>
            {/if}
          </div>

          {#if status.has_ticket && remainingSeconds !== null && ticketLifetimeSeconds}
            <div class="h-1 overflow-hidden rounded-full bg-surface-active" aria-hidden="true">
              <div class="h-full rounded-full {ui.bar}" style:width="{progressPercent}%"></div>
            </div>
          {/if}

          <dl class="m-0 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
            {#if status.principal && status.has_ticket}
              <dt class="text-muted">Principal</dt>
              <dd class="m-0 truncate text-right font-mono text-secondary" title={status.principal}>{status.principal}</dd>
            {/if}
            {#if status.default_realm}
              <dt class="text-muted">Default realm</dt>
              <dd class="m-0 truncate text-right font-mono text-secondary">{status.default_realm}</dd>
            {:else if !status.client_configured}
              <dt class="text-muted">Client</dt>
              <dd class="m-0 text-right text-secondary">Not configured</dd>
            {/if}
            {#if status.renewable && status.renew_until}
              <dt class="text-muted">Renewable until</dt>
              <dd class="m-0 truncate text-right tabular-nums text-secondary">
                {new Date(status.renew_until * 1000).toLocaleString()}
              </dd>
            {/if}
            {#if status.cache_path}
              <dt class="text-muted">Cache</dt>
              <dd class="m-0 truncate text-right font-mono text-secondary" title={status.cache_path}>{status.cache_path}</dd>
            {/if}
            {#if status.config_path}
              <dt class="text-muted">Config</dt>
              <dd class="m-0 truncate text-right font-mono text-secondary" title={status.config_path}>{status.config_path}</dd>
            {/if}
          </dl>
        </div>

        {#if error}
          <div class="alert alert-error" role="alert">{error}</div>
        {/if}

        <!-- Renew / acquire -->
        <div class="flex flex-col gap-3">
          <div class="flex flex-col gap-0.5">
            <h3 class="m-0 text-sm font-medium text-primary">{renewing ? "Renew ticket" : "Get a ticket"}</h3>
            <p class="m-0 text-xs leading-snug text-muted">
              {#if renewing}
                Tries a passwordless renewal (<code class="font-mono">kinit -R</code>) first.
              {:else}
                Sign in with your Kerberos principal and password.
              {/if}
            </p>
          </div>

          {#if !renewing}
            <div class="field">
              <label class="field-label" for="kerberos-principal">Principal</label>
              <input
                id="kerberos-principal"
                type="text"
                class="input input-mono"
                placeholder="user@REALM.EXAMPLE"
                autocomplete="username"
                spellcheck="false"
                bind:value={principal}
              />
            </div>
          {:else}
            <label for="kerberos-needs-password" class="flex cursor-pointer items-center justify-between gap-4">
              <span class="text-sm text-secondary">Renewal needs my password</span>
              <input id="kerberos-needs-password" type="checkbox" class="switch" bind:checked={needsPassword} />
            </label>
          {/if}

          {#if showPasswordField}
            <div class="field">
              <label class="field-label" for="kerberos-password">Password</label>
              <div class="relative">
                <input
                  id="kerberos-password"
                  type={showPassword ? "text" : "password"}
                  class="input pr-9"
                  placeholder="Kerberos password"
                  autocomplete="current-password"
                  bind:value={password}
                  data-autofocus
                />
                <button
                  type="button"
                  class="btn-icon btn-icon-sm absolute right-1 top-1/2 -translate-y-1/2"
                  onclick={() => (showPassword = !showPassword)}
                  aria-label={showPassword ? "Hide password" : "Show password"}
                >
                  {#if showPassword}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
                </button>
              </div>
            </div>
          {/if}

          {#if !renewing}
            <div class="rounded-md border border-border">
              <button
                type="button"
                class="flex h-9 w-full cursor-pointer items-center gap-1.5 rounded-md px-3 text-left text-sm text-secondary transition-colors hover:text-primary"
                aria-expanded={showAdvanced}
                aria-controls="krb-advanced"
                onclick={() => (showAdvanced = !showAdvanced)}
              >
                <ChevronRight
                  size={14}
                  class="text-muted transition-transform duration-150 {showAdvanced ? 'rotate-90' : ''}"
                />
                Ticket options
              </button>
              {#if showAdvanced}
                <div id="krb-advanced" class="flex flex-col gap-3 border-t border-border-subtle p-3">
                  <label for="krb-forwardable" class="flex cursor-pointer items-center justify-between gap-4">
                    <span class="flex flex-col gap-0.5">
                      <span class="text-sm text-secondary">Forwardable <span class="mono text-muted">-f</span></span>
                      <span class="text-xs text-muted">Allow the ticket to be forwarded to remote hosts.</span>
                    </span>
                    <input id="krb-forwardable" type="checkbox" class="switch" bind:checked={forwardable} />
                  </label>
                  <label for="krb-proxiable" class="flex cursor-pointer items-center justify-between gap-4">
                    <span class="flex flex-col gap-0.5">
                      <span class="text-sm text-secondary">Proxiable <span class="mono text-muted">-p</span></span>
                      <span class="text-xs text-muted">Allow the ticket to be proxied to other hosts.</span>
                    </span>
                    <input id="krb-proxiable" type="checkbox" class="switch" bind:checked={proxiable} />
                  </label>
                  <div class="grid grid-cols-2 gap-3">
                    <div class="field">
                      <label for="krb-opt-lifetime" class="field-label">Lifetime <span class="mono text-muted">-l</span></label>
                      <input
                        id="krb-opt-lifetime"
                        type="text"
                        placeholder="10h"
                        spellcheck="false"
                        bind:value={lifetime}
                        class="input input-mono"
                      />
                    </div>
                    <div class="field">
                      <label for="krb-opt-renew-lifetime" class="field-label">Renewable for <span class="mono text-muted">-r</span></label>
                      <input
                        id="krb-opt-renew-lifetime"
                        type="text"
                        placeholder="7d"
                        spellcheck="false"
                        bind:value={renewLifetime}
                        class="input input-mono"
                      />
                    </div>
                    <span class="field-hint col-span-2">Leave empty to use the realm defaults.</span>
                  </div>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={onClose}>Close</button>
      {#if status.tools_available}
        <button type="submit" class="btn btn-primary" disabled={!canSubmit}>
          {#if loading}<span class="spinner"></span>{/if}
          {renewing ? "Renew ticket" : "Get ticket"}
        </button>
      {/if}
    </div>
  </form>
</ModalShell>
