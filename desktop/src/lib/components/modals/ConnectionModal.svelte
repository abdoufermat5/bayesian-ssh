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
      <h2 class="text-base font-semibold tracking-tight m-0 text-primary">
        {isEditing ? "Edit Connection" : "New SSH Connection"}
      </h2>
      <button
        class="bg-transparent border-none text-muted cursor-pointer flex p-1 rounded-md transition-all duration-100 hover:text-primary hover:bg-white/5"
        onclick={onClose}
      >
        <X size={18} />
      </button>
    </div>

    <div class="px-6 py-5 flex flex-col gap-4 max-h-[60vh] overflow-y-auto">
      <div class="flex gap-3">
        <div class="flex flex-col gap-1.5 flex-[2]">
          <label for="c-name" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Connection Name</label>
          <input
            id="c-name"
            type="text"
            placeholder="e.g. Server Production"
            bind:value={modalName}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
          />
        </div>
        <div class="flex flex-col gap-1.5 flex-[3]">
          <label for="c-host" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Hostname / IP Address</label>
          <input
            id="c-host"
            type="text"
            placeholder="e.g. 192.168.1.50"
            bind:value={modalHost}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
          />
        </div>
      </div>

      <div class="flex gap-3">
        <div class="flex flex-col gap-1.5 flex-1">
          <label for="c-user" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">SSH Username</label>
          <input
            id="c-user"
            type="text"
            placeholder="root"
            bind:value={modalUser}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
          />
        </div>
        <div class="flex flex-col gap-1.5 flex-1">
          <label for="c-port" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Port</label>
          <input
            id="c-port"
            type="number"
            bind:value={modalPort}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
          />
        </div>
      </div>

      <div class="flex flex-col gap-1.5">
        <label for="c-key" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Identity File (SSH Private Key)</label>
        <div class="flex gap-2">
          <input
            id="c-key"
            type="text"
            placeholder="Path to SSH key file"
            bind:value={modalKeyPath}
            class="flex-1 bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
          />
          <button
            class="bg-white/[0.04] border border-border text-secondary px-3.5 rounded-lg cursor-pointer font-semibold flex items-center gap-1.5 text-xs whitespace-nowrap transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.06]"
            onclick={onBrowseKey}
          >
            <Key size={14} /> Browse
          </button>
        </div>
      </div>

      <div
        class="flex items-center gap-2 text-[13px] cursor-pointer select-none text-secondary py-1"
        onclick={() => (modalUseKerberos = !modalUseKerberos)}
        role="presentation"
      >
        <input
          id="c-krb"
          type="checkbox"
          bind:checked={modalUseKerberos}
          onclick={(e) => e.stopPropagation()}
          class="cursor-pointer accent-accent w-[16px] h-[16px]"
        />
        <label for="c-krb" class="cursor-pointer">Enable Kerberos / GSSAPI Authentication</label>
      </div>

      <div class="text-[10px] font-bold tracking-widest text-muted uppercase flex items-center gap-2.5 my-2">
        <span>BASTION JUMP HOST (OPTIONAL)</span>
        <span class="flex-1 h-px bg-border"></span>
      </div>

      <div class="flex gap-3">
        <div class="flex flex-col gap-1.5 flex-1">
          <label for="c-bastion" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Bastion Address</label>
          <input
            id="c-bastion"
            type="text"
            placeholder="bastion.internal"
            bind:value={modalBastion}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
          />
        </div>
        <div class="flex flex-col gap-1.5 flex-1">
          <label for="c-bastion-user" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Bastion Username</label>
          <input
            id="c-bastion-user"
            type="text"
            placeholder="jumpuser"
            bind:value={modalBastionUser}
            class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
          />
        </div>
      </div>

      <div class="flex flex-col gap-1.5">
        <label for="c-tags" class="text-[11px] font-semibold text-muted uppercase tracking-wider pl-0.5">Tags (Separated by commas)</label>
        <input
          id="c-tags"
          type="text"
          placeholder="e.g. backend, aws, production"
          bind:value={modalTagsString}
          class="bg-surface-input border border-border text-primary py-2 px-3 rounded-lg outline-none text-[13px] transition-all duration-100 hover:border-border-hover focus:border-border-focus focus:shadow-[0_0_0_3px_rgba(59,130,246,0.12)]"
        />
      </div>
    </div>

    <div class="flex justify-end gap-2 px-6 py-4 border-t border-border">
      <button
        class="py-2 px-4 rounded-lg text-[13px] font-semibold cursor-pointer bg-transparent border border-border text-secondary transition-all duration-100 hover:border-border-hover hover:text-primary hover:bg-white/[0.03]"
        onclick={onClose}
      >
        Cancel
      </button>
      <button
        class="py-2 px-4 rounded-lg text-[13px] font-semibold cursor-pointer bg-accent border-none text-white transition-colors duration-100 hover:bg-accent-hover"
        onclick={onSave}
      >
        Save Server
      </button>
    </div>
  </div>
</div>
