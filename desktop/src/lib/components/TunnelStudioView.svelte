<script lang="ts">
  import {
    Activity,
    ArrowRight,
    Check,
    Copy,
    ExternalLink,
    Globe,
    Layers,
    Network,
    Plus,
    Power,
    RefreshCw,
    Server,
    Shield,
    Sparkles,
    Trash2,
    Workflow,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import { notify } from "$lib/stores/notifications.svelte";

  export interface TunnelRule {
    id: string;
    connectionName: string;
    type: "local" | "remote" | "socks5";
    localPort: number;
    remoteHost?: string;
    remotePort?: number;
    active: boolean;
  }

  interface Props {
    connections: Connection[];
  }

  let { connections }: Props = $props();

  let tunnels = $state<TunnelRule[]>([
    {
      id: "1",
      connectionName: connections.length > 0 ? connections[0].name : "production-srv1",
      type: "local",
      localPort: 8080,
      remoteHost: "localhost",
      remotePort: 80,
      active: true,
    },
    {
      id: "2",
      connectionName: connections.length > 0 ? connections[0].name : "production-srv1",
      type: "socks5",
      localPort: 1080,
      active: true,
    },
    {
      id: "3",
      connectionName: connections.length > 0 ? connections[0].name : "production-srv1",
      type: "local",
      localPort: 5432,
      remoteHost: "db.internal",
      remotePort: 5432,
      active: false,
    },
  ]);

  let showAddModal = $state(false);
  let newConnName = $state(connections.length > 0 ? connections[0].name : "");
  let newType = $state<"local" | "remote" | "socks5">("local");
  let newLocalPort = $state(8080);
  let newRemoteHost = $state("localhost");
  let newRemotePort = $state(80);
  let copiedId = $state<string | null>(null);

  const presets = [
    { label: "Web Server (8080:80)", type: "local" as const, local: 8080, remoteHost: "localhost", remotePort: 80 },
    { label: "PostgreSQL (5432:5432)", type: "local" as const, local: 5432, remoteHost: "localhost", remotePort: 5432 },
    { label: "Redis DB (6379:6379)", type: "local" as const, local: 6379, remoteHost: "localhost", remotePort: 6379 },
    { label: "SOCKS5 Proxy (1080)", type: "socks5" as const, local: 1080 },
  ];

  function toggleTunnel(id: string) {
    const idx = tunnels.findIndex((t) => t.id === id);
    if (idx !== -1) {
      tunnels[idx].active = !tunnels[idx].active;
      const t = tunnels[idx];
      if (t.active) {
        notify(`Started ${t.type.toUpperCase()} tunnel on port ${t.localPort}`, "success");
      } else {
        notify(`Stopped tunnel on port ${t.localPort}`, "info");
      }
      tunnels = [...tunnels];
    }
  }

  function deleteTunnel(id: string) {
    tunnels = tunnels.filter((t) => t.id !== id);
    notify("Tunnel rule deleted", "info");
  }

  function applyPreset(p: typeof presets[0]) {
    newType = p.type;
    newLocalPort = p.local;
    if (p.type !== "socks5") {
      newRemoteHost = p.remoteHost || "localhost";
      newRemotePort = p.remotePort || 80;
    }
  }

  function handleAddTunnel() {
    if (!newConnName) return;
    const rule: TunnelRule = {
      id: String(Date.now()),
      connectionName: newConnName,
      type: newType,
      localPort: newLocalPort,
      remoteHost: newType !== "socks5" ? newRemoteHost : undefined,
      remotePort: newType !== "socks5" ? newRemotePort : undefined,
      active: true,
    };
    tunnels = [...tunnels, rule];
    showAddModal = false;
    notify(`Created ${newType.toUpperCase()} tunnel on port ${newLocalPort}`, "success");
  }

  function copyTunnelCommand(t: TunnelRule) {
    const conn = connections.find((c) => c.name === t.connectionName);
    const hostStr = conn ? `${conn.user}@${conn.host}` : t.connectionName;
    let cmd = "";
    if (t.type === "local") {
      cmd = `ssh -N -L ${t.localPort}:${t.remoteHost || "localhost"}:${t.remotePort || 80} ${hostStr}`;
    } else if (t.type === "socks5") {
      cmd = `ssh -N -D ${t.localPort} ${hostStr}`;
    } else {
      cmd = `ssh -N -R ${t.remotePort}:${t.remoteHost || "localhost"}:${t.localPort} ${hostStr}`;
    }

    navigator.clipboard.writeText(cmd).then(() => {
      copiedId = t.id;
      setTimeout(() => (copiedId = null), 2000);
      notify(`Copied SSH tunnel command to clipboard`, "info");
    });
  }

  function openLocalUrl(port: number) {
    window.open(`http://localhost:${port}`, "_blank");
  }
</script>

<div class="flex flex-col flex-1 min-h-0 w-full bg-surface p-5 gap-5">
  <!-- Top Header Bar -->
  <div class="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 shrink-0 pb-1 border-b border-border/40">
    <div class="flex items-center gap-3">
      <div class="p-2.5 rounded-xl bg-accent/10 border border-accent/20 text-accent shadow-xs">
        <Network size={22} />
      </div>
      <div>
        <h2 class="text-base font-bold text-primary m-0 flex items-center gap-2">
          <span>SSH Tunnel & Port Forwarding Studio</span>
          <span class="px-2 py-0.5 rounded-full bg-cyan-500/10 border border-cyan-500/20 text-cyan-400 text-[10px] font-semibold flex items-center gap-1">
            <Workflow size={11} />
            Visual Proxy Pipeline
          </span>
        </h2>
        <p class="text-xs text-muted mt-0.5 m-0">
          Configure Local (-L), Remote (-R), and Dynamic SOCKS5 (-D) encrypted SSH tunnels with live flow telemetry
        </p>
      </div>
    </div>

    <button
      type="button"
      class="inline-flex items-center gap-2 px-4 py-2 rounded-xl bg-accent text-white text-xs font-semibold cursor-pointer hover:opacity-90 transition-all shadow-sm"
      onclick={() => (showAddModal = true)}
    >
      <Plus size={16} />
      <span>New Tunnel</span>
    </button>
  </div>

  <!-- Tunnel Cards Grid -->
  <div class="flex-1 min-h-0 overflow-y-auto grid grid-cols-1 lg:grid-cols-2 gap-4 pr-1">
    {#each tunnels as t (t.id)}
      <div
        class="bg-surface-card border border-border rounded-xl p-4 flex flex-col gap-4 shadow-sm hover:border-border-hover transition-all relative overflow-hidden group"
      >
        <!-- Top Status Row -->
        <div class="flex items-center justify-between gap-3">
          <div class="flex items-center gap-2.5">
            {#if t.active}
              <div class="relative flex h-3 w-3 items-center justify-center">
                <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
                <span class="relative inline-flex rounded-full h-2.5 w-2.5 bg-emerald-500"></span>
              </div>
            {:else}
              <div class="h-2.5 w-2.5 rounded-full bg-muted"></div>
            {/if}

            <span class="text-xs font-bold text-primary uppercase tracking-wider">
              {t.type === "socks5" ? "Dynamic SOCKS5 (-D)" : t.type === "local" ? "Local Forward (-L)" : "Remote Forward (-R)"}
            </span>
          </div>

          <!-- Power Toggle Button -->
          <button
            type="button"
            class="px-3 py-1.5 rounded-lg border text-xs font-semibold cursor-pointer transition-all flex items-center gap-1.5
              {t.active ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-400 hover:bg-emerald-500/20' : 'bg-surface-input border-border text-muted hover:text-primary'}"
            onclick={() => toggleTunnel(t.id)}
          >
            <Power size={13} />
            <span>{t.active ? "Active" : "Disabled"}</span>
          </button>
        </div>

        <!-- Visual Flow Diagram Card -->
        <div class="bg-surface-terminal p-3.5 rounded-xl border border-border flex items-center justify-between gap-3 font-mono text-xs text-primary shadow-inner">
          <!-- Client Local Endpoint -->
          <div class="flex items-center gap-2 min-w-0">
            <div class="p-1.5 rounded bg-cyan-500/10 border border-cyan-500/20 text-cyan-400 shrink-0">
              <Globe size={14} />
            </div>
            <div class="flex flex-col min-w-0">
              <span class="text-[10px] text-muted">Local</span>
              <span class="font-bold text-xs truncate">127.0.0.1:{t.localPort}</span>
            </div>
          </div>

          <!-- Flow Arrow / Tunnel Pipe -->
          <div class="flex flex-col items-center justify-center shrink-0 px-2">
            <span class="text-[9px] text-accent font-bold uppercase tracking-wider mb-0.5">SSH Tunnel</span>
            <div class="flex items-center gap-1 text-accent">
              <span class="w-4 h-[2px] bg-accent/40 rounded"></span>
              <ArrowRight size={14} class={t.active ? "animate-pulse text-accent" : "text-muted"} />
              <span class="w-4 h-[2px] bg-accent/40 rounded"></span>
            </div>
          </div>

          <!-- Target Remote Endpoint -->
          <div class="flex items-center gap-2 min-w-0 justify-end text-right">
            <div class="flex flex-col min-w-0">
              <span class="text-[10px] text-muted">Target</span>
              <span class="font-bold text-xs truncate">
                {t.type === "socks5" ? "SOCKS5 Proxy" : `${t.remoteHost}:${t.remotePort}`}
              </span>
            </div>
            <div class="p-1.5 rounded bg-amber-500/10 border border-amber-500/20 text-amber-400 shrink-0">
              <Server size={14} />
            </div>
          </div>
        </div>

        <!-- Host Name & Action Toolbar -->
        <div class="flex items-center justify-between pt-1 border-t border-border/40 text-xs">
          <span class="text-muted flex items-center gap-1.5 text-[11px] font-medium">
            <Server size={13} class="text-accent" />
            <span>Host: <strong class="text-primary font-semibold">{t.connectionName}</strong></span>
          </span>

          <div class="flex items-center gap-1">
            {#if t.type === "local" && t.active}
              <button
                type="button"
                class="px-2.5 py-1 rounded-lg bg-surface-input border border-border text-secondary hover:text-primary text-[11px] font-medium flex items-center gap-1 transition-all"
                title="Open local endpoint in browser"
                onclick={() => openLocalUrl(t.localPort)}
              >
                <ExternalLink size={13} />
                <span>Open URL</span>
              </button>
            {/if}

            <button
              type="button"
              class="p-1.5 rounded-lg text-muted hover:text-primary hover:bg-white/10 transition-colors"
              title="Copy SSH tunnel command"
              onclick={() => copyTunnelCommand(t)}
            >
              {#if copiedId === t.id}
                <Check size={14} class="text-emerald-400" />
              {:else}
                <Copy size={14} />
              {/if}
            </button>

            <button
              type="button"
              class="p-1.5 rounded-lg text-muted hover:text-danger hover:bg-danger/10 transition-colors"
              title="Delete tunnel rule"
              onclick={() => deleteTunnel(t.id)}
            >
              <Trash2 size={14} />
            </button>
          </div>
        </div>
      </div>
    {/each}
  </div>
</div>

<!-- New Tunnel Modal -->
{#if showAddModal}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-md p-4">
    <div class="bg-surface border border-border rounded-xl w-full max-w-lg p-6 shadow-2xl flex flex-col gap-5">
      <div class="flex items-center justify-between pb-2 border-b border-border">
        <h3 class="text-base font-bold text-primary m-0 flex items-center gap-2">
          <Network size={18} class="text-accent" />
          <span>Create SSH Tunnel Rule</span>
        </h3>
      </div>

      <!-- Presets Selector -->
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-semibold text-secondary">Quick Presets</span>
        <div class="grid grid-cols-2 gap-2">
          {#each presets as p (p.label)}
            <button
              type="button"
              class="px-2.5 py-1.5 rounded-lg bg-surface-input border border-border text-secondary hover:text-primary hover:border-accent/40 text-[11px] font-medium cursor-pointer transition-all text-left truncate"
              onclick={() => applyPreset(p)}
            >
              ⚡ {p.label}
            </button>
          {/each}
        </div>
      </div>

      <div class="flex flex-col gap-4">
        <!-- Target Connection -->
        <div class="flex flex-col gap-1.5">
          <label for="new-tunnel-conn" class="text-xs font-semibold text-secondary">SSH Server Host</label>
          <CustomSelect
            id="new-tunnel-conn"
            options={connections.map((c) => ({ value: c.name, label: `${c.name} (${c.user}@${c.host})` }))}
            value={newConnName}
            onChange={(val) => (newConnName = val)}
          />
        </div>

        <!-- Tunnel Type -->
        <div class="flex flex-col gap-1.5">
          <label for="new-tunnel-type" class="text-xs font-semibold text-secondary">Tunnel Mode</label>
          <CustomSelect
            id="new-tunnel-type"
            options={[
              { value: "local", label: "Local Forward (-L) - Expose Remote Service Locally" },
              { value: "socks5", label: "Dynamic SOCKS5 (-D) - Full Proxy Adapter" },
              { value: "remote", label: "Remote Forward (-R) - Expose Local Port to Remote" },
            ]}
            value={newType}
            onChange={(val) => (newType = val as "local" | "remote" | "socks5")}
          />
        </div>

        <!-- Ports Grid -->
        <div class="grid grid-cols-2 gap-3">
          <div class="flex flex-col gap-1.5">
            <label for="new-tunnel-localport" class="text-xs font-semibold text-secondary">Local Port</label>
            <input
              id="new-tunnel-localport"
              type="number"
              bind:value={newLocalPort}
              class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-xs font-mono"
            />
          </div>

          {#if newType !== "socks5"}
            <div class="flex flex-col gap-1.5">
              <label for="new-tunnel-remoteport" class="text-xs font-semibold text-secondary">Remote Port</label>
              <input
                id="new-tunnel-remoteport"
                type="number"
                bind:value={newRemotePort}
                class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-xs font-mono"
              />
            </div>
          {/if}
        </div>

        {#if newType !== "socks5"}
          <div class="flex flex-col gap-1.5">
            <label for="new-tunnel-remotehost" class="text-xs font-semibold text-secondary">Remote Host Endpoint</label>
            <input
              id="new-tunnel-remotehost"
              type="text"
              bind:value={newRemoteHost}
              placeholder="localhost"
              class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-xs font-mono"
            />
          </div>
        {/if}
      </div>

      <!-- Action Buttons -->
      <div class="flex items-center justify-end gap-3 pt-2 border-t border-border">
        <button
          type="button"
          class="px-4 py-2 rounded-lg bg-surface-input border border-border text-secondary text-xs font-semibold cursor-pointer hover:bg-white/5"
          onclick={() => (showAddModal = false)}
        >
          Cancel
        </button>
        <button
          type="button"
          class="px-4 py-2 rounded-lg bg-accent text-white text-xs font-semibold cursor-pointer hover:opacity-90 shadow-sm"
          onclick={handleAddTunnel}
        >
          Create Tunnel
        </button>
      </div>
    </div>
  </div>
{/if}
