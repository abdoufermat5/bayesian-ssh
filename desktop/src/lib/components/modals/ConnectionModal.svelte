<script lang="ts">
  import { X, Key } from "lucide-svelte";

  interface Props {
    isEditing: boolean;
    modalName: string;
    modalHost: string;
    modalUser: string;
    modalPort: number;
    modalUseKerberos: boolean;
    modalBastion: string;
    modalBastionUser: string;
    modalKeyPath: string;
    modalTagsString: string;
    onClose: () => void;
    onSave: () => void;
    onBrowseKey: () => void;
  }

  let {
    isEditing,
    modalName = $bindable(),
    modalHost = $bindable(),
    modalUser = $bindable(),
    modalPort = $bindable(),
    modalUseKerberos = $bindable(),
    modalBastion = $bindable(),
    modalBastionUser = $bindable(),
    modalKeyPath = $bindable(),
    modalTagsString = $bindable(),
    onClose,
    onSave,
    onBrowseKey,
  }: Props = $props();
</script>

<div class="modal-backdrop" onclick={onClose}>
  <div class="modal-dialog" onclick={(e) => e.stopPropagation()}>
    <div class="modal-header">
      <h2>{isEditing ? "Edit Connection" : "New SSH Connection"}</h2>
      <button class="close-btn" onclick={onClose}><X size={18} /></button>
    </div>

    <div class="modal-body">
      <div class="form-row">
        <div class="form-group flex-2">
          <label for="c-name">Connection Name</label>
          <input id="c-name" type="text" placeholder="e.g. Server Production" bind:value={modalName} class="cyber-input" />
        </div>
        <div class="form-group flex-3">
          <label for="c-host">Hostname / IP Address</label>
          <input id="c-host" type="text" placeholder="e.g. 192.168.1.50" bind:value={modalHost} class="cyber-input" />
        </div>
      </div>

      <div class="form-row">
        <div class="form-group">
          <label for="c-user">SSH Username</label>
          <input id="c-user" type="text" placeholder="root" bind:value={modalUser} class="cyber-input" />
        </div>
        <div class="form-group">
          <label for="c-port">Port</label>
          <input id="c-port" type="number" bind:value={modalPort} class="cyber-input" />
        </div>
      </div>

      <div class="form-group">
        <label for="c-key">Identity File (SSH Private Key)</label>
        <div class="input-with-action">
          <input id="c-key" type="text" placeholder="Path to SSH key file" bind:value={modalKeyPath} class="cyber-input" />
          <button class="browse-btn" onclick={onBrowseKey}><Key size={14} /> Browse</button>
        </div>
      </div>

      <div class="checkbox-row" onclick={() => (modalUseKerberos = !modalUseKerberos)} role="presentation">
        <input id="c-krb" type="checkbox" bind:checked={modalUseKerberos} onclick={(e) => e.stopPropagation()} />
        <label for="c-krb">Enable Kerberos / GSSAPI Authentication</label>
      </div>

      <div class="form-section-divider">
        <span>BASTION JUMP HOST (OPTIONAL)</span>
      </div>

      <div class="form-row">
        <div class="form-group">
          <label for="c-bastion">Bastion Address</label>
          <input id="c-bastion" type="text" placeholder="bastion.internal" bind:value={modalBastion} class="cyber-input" />
        </div>
        <div class="form-group">
          <label for="c-bastion-user">Bastion Username</label>
          <input id="c-bastion-user" type="text" placeholder="jumpuser" bind:value={modalBastionUser} class="cyber-input" />
        </div>
      </div>

      <div class="form-group">
        <label for="c-tags">Tags (Separated by commas)</label>
        <input id="c-tags" type="text" placeholder="e.g. backend, aws, production" bind:value={modalTagsString} class="cyber-input" />
      </div>
    </div>

    <div class="modal-footer">
      <button class="cancel-btn" onclick={onClose}>Cancel</button>
      <button class="save-btn" onclick={onSave}>Save Server</button>
    </div>
  </div>
</div>
