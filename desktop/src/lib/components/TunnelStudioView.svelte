<script lang="ts">
  import {
    ArrowRight,
    Check,
    Copy,
    Network,
    Plus,
    Power,
    RefreshCw,
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
    { label: "Web", type: "local" as const, local: 8080, remoteHost: "localhost", remotePort: 80 },
    { label: "Postgres", type: "local" as const, local: 5432, remoteHost: "localhost", remotePort: 5432 },
    { label: "Redis", type: "local" as const, local: 6379, remoteHost: "localhost", remotePort: 6379 },
    { label: "SOCKS5", type: "socks5" as const, local: 1080 },
  ];

  function toggleTunnel(id: string) {
    const idx = tunnels.findIndex((t) => t.id === id);
    if (idx !== -1) {
      tunnels[idx].active = !tunnels[idx].active;
      const t = tunnels[idx];
      notify(
        t.active ? `Started ${t.type.toUpperCase()} tunnel on port ${t.localPort}` : `Stopped tunnel on port ${t.localPort}`,
        t.active ? "success" : "info",
      );
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
    const str = t.type === "socks5" ? `socks5://127.0.0.1:${t.localPort}` : `http://localhost:${t.localPort}`;
    navigator.clipboard.writeText(str);
    copiedId = t.id;
    notify(`Copied connection address: ${str}`, "success");
    setTimeout(() => {
      if (copiedId === t.id) copiedId = null;
    }, 2000);
  }

  function targetLabel(t: TunnelRule) {
    return t.type === "socks5" ? "Dynamic SOCKS5" : `${t.remoteHost}:${t.remotePort}`;
  }

  const activeCount = $derived(tunnels.filter((t) => t.active).length);
</script>

<div class="flex min-h-0 w-full flex-1 flex-col overflow-hidden bg-surface text-[13px] text-primary select-none">
  <div class="view-header">
    <div class="flex min-w-0 items-center gap-3">
      <div class="icon-tile rounded-md">
        <Network size={16} />
      </div>
      <div class="min-w-0">
        <h2 class="m-0 flex items-center gap-2 truncate text-sm font-bold tracking-tight text-primary">
          Tunnel Studio
          <span class="badge badge-running">{activeCount} active</span>
          <span class="badge badge-subtle">Local rules</span>
        </h2>
        <p class="m-0 truncate text-xs text-muted">Port-forwarding rules currently managed in local UI state</p>
      </div>
    </div>

    <button
      type="button"
      class="btn btn-primary"
      onclick={() => {
        newConnName = defaultHostName;
        showAddModal = true;
      }}
    >
      <Plus size={14} />
      <span>New Tunnel</span>
    </button>
  </div>

  <div class="toolbar-band">
    <span class="table-header whitespace-nowrap">Presets</span>
    <div class="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto">
      {#each presets as p}
        <button
          type="button"
          class="btn btn-secondary btn-sm"
          onclick={() => {
            applyPreset(p);
            newConnName = defaultHostName;
            showAddModal = true;
          }}
        >
          <Plus size={12} />
          <span>{p.label}</span>
        </button>
      {/each}
    </div>
  </div>

  <div class="view-content">
    {#if tunnels.length > 0}
      <div class="data-table overflow-x-auto">
        <table class="w-full border-collapse">
          <thead>
            <tr class="border-b border-border bg-surface-input/80 text-left table-header">
              <th class="px-4 py-2.5 font-bold">State</th>
              <th class="px-4 py-2.5 font-bold">Connection</th>
              <th class="px-4 py-2.5 font-bold">Type</th>
              <th class="px-4 py-2.5 font-bold">Route</th>
              <th class="px-4 py-2.5 text-right font-bold">Actions</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-border/60">
            {#each tunnels as t (t.id)}
              <tr class="row border-0 text-xs text-secondary">
                <td class="px-4 py-2.5">
                  <button
                    type="button"
                    role="switch"
                    aria-label="Toggle tunnel"
                    aria-checked={t.active}
                    onclick={() => toggleTunnel(t.id)}
                    class="inline-flex items-center gap-2 border-none bg-transparent text-xs font-semibold {t.active ? 'text-running' : 'text-muted'}"
                  >
                    <span class="status-dot status-dot-sm {t.active ? 'status-dot-running' : 'status-dot-offline'}"></span>
                    {t.active ? "Running" : "Stopped"}
                  </button>
                </td>
                <td class="px-4 py-2.5 font-semibold text-primary">{t.connectionName}</td>
                <td class="px-4 py-2.5">
                  <span class="tag uppercase">{t.type}</span>
                </td>
                <td class="px-4 py-2.5">
                  <div class="flex items-center gap-2 font-mono text-xs">
                    <span class="text-primary">127.0.0.1:{t.localPort}</span>
                    <ArrowRight size={13} class={t.active ? "text-accent" : "text-muted"} />
                    <span class="text-muted">{targetLabel(t)}</span>
                  </div>
                </td>
                <td class="px-4 py-2.5">
                  <div class="flex justify-end gap-1.5">
                    <button
                      type="button"
                      class="btn-icon"
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
                      class="btn-icon hover:bg-error/15 hover:text-error"
                      onclick={() => deleteTunnel(t.id)}
                      title="Delete tunnel rule"
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="empty-state empty-state-dashed">
        <div class="empty-state-icon">
          <Network size={22} />
        </div>
        <h3 class="empty-state-title">No tunnel rules</h3>
        <p class="empty-state-desc">Create a local, remote, or SOCKS5 forwarding rule.</p>
        <button type="button" class="btn btn-primary empty-state-action" onclick={() => (showAddModal = true)}>
          <Plus size={14} />
          <span>Create tunnel</span>
        </button>
      </div>
    {/if}
  </div>
</div>

{#if showAddModal}
  <ModalShell
    open={true}
    title="Create SSH Tunnel Rule"
    onClose={() => (showAddModal = false)}
    width="md"
  >
    <div class="modal-header">
      <h3 class="modal-title">Create SSH Tunnel</h3>
      <button type="button" class="modal-close" onclick={() => (showAddModal = false)} title="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-4">
      <div class="field">
        <label for="tunnel-conn-select" class="field-label">SSH host</label>
        <CustomSelect
          id="tunnel-conn-select"
          options={connections.map((c) => ({ value: c.name, label: `${c.name} (${c.user}@${c.host})` }))}
          value={newConnName}
          onChange={(val) => (newConnName = val)}
        />
      </div>

      <div class="field">
        <span class="field-label">Forwarding type</span>
        <div class="segmented-control">
          <button
            type="button"
            class="segmented-button flex-1 px-3 py-1.5 text-xs {newType === 'local' ? 'segmented-button-active' : ''}"
            onclick={() => (newType = "local")}
          >
            Local (-L)
          </button>
          <button
            type="button"
            class="segmented-button flex-1 px-3 py-1.5 text-xs {newType === 'socks5' ? 'segmented-button-active' : ''}"
            onclick={() => (newType = "socks5")}
          >
            SOCKS5 (-D)
          </button>
          <button
            type="button"
            class="segmented-button flex-1 px-3 py-1.5 text-xs {newType === 'remote' ? 'segmented-button-active' : ''}"
            onclick={() => (newType = "remote")}
          >
            Remote (-R)
          </button>
        </div>
      </div>

      <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div class="field">
          <label for="local-port-input" class="field-label">Local port</label>
          <input id="local-port-input" type="number" bind:value={newLocalPort} class="input font-mono text-xs" placeholder="8080" />
        </div>

        {#if newType !== "socks5"}
          <div class="field">
            <label for="remote-port-input" class="field-label">Remote port</label>
            <input id="remote-port-input" type="number" bind:value={newRemotePort} class="input font-mono text-xs" placeholder="80" />
          </div>
        {/if}
      </div>

      {#if newType !== "socks5"}
        <div class="field">
          <label for="remote-host-input" class="field-label">Remote host</label>
          <input id="remote-host-input" type="text" bind:value={newRemoteHost} class="input font-mono text-xs" placeholder="localhost or db.internal" />
        </div>
      {/if}

      <div class="panel flex items-center justify-between gap-3 px-3 py-2.5 font-mono text-xs text-muted">
        <span>127.0.0.1:{newLocalPort}</span>
        <ArrowRight size={13} class="text-accent" />
        <span>{newType === "socks5" ? "Dynamic SOCKS5" : `${newRemoteHost}:${newRemotePort}`}</span>
      </div>
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={() => (showAddModal = false)}>Cancel</button>
      <button type="button" class="btn btn-primary" onclick={handleAddTunnel}>
        <Power size={13} />
        <span>Add Tunnel</span>
      </button>
    </div>
  </ModalShell>
{/if}
