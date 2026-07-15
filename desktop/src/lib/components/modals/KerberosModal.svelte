<script lang="ts">
  import { Eye, EyeOff, KeyRound, RefreshCw, ShieldCheck, X, ChevronDown, ChevronUp, Settings2 } from "lucide-svelte";
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

  // Advanced options states
  let showAdvanced = $state(false);
  let forwardable = $state(true);
  let proxiable = $state(false);
  let lifetime = $state("");
  let renewLifetime = $state("");

  const health = $derived.by(() => {
    if (!status.tools_available) return "unavailable";
    if (!status.has_ticket) return "missing";
    if (remainingSeconds !== null && remainingSeconds <= 0) return "expired";
    if (!status.valid) return "expired";
    if (remainingSeconds !== null && remainingSeconds <= 15 * 60) return "warning";
    return "valid";
  });

  const progressPercent = $derived.by(() => {
    if (remainingSeconds === null || !ticketLifetimeSeconds || ticketLifetimeSeconds <= 0) return 0;
    return Math.max(0, Math.min(100, (remainingSeconds / ticketLifetimeSeconds) * 100));
  });

  $effect(() => {
    const candidate = status.principal ?? status.suggested_principal;
    if (candidate && !principal) {
      principal = candidate;
    }
  });

  async function handleRenew() {
    await onRenew(needsPassword ? password : undefined);
  }

  async function handleAcquire() {
    await onAcquire(principal, password, forwardable, proxiable, lifetime, renewLifetime);
  }
</script>

