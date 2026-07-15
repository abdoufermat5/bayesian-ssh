<script lang="ts">
  import { X, Plus } from "lucide-svelte";

  interface Props {
    agentSocket: string | null;
    agentKeys: string[];
    onClose: () => void;
    onAddKey: () => void;
  }

  let { agentSocket, agentKeys, onClose, onAddKey }: Props = $props();
</script>

<div
  class="fixed inset-0 bg-black/75 backdrop-blur-sm flex items-center justify-center z-[100]"
  onclick={onClose}
  role="presentation"
>
  <div
    class="bg-surface border border-border rounded-2xl w-[520px] shadow-xl flex flex-col animate-[modal-enter_0.25s_cubic-bezier(0.16,1,0.3,1)_forwards]"
    onclick={(e) => e.stopPropagation()}
    role="presentation"
  >
    <div class="flex justify-between items-center px-6 py-5 border-b border-border">
      <h2 class="text-base font-semibold tracking-tight m-0 text-primary">SSH Agent Manager</h2>
      <button
        class="bg-transparent border-none text-muted cursor-pointer flex p-1 rounded-md transition-all duration-100 hover:text-primary hover:bg-white/5"
        onclick={onClose}
      >
        <X size={18} />
      </button>
    </div>

    <div class="px-6 py-5 flex flex-col gap-4 max-h-[60vh] overflow-y-auto">
      <div class="bg-surface-raised border border-border rounded-xl p-4 flex flex-col gap-2">
        <div class="flex justify-between items-center">
          <span class="text-[10px] font-bold tracking-wider text-secondary uppercase">Agent Status</span>
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
          <span class="text-[10px] font-bold tracking-wider text-secondary uppercase">LOADED KEYS ({agentKeys.length})</span>
          <button
            class="bg-accent border-none text-white py-1.5 px-3 rounded-lg font-semibold cursor-pointer inline-flex items-center gap-1.5 text-xs transition-colors duration-150 hover:bg-accent-hover"
            onclick={onAddKey}
          >
            <Plus size={12} /> Add Key File
          </button>
        </div>

        <div class="max-h-[200px] overflow-y-auto border border-border rounded-lg bg-surface-input divide-y divide-border">
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

    <div class="flex justify-end gap-2 px-6 py-4 border-t border-border">
      <button
        class="py-2 px-4 rounded-lg text-[13px] font-semibold cursor-pointer bg-transparent border border-border text-secondary transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.03]"
        onclick={onClose}
      >
        Close
      </button>
    </div>
  </div>
</div>
