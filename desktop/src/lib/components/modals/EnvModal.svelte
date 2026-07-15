<script lang="ts">
  import { X, Trash2 } from "lucide-svelte";
  import type { EnvInfo } from "$lib/types";

  interface Props {
    environments: EnvInfo[];
    newEnvName: string;
    onClose: () => void;
    onCreate: () => void;
    onDelete: (name: string) => void;
  }

  let { environments, newEnvName = $bindable(), onClose, onCreate, onDelete }: Props = $props();
</script>

<div
  class="fixed inset-0 bg-black/75 backdrop-blur-sm flex items-center justify-center z-[100]"
  onclick={onClose}
  role="presentation"
>
  <div
    class="bg-surface border border-border rounded-2xl w-[400px] shadow-xl flex flex-col animate-[modal-enter_0.25s_cubic-bezier(0.16,1,0.3,1)_forwards]"
    onclick={(e) => e.stopPropagation()}
    role="presentation"
  >
    <div class="flex justify-between items-center px-6 py-5 border-b border-border">
      <h2 class="text-base font-semibold tracking-tight m-0 text-primary">Manage Profiles</h2>
      <button
        class="bg-transparent border-none text-muted cursor-pointer flex p-1 rounded-md transition-all duration-100 hover:text-primary hover:bg-white/5"
        onclick={onClose}
      >
        <X size={18} />
      </button>
    </div>

    <div class="px-6 py-5 flex flex-col gap-4 max-h-[60vh] overflow-y-auto">
      <div class="flex flex-col gap-1.5">
        <label for="e-name" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">New Profile Name</label>
        <input
          id="e-name"
          type="text"
          placeholder="e.g. Dev-Local"
          bind:value={newEnvName}
          class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
        />
      </div>

      <div class="mt-2 border-t border-border pt-4">
        <span class="text-[10px] font-bold tracking-wider text-secondary uppercase block mb-2 pl-0.5">ACTIVE PROFILES</span>
        <div class="flex flex-col gap-1.5">
          {#each environments as env}
            <div class="flex justify-between items-center bg-white/[0.02] border border-border p-2 px-3 rounded-lg text-[13px]">
              <span class="text-secondary {env.is_active ? 'text-accent font-semibold' : ''}">{env.name}</span>
              {#if env.name !== "default" && !env.is_active}
                <button
                  class="bg-transparent border-none text-muted cursor-pointer p-1.5 flex rounded-md transition-colors hover:text-danger hover:bg-white/5"
                  onclick={() => onDelete(env.name)}
                  title="Delete Profile"
                >
                  <Trash2 size={12} />
                </button>
              {/if}
            </div>
          {/each}
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
      <button
        class="py-2 px-4 rounded-lg text-[13px] font-semibold cursor-pointer bg-accent border-none text-white transition-colors duration-100 hover:bg-accent-hover"
        onclick={onCreate}
      >
        Add Profile
      </button>
    </div>
  </div>
</div>
