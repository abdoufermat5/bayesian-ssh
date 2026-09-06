<script lang="ts">
  import { X, Trash2 } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
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

<ModalShell
  open={true}
  title="Manage Profiles"
  onClose={onClose}
  width="sm"
>
  <div class="modal-header">
      <h2 class="modal-title">Manage Profiles</h2>
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
      <div class="field">
        <label for="e-name" class="field-label">New Profile Name</label>
        <input
          id="e-name"
          type="text"
          placeholder="e.g. Dev-Local"
          bind:value={newEnvName}
          class="input"
        />
      </div>

      <div class="mt-2 border-t border-border pt-4">
        <span class="settings-section-title mb-2 block">ACTIVE PROFILES</span>
        <div class="flex flex-col gap-1.5">
          {#each environments as env}
            <div class="flex items-center justify-between rounded-md border border-border bg-panel px-3 py-2 text-sm">
              <span class="text-secondary {env.is_active ? 'text-accent font-semibold' : ''}">{env.name}</span>
              {#if env.name !== "default" && !env.is_active}
                <button
                  type="button"
                  class="btn-icon hover:text-danger"
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

    <div class="modal-footer">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={onClose}
      >
        Close
      </button>
      <button
        type="button"
        class="btn btn-primary"
        onclick={onCreate}
      >
        Add Profile
      </button>
    </div>
</ModalShell>