<div class="fixed inset-0 flex items-center justify-center z-[100]">
  <button
    type="button"
    class="absolute inset-0 bg-black/75 backdrop-blur-sm border-none p-0 cursor-default"
    onclick={onClose}
    aria-label="Close dialog"
  ></button>
  <div
    class="relative bg-surface border border-border rounded-2xl w-[520px] shadow-xl flex flex-col animate-[modal-enter_0.25s_cubic-bezier(0.16,1,0.3,1)_forwards]"
    role="dialog"
    aria-modal="true"
    aria-labelledby="kerberos-modal-title"
    tabindex="-1"
  >
    <div class="flex justify-between items-center px-6 py-5 border-b border-border">
      <div class="flex gap-3 items-start text-accent">
        <ShieldCheck size={20} class="mt-0.5 text-accent" />
        <div>
          <h2 id="kerberos-modal-title" class="text-base font-semibold tracking-tight m-0 text-primary">Kerberos Ticket</h2>
          <p class="text-xs text-muted mt-0.5">Keep your GSSAPI sessions alive with a valid ticket</p>
        </div>
      </div>
      <button
        type="button"
        class="bg-transparent border-none text-muted cursor-pointer flex p-1 rounded-md transition-all duration-100 hover:text-primary hover:bg-white/5"
        onclick={onClose}
        aria-label="Close"
      >
        <X size={16} />
      </button>
    </div>

    <div class="px-6 py-5 flex flex-col gap-4 max-h-[60vh] overflow-y-auto">
      {#if !status.tools_available}
        <div class="p-3.5 rounded-lg border border-danger/35 bg-danger/8 text-red-200 text-xs leading-normal">
          Kerberos tools are not installed. Install <code>krb5-user</code> (or your platform's Kerberos client) to use GSSAPI authentication.
        </div>
      {:else}
        {#if status.client_configured}
          <div class="p-3.5 rounded-lg border border-accent/25 bg-accent/8 text-accent text-xs leading-normal">
            Kerberos client configured
            {#if status.default_realm}
              — default realm <code class="font-mono">{status.default_realm}</code>
            {/if}
          </div>
        {/if}

        <div class="border border-border bg-white/[0.01] rounded-xl px-4 py-3.5 flex flex-col gap-3">
          <div class="flex items-center justify-between pb-2.5 border-b border-border/60">
            <span class="text-xs text-secondary font-medium">Ticket status</span>
            <div class="flex items-center gap-1.5">
              <span class="w-2 h-2 rounded-full
                {health === 'valid' ? 'bg-success' :
                 health === 'warning' ? 'bg-warning' :
                 'bg-danger'}"
              ></span>
              <span class="text-xs font-semibold
                {health === 'valid' ? 'text-success' :
                 health === 'warning' ? 'text-warning' :
                 'text-danger'}"
              >
                {#if health === "valid"}Valid
                {:else if health === "warning"}Expiring soon
                {:else if health === "missing"}No ticket
                {:else}Expired{/if}
              </span>
            </div>
          </div>

          <div class="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2 text-[12px]">
            {#if status.principal && status.has_ticket}
              <span class="text-muted font-medium">Principal</span>
              <span class="text-primary font-mono text-right break-all">{status.principal}</span>
            {:else if status.suggested_principal}
              <span class="text-muted font-medium">Suggested principal</span>
              <span class="text-primary font-mono text-right break-all">{status.suggested_principal}</span>
            {/if}

            {#if status.config_path}
              <span class="text-muted font-medium">Config</span>
              <span class="text-primary font-mono text-[11px] text-right break-all">{status.config_path}</span>
            {/if}

            <span class="text-muted font-medium">Time remaining</span>
            <span class="font-mono text-right font-bold text-accent">{formatKerberosRemaining(remainingSeconds)}</span>

            {#if status.cache_path}
              <span class="text-muted font-medium">Cache path</span>
              <span class="text-primary font-mono text-[11px] text-right break-all">{status.cache_path}</span>
            {/if}

            {#if status.renewable && status.renew_until}
              <span class="text-muted font-medium">Renewable until</span>
              <span class="text-primary font-mono text-[11px] text-right">{new Date(status.renew_until * 1000).toLocaleString()}</span>
            {/if}
          </div>

          {#if status.has_ticket && remainingSeconds !== null}
            <div class="mt-1">
              <div class="h-1 rounded-full bg-white/5 overflow-hidden" aria-hidden="true">
                <div class="h-full rounded-full bg-gradient-to-r from-accent to-success transition-all duration-300" style:width="{progressPercent}%"></div>
              </div>
            </div>
          {/if}
        </div>

        {#if error}
          <div class="p-3.5 rounded-lg border border-danger/35 bg-danger/8 text-red-200 text-xs leading-normal">{error}</div>
        {/if}

        <div class="flex flex-col gap-3.5">
          <div class="border border-border rounded-xl p-4 bg-white/[0.01] flex flex-col gap-3">
            <div class="flex items-center gap-2 text-xs font-semibold text-primary">
              <RefreshCw size={14} />
              <span>{status.has_ticket ? "Renew ticket" : "Acquire ticket"}</span>
            </div>
            <p class="text-xs text-muted leading-relaxed">
              {#if status.has_ticket}
                Tries passwordless renewal first (<code>kinit -R</code>). Enter your password if renewal requires it.
              {:else}
                Enter your Kerberos principal and password to obtain a GSSAPI credential ticket.
              {/if}
            </p>

            {#if !status.has_ticket}
              <div class="flex flex-col gap-1.5">
                <label class="text-[11px] font-semibold text-secondary uppercase tracking-wider pl-0.5" for="kerberos-principal">Principal</label>
                <input
                  id="kerberos-principal"
                  type="text"
                  class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
                  placeholder="user@REALM.EXAMPLE"
                  bind:value={principal}
                  autocomplete="username"
                />
              </div>
            {/if}

            <label class="flex items-center gap-2 text-[12px] cursor-pointer select-none text-secondary py-1" for="kerberos-needs-password">
              <input id="kerberos-needs-password" type="checkbox" bind:checked={needsPassword} class="cursor-pointer accent-accent w-[16px] h-[16px]" />
              <span>Password required for renewal</span>
            </label>

            {#if !status.has_ticket || needsPassword}
              <div class="flex flex-col gap-1.5">
                <label class="text-[11px] font-semibold text-secondary uppercase tracking-wider pl-0.5" for="kerberos-password">Password</label>
                <div class="relative">
                  <input
                    id="kerberos-password"
                    type={showPassword ? "text" : "password"}
                    class="w-full bg-surface-input border border-border text-primary py-2 pl-3 pr-10 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
                    placeholder="Kerberos password"
                    bind:value={password}
                    autocomplete="current-password"
                  />
                  <button
                    type="button"
                    class="absolute right-2 top-1/2 -translate-y-1/2 border-none bg-transparent text-muted cursor-pointer inline-flex p-1 hover:text-primary"
                    onclick={() => (showPassword = !showPassword)}
                    aria-label={showPassword ? "Hide password" : "Show password"}
                  >
                    {#if showPassword}
                      <EyeOff size={16} />
                    {:else}
                      <Eye size={16} />
                    {/if}
                  </button>
                </div>
              </div>
            {/if}

            {#if !status.has_ticket}
              <div class="border border-border/85 rounded-xl bg-white/[0.01] overflow-hidden mt-1">
                <button
                  type="button"
                  class="w-full flex items-center justify-between px-4 py-3 bg-transparent border-none cursor-pointer text-xs font-semibold text-secondary hover:text-primary transition-colors outline-none"
                  onclick={() => (showAdvanced = !showAdvanced)}
                >
                  <div class="flex items-center gap-2">
                    <Settings2 size={14} class="text-muted" />
                    <span>Advanced Ticket Options</span>
                  </div>
                  {#if showAdvanced}
                    <ChevronUp size={14} class="text-muted" />
                  {:else}
                    <ChevronDown size={14} class="text-muted" />
                  {/if}
                </button>

                {#if showAdvanced}
                  <div class="px-4 pb-4 pt-1.5 border-t border-border/40 flex flex-col gap-3.5 bg-black/[0.05]">
                    <div class="flex items-center justify-between gap-4">
                      <div class="flex flex-col gap-0.5">
                        <span class="text-xs font-medium text-secondary">Forwardable ticket (-f)</span>
                        <span class="text-[10px] text-muted">Allow ticket to be forwarded to remote hosts</span>
                      </div>
                      <input
                        type="checkbox"
                        bind:checked={forwardable}
                        class="w-[16px] h-[16px] accent-accent cursor-pointer shrink-0"
                      />
                    </div>

                    <div class="flex items-center justify-between gap-4">
                      <div class="flex flex-col gap-0.5">
                        <span class="text-xs font-medium text-secondary">Proxiable ticket (-p)</span>
                        <span class="text-[10px] text-muted">Allow ticket to be proxied to other hosts</span>
                      </div>
                      <input
                        type="checkbox"
                        bind:checked={proxiable}
                        class="w-[16px] h-[16px] accent-accent cursor-pointer shrink-0"
                      />
                    </div>

                    <div class="grid grid-cols-2 gap-3">
                      <div class="flex flex-col gap-1.5">
                        <label for="krb-opt-lifetime" class="text-[10px] font-semibold text-muted uppercase tracking-wider pl-0.5">Ticket Lifetime (-l)</label>
                        <input
                          id="krb-opt-lifetime"
                          type="text"
                          placeholder="e.g. 10h, 1d (blank for default)"
                          bind:value={lifetime}
                          class="bg-surface-input border border-border text-primary py-1.5 px-3 rounded-lg outline-none text-xs transition-all duration-100 hover:border-border-hover focus:border-border-focus"
                        />
                      </div>

                      <div class="flex flex-col gap-1.5">
                        <label for="krb-opt-renew-lifetime" class="text-[10px] font-semibold text-muted uppercase tracking-wider pl-0.5">Renew Lifetime (-r)</label>
                        <input
                          id="krb-opt-renew-lifetime"
                          type="text"
                          placeholder="e.g. 7d (blank for default)"
                          bind:value={renewLifetime}
                          class="bg-surface-input border border-border text-primary py-1.5 px-3 rounded-lg outline-none text-xs transition-all duration-100 hover:border-border-hover focus:border-border-focus"
                        />
                      </div>
                    </div>
                  </div>
                {/if}
              </div>
            {/if}

            <button
              type="button"
              class="inline-flex items-center justify-center gap-2 py-2.5 px-4 rounded-lg border border-accent/35 bg-accent/12 text-accent text-xs font-semibold cursor-pointer transition-colors duration-100 hover:bg-accent/20 disabled:opacity-50 disabled:cursor-not-allowed"
              disabled={loading || (!status.has_ticket && !password.trim()) || (needsPassword && !password.trim())}
              onclick={() => (status.has_ticket ? handleRenew() : handleAcquire())}
            >
              <KeyRound size={14} />
              {loading ? "Working..." : status.has_ticket ? "Renew ticket" : "Acquire ticket"}
            </button>
          </div>
        </div>
      {/if}
    </div>

    <div class="flex justify-end gap-2 px-6 py-4 border-t border-border">
      <button
        type="button"
        class="py-2 px-4 rounded-lg text-[13px] font-semibold cursor-pointer bg-transparent border border-border text-secondary transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.03]"
        onclick={onClose}
      >
        Close
      </button>
    </div>
  </div>
</div>
