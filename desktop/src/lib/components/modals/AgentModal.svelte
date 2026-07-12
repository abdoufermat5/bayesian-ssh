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

<div class="modal-backdrop" onclick={onClose}>
  <div class="modal-dialog" onclick={(e) => e.stopPropagation()}>
    <div class="modal-header">
      <h2>SSH Agent Manager</h2>
      <button class="close-btn" onclick={onClose}><X size={18} /></button>
    </div>

    <div class="modal-body">
      <div
        class="agent-info-box"
        style="background: var(--bg-card); padding: 12px; border: 1px solid var(--border-color); border-radius: 6px; margin-bottom: 16px;"
      >
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
          <span style="font-weight: 500; font-size: 13px; color: var(--text-color);">AGENT STATUS</span>
          <span style="font-size: 11px; font-weight: 600; color: var(--accent-cyan);">ACTIVE</span>
        </div>
        {#if agentSocket}
          <div style="font-size: 11px; color: var(--text-muted); word-break: break-all; font-family: monospace;">
            Socket: {agentSocket}
          </div>
        {/if}
      </div>

      <div class="agent-keys-section">
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
          <span class="section-title" style="margin: 0;">LOADED KEYS ({agentKeys.length})</span>
          <button
            class="cyber-btn mini"
            onclick={onAddKey}
            style="padding: 4px 8px; font-size: 11px; display: flex; align-items: center; gap: 4px;"
          >
            <Plus size={12} /> Add Key File
          </button>
        </div>

        <div
          class="keys-list-container"
          style="max-height: 200px; overflow-y: auto; border: 1px solid var(--border-color); border-radius: 6px; background: var(--bg-card);"
        >
          {#if agentKeys.length === 0}
            <div style="padding: 24px; text-align: center; color: var(--text-muted); font-size: 12px;">
              No keys currently loaded in the SSH Agent.
            </div>
          {:else}
            {#each agentKeys as key}
              <div
                class="key-item"
                style="padding: 10px 12px; border-bottom: 1px solid var(--border-color); font-family: monospace; font-size: 11px; color: var(--text-color); display: flex; justify-content: space-between; align-items: center;"
              >
                <span style="word-break: break-all; margin-right: 8px;">{key}</span>
              </div>
            {/each}
          {/if}
        </div>
      </div>
    </div>

    <div class="modal-footer">
      <button class="cancel-btn" onclick={onClose}>Close</button>
    </div>
  </div>
</div>
