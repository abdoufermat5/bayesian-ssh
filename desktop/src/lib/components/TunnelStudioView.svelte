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
    X,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
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

  const defaultHostName = $derived(connections.length > 0 ? connections[0].name : "production-srv1");

  let tunnels = $state<TunnelRule[]>([
    {
      id: "1",
      connectionName: "production-srv1",
      type: "local",
      localPort: 8080,
      remoteHost: "localhost",
      remotePort: 80,
      active: true,
    },
    {
      id: "2",
      connectionName: "production-srv1",
      type: "socks5",
      localPort: 1080,
      active: true,
    },
    {
      id: "3",
      connectionName: "production-srv1",
      type: "local",
      localPort: 5432,
      remoteHost: "db.internal",
      remotePort: 5432,
      active: false,
    },
  ]);

  let showAddModal = $state(false);
  let newConnName = $state("");
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

  function applyPreset(p: typeof presets[number]) {
    newType = p.type;
    newLocalPort = p.local;
    if (p.remoteHost) newRemoteHost = p.remoteHost;
    if (p.remotePort) newRemotePort = p.remotePort;
  }

  function handleAddTunnel() {
    const host = newConnName || defaultHostName;
    const rule: TunnelRule = {
      id: Date.now().toString(),
      connectionName: host,
      type: newType,
      localPort: Number(newLocalPort),
      remoteHost: newType === "socks5" ? undefined : newRemoteHost,
      remotePort: newType === "socks5" ? undefined : Number(newRemotePort),
      active: true,
    };
    tunnels = [...tunnels, rule];
    showAddModal = false;
    notify(`Created ${rule.type.toUpperCase()} tunnel on port ${rule.localPort}`, "success");
  }

  function copyConnectionString(t: TunnelRule) {
    let str = "";
    if (t.type === "socks5") {
      str = `socks5://127.0.0.1:${t.localPort}`;
    } else {
      str = `http://localhost:${t.localPort}`;
    }
    navigator.clipboard.writeText(str);
    copiedId = t.id;
    notify(`Copied connection address: ${str}`, "success");
    setTimeout(() => {
      if (copiedId === t.id) copiedId = null;
    }, 2000);
  }

  const activeCount = $derived(tunnels.filter((t) => t.active).length);
</script>

