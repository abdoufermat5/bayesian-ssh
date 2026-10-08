<script lang="ts">
  import { ArrowRight, Check, Copy, Network, Pause, Pencil, Play, Plus, Trash2, X } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";

  type TunnelType = "local" | "remote" | "socks5";

  export interface TunnelRule {
    id: string;
    connectionName: string;
    type: TunnelType;
    localPort: number;
    remoteHost?: string;
    remotePort?: number;
    active: boolean;
  }

  interface Props {
    connections: Connection[];
  }

  let { connections }: Props = $props();

  const defaultHostName = $derived(connections.length > 0 ? connections[0].name : "");

  // Tunnel rules live in this view only: there is no backend IPC for them yet.
  let tunnels = $state<TunnelRule[]>([]);

  let showAddModal = $state(false);
  let editingId = $state<string | null>(null);
  let newConnName = $state("");
  let newType = $state<TunnelType>("local");
  let newLocalPort = $state(8080);
  let newRemoteHost = $state("localhost");
  let newRemotePort = $state(80);
  let copiedId = $state<string | null>(null);

  const TYPE_LABEL: Record<TunnelType, string> = { local: "Local", remote: "Remote", socks5: "SOCKS5" };
  const TYPE_FLAG: Record<TunnelType, string> = { local: "-L", remote: "-R", socks5: "-D" };
  const TYPES: TunnelType[] = ["local", "remote", "socks5"];

  const presets = [
    { label: "Web", type: "local" as const, local: 8080, remoteHost: "localhost", remotePort: 80 },
    { label: "Postgres", type: "local" as const, local: 5432, remoteHost: "localhost", remotePort: 5432 },
    { label: "Redis", type: "local" as const, local: 6379, remoteHost: "localhost", remotePort: 6379 },
    { label: "SOCKS5", type: "socks5" as const, local: 1080 },
  ];

  const hostOptions = $derived(
    connections.map((c) => ({ value: c.name, label: `${c.name} (${c.user}@${c.host})` })),
  );

  const activeCount = $derived(tunnels.reduce((n, t) => n + (t.active ? 1 : 0), 0));

  const validPort = (p: unknown) => Number.isInteger(Number(p)) && Number(p) >= 1 && Number(p) <= 65535;
  const localPortError = $derived(validPort(newLocalPort) ? null : "Enter a port between 1 and 65535.");
  const remotePortError = $derived(
    newType === "socks5" || validPort(newRemotePort) ? null : "Enter a port between 1 and 65535.",
  );
  const remoteHostError = $derived(
    newType === "socks5" || newRemoteHost.trim() ? null : "Enter a host name or address.",
  );
  const formHost = $derived(newConnName || defaultHostName);
  const formValid = $derived(!!formHost && !localPortError && !remotePortError && !remoteHostError);

  const sshPreview = $derived.by(() => {
    const target = connections.find((c) => c.name === formHost);
    const dest = target ? `${target.user}@${target.host}` : formHost || "host";
    if (newType === "socks5") return `ssh -N -D ${newLocalPort} ${dest}`;
    if (newType === "remote") return `ssh -N -R ${newRemoteHost.trim() || "localhost"}:${newRemotePort}:localhost:${newLocalPort} ${dest}`;
    return `ssh -N -L ${newLocalPort}:${newRemoteHost.trim() || "localhost"}:${newRemotePort} ${dest}`;
  });

  function toggleTunnel(id: string) {
    const t = tunnels.find((x) => x.id === id);
    if (!t) return;
    t.active = !t.active;
    notify(
      t.active ? `Enabled ${TYPE_LABEL[t.type]} tunnel on port ${t.localPort}` : `Disabled tunnel on port ${t.localPort}`,
      t.active ? "success" : "info",
    );
  }

  function deleteTunnel(id: string) {
    tunnels = tunnels.filter((t) => t.id !== id);
    notify("Tunnel rule deleted", "info");
  }

  function openCreate(p?: (typeof presets)[number]) {
    editingId = null;
    newConnName = defaultHostName;
    if (p) {
      newType = p.type;
      newLocalPort = p.local;
      if (p.remoteHost) newRemoteHost = p.remoteHost;
      if (p.remotePort) newRemotePort = p.remotePort;
    }
    showAddModal = true;
  }

  function openEdit(t: TunnelRule) {
    editingId = t.id;
    newConnName = t.connectionName;
    newType = t.type;
    newLocalPort = t.localPort;
    newRemoteHost = t.remoteHost ?? "localhost";
    newRemotePort = t.remotePort ?? 80;
    showAddModal = true;
  }

  function closeModal() {
    showAddModal = false;
    editingId = null;
  }

  function handleSaveTunnel() {
    if (!formValid) return;
    const fields = {
      connectionName: formHost,
      type: newType,
      localPort: Number(newLocalPort),
      remoteHost: newType === "socks5" ? undefined : newRemoteHost.trim(),
      remotePort: newType === "socks5" ? undefined : Number(newRemotePort),
    };
    if (editingId) {
      const existing = tunnels.find((t) => t.id === editingId);
      if (existing) Object.assign(existing, fields);
      notify(`Updated tunnel on port ${fields.localPort}`, "success");
    } else {
      tunnels.push({ id: Date.now().toString(), active: true, ...fields });
      notify(`Created ${TYPE_LABEL[fields.type]} tunnel on port ${fields.localPort}`, "success");
    }
    closeModal();
  }

  function copyConnectionString(t: TunnelRule) {
    const str = t.type === "socks5" ? `socks5://127.0.0.1:${t.localPort}` : `http://localhost:${t.localPort}`;
    copyTextWithFallback(str);
    copiedId = t.id;
    notify(`Copied connection address: ${str}`, "success");
    setTimeout(() => {
      if (copiedId === t.id) copiedId = null;
    }, 2000);
  }

  /** [listen side, forward side] of a rule, in traffic direction. */
  function routeOf(t: { type: TunnelType; localPort: number; remoteHost?: string; remotePort?: number }) {
    if (t.type === "socks5") return [`127.0.0.1:${t.localPort}`, "dynamic (SOCKS5)"];
    if (t.type === "remote") return [`${t.remoteHost}:${t.remotePort}`, `127.0.0.1:${t.localPort}`];
    return [`127.0.0.1:${t.localPort}`, `${t.remoteHost}:${t.remotePort}`];
  }

  const formRoute = $derived(
    routeOf({ type: newType, localPort: newLocalPort, remoteHost: newRemoteHost.trim() || "…", remotePort: newRemotePort }),
  );
