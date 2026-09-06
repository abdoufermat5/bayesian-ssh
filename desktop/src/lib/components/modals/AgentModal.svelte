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
</script>

<ModalShell
  open={true}
  title="SSH Agent Manager"
  onClose={onClose}
  width="md"
>
  <div class="modal-header">
      <h2 class="modal-title">SSH Agent Manager</h2>
      <button
        type="button"
        class="modal-close"
        onclick={onClose}
        aria-label="Close"
      >
        <X size={18} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-4">
      <div class="settings-section">
        <div class="flex justify-between items-center">
          <span class="settings-section-title">Agent Status</span>
          <span class="text-xs font-semibold text-accent">ACTIVE</span>
        </div>
        {#if agentSocket}
          <div class="text-[11px] text-muted break-all font-mono">
            Socket: {agentSocket}
          </div>
        {/if}
      </div>

      <div class="flex flex-col">
        <div class="flex justify-between items-center mb-2">
          <span class="settings-section-title">LOADED KEYS ({agentKeys.length})</span>
          <button
            type="button"
            class="btn btn-primary btn-sm"
            onclick={onAddKey}
          >
            <Plus size={12} /> Add Key File
          </button>
        </div>

        <div class="max-h-[200px] overflow-y-auto rounded-md border border-border bg-surface-input divide-y divide-border">
          {#if agentKeys.length === 0}
            <div class="p-6 text-center text-muted text-xs">
              No keys currently loaded in the SSH Agent.
            </div>
          {:else}
            {#each agentKeys as key}
              <div class="p-2.5 px-3.5 font-mono text-[11px] text-secondary break-all">
                {key}
              </div>
            {/each}
          {/if}
        </div>
      </div>
    </div>

    <div class="modal-footer">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={onClose}
      >
        Close
      </button>
    </div>
</ModalShell>
