<script lang="ts">
  import { Eye, EyeOff, KeyRound, RefreshCw, ShieldCheck, X } from "lucide-svelte";
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
    onAcquire: (principal: string, password: string) => void | Promise<void>;
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
  let principal = $state(status.principal ?? "");
  let showPassword = $state(false);
  let needsPassword = $state(false);

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
    if (status.principal && !principal) {
      principal = status.principal;
    }
  });

  async function handleRenew() {
    await onRenew(needsPassword ? password : undefined);
  }

  async function handleAcquire() {
    await onAcquire(principal, password);
  }
</script>

<div class="modal-backdrop" onclick={onClose} role="dialog" aria-modal="true" aria-labelledby="kerberos-modal-title">
  <div class="kerberos-dialog" onclick={(e) => e.stopPropagation()}>
    <div class="kerberos-header">
      <div class="kerberos-title-wrap">
        <ShieldCheck size={20} />
        <div>
          <h2 id="kerberos-modal-title">Kerberos Ticket</h2>
          <p>Keep your GSSAPI sessions alive with a valid ticket</p>
        </div>
      </div>
      <button type="button" class="icon-btn" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="kerberos-body">
      {#if !status.tools_available}
        <div class="kerberos-alert danger">
          Kerberos tools are not installed. Install <code>krb5-user</code> (or your platform's Kerberos client) to use GSSAPI authentication.
        </div>
      {:else}
        <div class="kerberos-status-card" class:warning={health === "warning"} class:expired={health === "expired" || health === "missing"}>
          <div class="status-row">
            <span class="status-label">Status</span>
            <span class="status-value {health}">
              {#if health === "valid"}Valid
              {:else if health === "warning"}Expiring soon
              {:else if health === "missing"}No ticket
              {:else}Expired{/if}
            </span>
          </div>

          {#if status.principal}
            <div class="status-row">
              <span class="status-label">Principal</span>
              <span class="status-mono">{status.principal}</span>
            </div>
          {/if}

          {#if status.cache_path}
            <div class="status-row">
              <span class="status-label">Cache</span>
              <span class="status-mono">{status.cache_path}</span>
            </div>
          {/if}

          <div class="status-row">
            <span class="status-label">Time remaining</span>
            <span class="status-countdown">{formatKerberosRemaining(remainingSeconds)}</span>
          </div>

          {#if status.has_ticket && remainingSeconds !== null}
            <div class="progress-track" aria-hidden="true">
              <div class="progress-fill" style:width="{progressPercent}%"></div>
            </div>
          {/if}

          {#if status.renewable && status.renew_until}
            <div class="renew-hint">
              Renewable until {new Date(status.renew_until * 1000).toLocaleString()}
            </div>
          {/if}
        </div>

        {#if error}
          <div class="kerberos-alert danger">{error}</div>
        {/if}

        <div class="kerberos-actions">
          <div class="action-block">
            <div class="action-heading">
              <RefreshCw size={14} />
              <span>{status.has_ticket ? "Renew ticket" : "Acquire ticket"}</span>
            </div>
            <p class="action-copy">
              {#if status.has_ticket}
                Tries passwordless renewal first (<code>kinit -R</code>). Enter your password if renewal requires it.
              {:else}
                Enter your Kerberos principal and password to obtain a forwardable ticket.
              {/if}
            </p>

            {#if !status.has_ticket}
              <label class="field-label" for="kerberos-principal">Principal</label>
              <input
                id="kerberos-principal"
                type="text"
                class="cyber-input"
                placeholder="user@REALM.EXAMPLE"
                bind:value={principal}
                autocomplete="username"
              />
            {/if}

            <label class="checkbox-row" for="kerberos-needs-password">
              <input id="kerberos-needs-password" type="checkbox" bind:checked={needsPassword} />
              <span>Password required for renewal</span>
            </label>

            {#if !status.has_ticket || needsPassword}
              <label class="field-label" for="kerberos-password">Password</label>
              <div class="password-field">
                <input
                  id="kerberos-password"
                  type={showPassword ? "text" : "password"}
                  class="cyber-input"
                  placeholder="Kerberos password"
                  bind:value={password}
                  autocomplete="current-password"
                />
                <button
                  type="button"
                  class="password-toggle"
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
            {/if}

            <button
              type="button"
              class="primary-btn"
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

    <div class="kerberos-footer">
      <button type="button" class="cancel-btn" onclick={onClose}>Close</button>
    </div>
  </div>
</div>

<style>
  .kerberos-dialog {
    width: min(520px, calc(100vw - 32px));
    display: flex;
    flex-direction: column;
    background: var(--bg-card);
    border: 1px solid var(--border-color);
    border-radius: 14px;
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.45);
    overflow: hidden;
  }

  .kerberos-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 18px 20px 12px;
    border-bottom: 1px solid var(--border-color);
    color: var(--accent-cyan);
  }

  .kerberos-title-wrap {
    display: flex;
    gap: 12px;
    align-items: flex-start;
  }

  .kerberos-title-wrap h2 {
    margin: 0 0 4px;
    font-size: 18px;
    color: var(--text-primary);
  }

  .kerberos-title-wrap p {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .icon-btn {
    width: 32px;
    height: 32px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .kerberos-body {
    padding: 16px 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .kerberos-status-card {
    border: 1px solid rgba(0, 240, 255, 0.25);
    background: rgba(0, 240, 255, 0.05);
    border-radius: 12px;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .kerberos-status-card.warning {
    border-color: rgba(245, 158, 11, 0.35);
    background: rgba(245, 158, 11, 0.08);
  }

  .kerberos-status-card.expired {
    border-color: rgba(239, 68, 68, 0.35);
    background: rgba(239, 68, 68, 0.08);
  }

  .status-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    align-items: baseline;
    font-size: 12px;
  }

  .status-label {
    color: var(--text-muted);
  }

  .status-mono,
  .status-countdown {
    font-family: monospace;
    color: var(--text-primary);
    text-align: right;
    word-break: break-all;
  }

  .status-countdown {
    font-size: 14px;
    font-weight: 700;
    color: var(--accent-cyan);
  }

  .status-value.valid { color: #34d399; }
  .status-value.warning { color: #fbbf24; }
  .status-value.expired,
  .status-value.missing { color: #f87171; }

  .progress-track {
    height: 6px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--accent-cyan), #34d399);
    transition: width 0.4s ease;
  }

  .renew-hint {
    font-size: 11px;
    color: var(--text-muted);
  }

  .kerberos-alert {
    padding: 10px 12px;
    border-radius: 8px;
    font-size: 12px;
    line-height: 1.5;
  }

  .kerberos-alert.danger {
    border: 1px solid rgba(239, 68, 68, 0.35);
    background: rgba(239, 68, 68, 0.08);
    color: #fecaca;
  }

  .kerberos-actions {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .action-block {
    border: 1px solid var(--border-color);
    border-radius: 12px;
    padding: 14px;
    background: rgba(255, 255, 255, 0.02);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .action-heading {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .action-copy {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    line-height: 1.5;
  }

  .field-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .password-field {
    position: relative;
  }

  .password-field .cyber-input {
    width: 100%;
    padding-right: 40px;
    box-sizing: border-box;
  }

  .password-toggle {
    position: absolute;
    right: 8px;
    top: 50%;
    transform: translateY(-50%);
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    display: inline-flex;
    padding: 4px;
  }

  .primary-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 10px 14px;
    border-radius: 8px;
    border: 1px solid rgba(0, 240, 255, 0.35);
    background: rgba(0, 240, 255, 0.12);
    color: var(--accent-cyan);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .primary-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .kerberos-footer {
    padding: 12px 20px 18px;
    border-top: 1px solid var(--border-color);
    display: flex;
    justify-content: flex-end;
  }
</style>
