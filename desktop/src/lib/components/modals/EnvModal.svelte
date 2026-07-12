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

<div class="modal-backdrop" onclick={onClose}>
  <div class="modal-dialog mini" onclick={(e) => e.stopPropagation()}>
    <div class="modal-header">
      <h2>Manage Profiles</h2>
      <button class="close-btn" onclick={onClose}><X size={18} /></button>
    </div>

    <div class="modal-body">
      <div class="form-group">
        <label for="e-name">New Profile Name</label>
        <input id="e-name" type="text" placeholder="e.g. Dev-Local" bind:value={newEnvName} class="cyber-input" />
      </div>

      <div class="profile-manager-list">
        <span class="section-title">ACTIVE PROFILES</span>
        <div class="profile-rows">
          {#each environments as env}
            <div class="profile-row">
              <span class="profile-name" class:active={env.is_active}>{env.name}</span>
              {#if env.name !== "default" && !env.is_active}
                <button class="profile-delete-btn" onclick={() => onDelete(env.name)} title="Delete Profile">
                  <Trash2 size={12} />
                </button>
              {/if}
            </div>
          {/each}
        </div>
      </div>
    </div>

    <div class="modal-footer">
      <button class="cancel-btn" onclick={onClose}>Close</button>
      <button class="save-btn" onclick={onCreate}>Add Profile</button>
    </div>
  </div>
</div>