</script>

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Tunnels</h1>
      {#if tunnels.length > 0}
        <span class="text-sm tabular-nums text-muted">{activeCount} of {tunnels.length} enabled</span>
      {/if}
    </div>
    <div class="view-actions">
      <button type="button" class="btn btn-primary" onclick={() => openCreate()} disabled={connections.length === 0}>
        <Plus size={15} />
        New tunnel
      </button>
    </div>
  </header>

  <div class="view-toolbar">
    <span class="mr-1 text-xs text-muted">Quick add</span>
    <div class="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto scrollbar-none" role="group" aria-label="Tunnel presets">
      {#each presets as p (p.label)}
        <button
          type="button"
          class="chip"
          onclick={() => openCreate(p)}
          disabled={connections.length === 0}
          title={p.type === "socks5" ? `SOCKS5 proxy on port ${p.local}` : `Forward port ${p.local} to ${p.remoteHost}:${p.remotePort}`}
        >
          <Plus size={12} />
          {p.label}
          <span class="font-mono text-2xs text-muted">{p.local}</span>
        </button>
      {/each}
    </div>
  </div>

  <div class="view-body">
    {#if tunnels.length === 0}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><Network size={18} /></div>
        <div class="empty-state-title">No tunnels yet</div>
        {#if connections.length === 0}
          <p class="empty-state-desc">Add a host first; tunnels forward ports through one of your SSH hosts.</p>
        {:else}
          <p class="empty-state-desc">
            Add a local, remote or SOCKS5 port-forwarding rule for one of your hosts, or start from a preset above.
          </p>
          <div class="empty-state-action">
            <button type="button" class="btn btn-secondary" onclick={() => openCreate()}>
              <Plus size={14} />
              New tunnel
            </button>
          </div>
        {/if}
      </div>
    {:else}
      <div class="table-wrap">
        <table class="data-table table-fixed">
          <thead>
            <tr>
              <th class="w-[120px]">Status</th>
              <th class="w-[24%]">Host</th>
              <th class="w-[96px]">Type</th>
              <th>Route</th>
              <th class="w-[132px]"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {#each tunnels as t (t.id)}
              {@const [from, to] = routeOf(t)}
              <tr class="cursor-default" ondblclick={() => openEdit(t)}>
                <td>
                  <button
                    type="button"
                    role="switch"
                    aria-checked={t.active}
                    aria-label={t.active ? "Disable tunnel" : "Enable tunnel"}
                    class="inline-flex cursor-pointer items-center gap-2 text-sm {t.active ? 'text-primary' : 'text-muted'}"
                    onclick={() => toggleTunnel(t.id)}
                  >
                    <span class="status-dot {t.active ? 'status-dot-success' : 'status-dot-offline'}"></span>
                    {t.active ? "Enabled" : "Disabled"}
                  </button>
                </td>
                <td><span class="block truncate font-medium text-primary" title={t.connectionName}>{t.connectionName}</span></td>
                <td>
                  <span class="badge badge-neutral" title="ssh {TYPE_FLAG[t.type]}">{TYPE_LABEL[t.type]}</span>
                </td>
                <td>
                  <div class="flex min-w-0 items-center gap-2 font-mono text-xs">
                    <span class="truncate text-primary">{from}</span>
                    <ArrowRight size={14} class="shrink-0 text-faint" />
                    <span class="truncate text-muted">{to}</span>
                  </div>
                </td>
                <td>
                  <div class="row-actions">
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      onclick={() => toggleTunnel(t.id)}
                      aria-label={t.active ? "Disable tunnel" : "Enable tunnel"}
                      title={t.active ? "Disable" : "Enable"}
                    >
                      {#if t.active}<Pause size={14} />{:else}<Play size={14} />{/if}
                    </button>
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      onclick={() => copyConnectionString(t)}
                      aria-label="Copy local address"
                      title="Copy local address"
                    >
                      {#if copiedId === t.id}
                        <Check size={14} class="text-success" />
                      {:else}
                        <Copy size={14} />
                      {/if}
                    </button>
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      onclick={() => openEdit(t)}
                      aria-label="Edit tunnel"
                      title="Edit"
                    >
                      <Pencil size={14} />
                    </button>
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm btn-icon-danger"
                      onclick={() => deleteTunnel(t.id)}
                      aria-label="Delete tunnel"
                      title="Delete"
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>

{#if showAddModal}
  <ModalShell open={true} title={editingId ? "Edit tunnel" : "New tunnel"} onClose={closeModal} width="md">
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">{editingId ? "Edit tunnel" : "New tunnel"}</h2>
        <p class="modal-subtitle">Forward a port through one of your SSH hosts.</p>
      </div>
      <button type="button" class="modal-close" onclick={closeModal} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <form
      class="contents"
      onsubmit={(e) => {
        e.preventDefault();
        handleSaveTunnel();
      }}
    >
      <div class="modal-body flex flex-col gap-4">
        <div class="field">
          <label for="tunnel-conn-select" class="field-label">Host</label>
          <CustomSelect
            id="tunnel-conn-select"
            options={hostOptions}
            value={formHost}
            placeholder="No hosts"
            onChange={(val) => (newConnName = val)}
          />
        </div>

        <div class="field">
          <span class="field-label" id="tunnel-type-label">Type</span>
          <div class="segmented w-full" role="group" aria-labelledby="tunnel-type-label">
            {#each TYPES as ty (ty)}
              <button
                type="button"
                class="segmented-item flex-1 {newType === ty ? 'segmented-item-active' : ''}"
                onclick={() => (newType = ty)}
                aria-pressed={newType === ty}
              >
                {TYPE_LABEL[ty]}
                <span class="font-mono text-2xs text-muted">{TYPE_FLAG[ty]}</span>
              </button>
            {/each}
          </div>
          <span class="field-hint">
            {#if newType === "local"}
              Open a port on this machine that reaches a service behind the host.
            {:else if newType === "remote"}
              Open a port on the host that reaches a service on this machine.
            {:else}
              Run a SOCKS5 proxy on this machine that sends traffic through the host.
            {/if}
          </span>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div class="field">
            <label for="local-port-input" class="field-label">Local port</label>
            <input
              id="local-port-input"
              type="number"
              min="1"
              max="65535"
              bind:value={newLocalPort}
              class="input input-mono {localPortError ? 'input-invalid' : ''}"
              placeholder="8080"
            />
            {#if localPortError}<span class="field-error">{localPortError}</span>{/if}
          </div>
          {#if newType !== "socks5"}
            <div class="field">
              <label for="remote-port-input" class="field-label">Remote port</label>
              <input
                id="remote-port-input"
                type="number"
                min="1"
                max="65535"
                bind:value={newRemotePort}
                class="input input-mono {remotePortError ? 'input-invalid' : ''}"
                placeholder="80"
              />
              {#if remotePortError}<span class="field-error">{remotePortError}</span>{/if}
            </div>
          {/if}
        </div>

        {#if newType !== "socks5"}
          <div class="field">
            <label for="remote-host-input" class="field-label">
              {newType === "remote" ? "Bind address on host" : "Destination host"}
            </label>
            <input
              id="remote-host-input"
              type="text"
              bind:value={newRemoteHost}
              class="input input-mono {remoteHostError ? 'input-invalid' : ''}"
              placeholder="localhost or db.internal"
              spellcheck="false"
              autocomplete="off"
            />
            {#if remoteHostError}<span class="field-error">{remoteHostError}</span>{/if}
          </div>
        {/if}

        <div class="field">
          <span class="field-label">Preview</span>
          <div class="code-block flex flex-col gap-1">
            <span class="flex items-center gap-2">
              <span class="text-primary">{formRoute[0]}</span>
              <ArrowRight size={14} class="shrink-0 text-faint" />
              <span>{formRoute[1]}</span>
            </span>
            <span class="select-text text-muted">{sshPreview}</span>
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <button type="button" class="btn btn-secondary" onclick={closeModal}>Cancel</button>
        <button type="submit" class="btn btn-primary" disabled={!formValid}>
          {editingId ? "Save changes" : "Create tunnel"}
        </button>
      </div>
    </form>
  </ModalShell>
{/if}
