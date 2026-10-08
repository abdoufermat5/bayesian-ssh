<script lang="ts">
  import { X, Plus } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    agentSocket: string | null;
    agentKeys: string[];
    onClose: () => void;
    onAddKey: () => void;
  }

  let { agentSocket, agentKeys, onClose, onAddKey }: Props = $props();

  /** ssh-add -l lines look like "256 SHA256:abc comment (ED25519)" or "ED25519 SHA256:abc comment". */
  function splitKey(line: string): { type: string | null; fingerprint: string; comment: string } {
    const parts = line.trim().split(/\s+/);
    const fpIndex = parts.findIndex((p) => p.startsWith("SHA256:") || p.startsWith("MD5:"));
    if (fpIndex === -1) return { type: null, fingerprint: line, comment: "" };
    let rest = parts.slice(fpIndex + 1);
    let type: string | null = null;
    const tail = rest[rest.length - 1];
    if (tail && /^\(.+\)$/.test(tail)) {
      type = tail.slice(1, -1);
      rest = rest.slice(0, -1);
    } else if (fpIndex > 0 && !/^\d+$/.test(parts[0])) {
      type = parts[0];
    }
    return { type, fingerprint: parts[fpIndex], comment: rest.join(" ") };
  }

  const keys = $derived(agentKeys.map((line, i) => ({ id: `${i}:${line}`, ...splitKey(line) })));
</script>

<ModalShell open={true} title="SSH agent" {onClose} width="md">
  <div class="modal-header">
    <div class="min-w-0">
      <h2 class="modal-title">SSH agent</h2>
      <p class="modal-subtitle">Keys loaded here are offered to every host.</p>
    </div>
    <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
      <X size={16} />
    </button>
  </div>

  <div class="modal-body flex flex-col gap-4">
    <div class="flex items-center gap-2.5 rounded-md border border-border bg-panel px-3 py-2.5">
      <span class="status-dot status-dot-success" aria-hidden="true"></span>
      <span class="text-sm text-primary">Running</span>
      {#if agentSocket}
        <span class="ml-auto min-w-0 truncate font-mono text-xs text-muted" title={agentSocket}>{agentSocket}</span>
      {/if}
    </div>

    <div class="flex flex-col gap-2">
      <div class="flex items-center gap-2">
        <span class="section-label">Loaded keys</span>
        <span class="count">{agentKeys.length}</span>
      </div>
      {#if keys.length === 0}
        <div class="rounded-lg border border-border bg-panel px-4 py-6 text-center">
          <p class="m-0 text-sm text-primary">No keys loaded</p>
          <p class="m-0 mt-1 text-xs text-muted">Add a private key file to make it available to SSH sessions.</p>
        </div>
      {:else}
        <ul class="m-0 flex max-h-60 list-none flex-col divide-y divide-border-subtle overflow-y-auto rounded-lg border border-border bg-panel p-0">
          {#each keys as key (key.id)}
            <li class="flex min-w-0 items-center gap-2.5 px-3 py-2">
              {#if key.type}
                <span class="badge badge-neutral badge-mono">{key.type}</span>
              {/if}
              <span class="flex min-w-0 flex-1 flex-col">
                <span class="truncate font-mono text-xs text-secondary" title={key.fingerprint}>{key.fingerprint}</span>
                {#if key.comment}
                  <span class="truncate text-xs text-muted">{key.comment}</span>
                {/if}
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>

  <div class="modal-footer">
    <button type="button" class="btn btn-secondary" onclick={onClose}>Close</button>
    <button type="button" class="btn btn-primary" onclick={onAddKey}>
      <Plus size={14} />
      Add key
    </button>
  </div>
</ModalShell>