<div class="flex flex-col flex-1 min-h-0 w-full overflow-hidden bg-surface select-none">
  <!-- Header Bar -->
  <div class="px-6 py-4 border-b border-border flex items-center justify-between gap-4 shrink-0 bg-surface-input/30">
    <div class="flex items-center gap-3">
      <div class="w-8 h-8 rounded-xl bg-accent/15 border border-accent/30 flex items-center justify-center text-accent shrink-0">
        <Network size={18} />
      </div>
      <div>
        <h2 class="text-sm font-bold text-primary tracking-tight m-0 flex items-center gap-2">
          Tunnel Studio
          <span class="badge-pill bg-accent/15 text-accent border border-accent/30 text-[10px]">
            {activeCount} active
          </span>
        </h2>
        <p class="text-[11px] text-muted m-0">SSH Port Forwarding &amp; Dynamic SOCKS5 Proxies</p>
      </div>
    </div>

    <button
      type="button"
      class="btn btn-primary shadow-sm"
      onclick={() => {
        newConnName = defaultHostName;
        showAddModal = true;
      }}
    >
      <Plus size={14} />
      <span>New Tunnel</span>
    </button>
  </div>

  <!-- Presets Strip -->
  <div class="px-6 py-2.5 border-b border-border/80 bg-surface-input/10 flex items-center gap-2 overflow-x-auto shrink-0 scrollbar-none">
    <span class="eyebrow text-[10px] whitespace-nowrap">Quick Presets:</span>
    {#each presets as p}
      <button
        type="button"
        class="flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-[11px] font-semibold text-secondary hover:text-primary hover:bg-surface-hover border border-border transition-colors cursor-pointer whitespace-nowrap bg-surface"
        onclick={() => {
          applyPreset(p);
          newConnName = defaultHostName;
          showAddModal = true;
        }}
      >
        <Sparkles size={11} class="text-warning" />
        <span>{p.label}</span>
      </button>
    {/each}
  </div>

  <!-- Active Tunnels List -->
  <div class="flex-1 min-h-0 overflow-y-auto px-6 py-4 space-y-3 scrollbar-none">
    {#if tunnels.length > 0}
      <div class="grid grid-cols-1 md:grid-cols-2 gap-3.5">
        {#each tunnels as t (t.id)}
          <div
            class="rounded-xl border p-4 transition-all duration-fast flex flex-col justify-between
              {t.active
                ? 'border-accent/40 bg-surface-input/60 shadow-sm'
                : 'border-border bg-surface-input/20 opacity-70'}"
          >
            <!-- Card Header -->
            <div>
              <div class="flex items-center justify-between gap-2 mb-3">
                <div class="flex items-center gap-2">
                  <span class="w-2 h-2 rounded-full {t.active ? 'bg-running' : 'bg-muted/40'}"></span>
                  <span class="font-bold text-xs text-primary uppercase tracking-wider font-mono">
                    {t.type} tunnel
                  </span>
                  <span class="badge-pill bg-surface border border-border text-secondary text-[10px]">
                    {t.connectionName}
                  </span>
                </div>

                <!-- Active Toggle Switch -->
                <button
                  type="button"
                  role="switch"
                  aria-label="Toggle Tunnel"
                  aria-checked={t.active}
                  onclick={() => toggleTunnel(t.id)}
                  class="relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full border-2 border-transparent transition-colors duration-fast focus:outline-none
                    {t.active ? 'bg-accent' : 'bg-surface-hover border-border'}"
                >
                  <span
                    class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow transition duration-fast
                      {t.active ? 'translate-x-4' : 'translate-x-0'}"
                  ></span>
                </button>
              </div>

              <!-- Visual Flow Route Diagram -->
              <div class="p-3 rounded-lg bg-surface border border-border/70 flex items-center justify-between gap-2 font-mono text-xs mb-3">
                <div class="flex flex-col">
                  <span class="text-[9px] uppercase tracking-wider text-muted font-sans font-bold">Local Endpoint</span>
                  <span class="font-semibold text-primary">127.0.0.1:{t.localPort}</span>
                </div>

                <div class="flex flex-col items-center gap-0.5 px-2">
                  <ArrowRight size={14} class={t.active ? "text-accent animate-pulse" : "text-muted"} />
                  <span class="text-[9px] text-muted font-sans font-semibold">via SSH</span>
                </div>

                <div class="flex flex-col text-right">
                  <span class="text-[9px] uppercase tracking-wider text-muted font-sans font-bold">Remote Target</span>
                  {#if t.type === "socks5"}
                    <span class="font-semibold text-cyan-400">Dynamic Proxy</span>
                  {:else}
                    <span class="font-semibold text-primary">{t.remoteHost}:{t.remotePort}</span>
                  {/if}
                </div>
              </div>
            </div>

            <!-- Footer Quick Actions -->
            <div class="flex items-center justify-between pt-2 border-t border-border/60">
              <span class="text-[10px] text-muted">
                {t.active ? "🟢 Forwarding traffic" : "⚪ Stopped"}
              </span>

              <div class="flex items-center gap-1.5">
                <button
                  type="button"
                  class="btn-icon p-1 text-muted hover:text-primary transition-colors cursor-pointer border-none bg-transparent"
                  onclick={() => copyConnectionString(t)}
                  title="Copy local connection address"
                >
                  {#if copiedId === t.id}
                    <Check size={13} class="text-running" />
                  {:else}
                    <Copy size={13} />
                  {/if}
                </button>

                <button
                  type="button"
                  class="btn-icon p-1 text-muted hover:text-error hover:bg-error/15 transition-colors cursor-pointer border-none bg-transparent"
                  onclick={() => deleteTunnel(t.id)}
                  title="Delete tunnel rule"
                >
                  <Trash2 size={13} />
                </button>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="py-20 flex flex-col items-center justify-center text-muted border border-dashed border-border rounded-2xl bg-surface-input/10">
        <div class="w-12 h-12 rounded-2xl bg-accent/10 border border-accent/25 flex items-center justify-center text-accent mb-3">
          <Network size={24} />
        </div>
        <h3 class="text-sm font-bold text-primary mb-1">No Active SSH Tunnels</h3>
        <p class="text-xs text-muted max-w-sm text-center mb-4 leading-relaxed">
          Forward local ports securely through your SSH connection or establish a dynamic SOCKS5 proxy.
        </p>
        <button
          type="button"
          class="btn btn-primary"
          onclick={() => (showAddModal = true)}
        >
          <Plus size={14} />
          <span>Create First Tunnel</span>
        </button>
      </div>
    {/if}
  </div>
</div>

<!-- Add Tunnel Modal -->
{#if showAddModal}
  <ModalShell
    open={true}
    title="Create SSH Tunnel Rule"
    onClose={() => (showAddModal = false)}
    width="md"
  >
    <div class="px-6 py-4 border-b border-border flex items-center justify-between">
      <h3 class="text-base font-bold text-primary m-0">Create SSH Tunnel</h3>
      <button
        type="button"
        class="text-muted hover:text-primary p-1 rounded-md border-none bg-transparent cursor-pointer"
        onclick={() => (showAddModal = false)}
      >
        <X size={16} />
      </button>
    </div>

    <div class="p-6 flex flex-col gap-4">
      <!-- Target Host -->
      <div class="flex flex-col gap-1.5">
        <label for="tunnel-conn-select" class="text-[11px] font-semibold text-muted uppercase tracking-wider">SSH Host Connection</label>
        <CustomSelect
          id="tunnel-conn-select"
          options={connections.map((c) => ({ value: c.name, label: `${c.name} (${c.user}@${c.host})` }))}
          value={newConnName}
          onChange={(val) => (newConnName = val)}
        />
      </div>

      <!-- Tunnel Type -->
      <div class="flex flex-col gap-1.5">
        <span class="text-[11px] font-semibold text-muted uppercase tracking-wider">Forwarding Type</span>
        <div class="grid grid-cols-3 gap-2">
          <button
            type="button"
            class="p-2.5 rounded-lg border text-xs font-semibold cursor-pointer transition-all text-center
              {newType === 'local' ? 'border-accent bg-accent/15 text-primary' : 'border-border bg-surface-input text-muted'}"
            onclick={() => (newType = 'local')}
          >
            Local (-L)
          </button>
          <button
            type="button"
            class="p-2.5 rounded-lg border text-xs font-semibold cursor-pointer transition-all text-center
              {newType === 'socks5' ? 'border-accent bg-accent/15 text-primary' : 'border-border bg-surface-input text-muted'}"
            onclick={() => (newType = 'socks5')}
          >
            SOCKS5 (-D)
          </button>
          <button
            type="button"
            class="p-2.5 rounded-lg border text-xs font-semibold cursor-pointer transition-all text-center
              {newType === 'remote' ? 'border-accent bg-accent/15 text-primary' : 'border-border bg-surface-input text-muted'}"
            onclick={() => (newType = 'remote')}
          >
            Remote (-R)
          </button>
        </div>
      </div>

      <!-- Ports Configuration -->
      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1.5">
          <label for="local-port-input" class="text-[11px] font-semibold text-muted uppercase tracking-wider">Local Port</label>
          <input
            id="local-port-input"
            type="number"
            bind:value={newLocalPort}
            class="input"
            placeholder="8080"
          />
        </div>

        {#if newType !== "socks5"}
          <div class="flex flex-col gap-1.5">
            <label for="remote-port-input" class="text-[11px] font-semibold text-muted uppercase tracking-wider">Remote Port</label>
            <input
              id="remote-port-input"
              type="number"
              bind:value={newRemotePort}
              class="input"
              placeholder="80"
            />
          </div>
        {/if}
      </div>

      {#if newType !== "socks5"}
        <div class="flex flex-col gap-1.5">
          <label for="remote-host-input" class="text-[11px] font-semibold text-muted uppercase tracking-wider">Remote Host</label>
          <input
            id="remote-host-input"
            type="text"
            bind:value={newRemoteHost}
            class="input"
            placeholder="localhost or db.internal"
          />
        </div>
      {/if}

      <!-- Visual Preview -->
      <div class="p-3 rounded-lg bg-surface-input border border-border/80 font-mono text-xs flex items-center justify-between text-muted">
        <span>127.0.0.1:{newLocalPort}</span>
        <ArrowRight size={13} class="text-accent" />
        <span>{newType === 'socks5' ? 'Dynamic SOCKS5' : `${newRemoteHost}:${newRemotePort}`}</span>
      </div>

      <!-- Modal Footer -->
      <div class="flex justify-end gap-2 pt-2 border-t border-border">
        <button
          type="button"
          class="btn btn-secondary"
          onclick={() => (showAddModal = false)}
        >
          Cancel
        </button>
        <button
          type="button"
          class="btn btn-primary"
          onclick={handleAddTunnel}
        >
          Add Tunnel
        </button>
      </div>
    </div>
  </ModalShell>
{/if}
