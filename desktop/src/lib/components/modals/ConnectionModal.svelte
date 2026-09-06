<script lang="ts">
  import { X, Key, Tag } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

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

  const parsedTags = $derived(
    modalTagsString
      ? modalTagsString
          .split(",")
          .map((t) => t.trim())
          .filter(Boolean)
      : [],
  );

  function removeTag(tagToRemove: string) {
    modalTagsString = parsedTags.filter((t) => t !== tagToRemove).join(", ");
  }
</script>

<ModalShell
  open={true}
  title={isEditing ? "Edit Connection" : "New SSH Connection"}
  onClose={onClose}
  panelClass="w-full max-w-xl"
>
  <div class="modal-header">
    <h2 class="modal-title">
      {isEditing ? "Edit Connection" : "New SSH Connection"}
    </h2>
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
    <!-- Connection Name -->
    <div class="field">
      <label for="c-name" class="field-label">Connection Name</label>
      <input
        id="c-name"
        type="text"
        placeholder="e.g. Production Cluster Gateway"
        bind:value={modalName}
        class="input"
      />
    </div>

    <!-- Host and Port -->
    <div class="flex gap-3">
      <div class="field flex-1">
        <label for="c-host" class="field-label">Hostname / IP Address</label>
        <input
          id="c-host"
          type="text"
          placeholder="e.g. 192.168.1.50 or ec2-host.aws.com"
          bind:value={modalHost}
          class="input font-mono text-xs"
        />
      </div>
      <div class="field w-28 shrink-0">
        <label for="c-port" class="field-label">Port</label>
        <input
          id="c-port"
          type="number"
          bind:value={modalPort}
          class="input font-mono text-xs"
          placeholder="22"
        />
      </div>
    </div>

    <!-- Username & Identity File -->
    <div class="flex gap-3">
      <div class="field w-40 shrink-0">
        <label for="c-user" class="field-label">SSH Username</label>
        <input
          id="c-user"
          type="text"
          placeholder="root"
          bind:value={modalUser}
          class="input font-mono text-xs"
        />
      </div>

      <div class="field flex-1">
        <label for="c-key" class="field-label">Identity File (Private Key)</label>
        <div class="flex gap-2">
          <input
            id="c-key"
            type="text"
            placeholder="~/.ssh/id_ed25519"
            bind:value={modalKeyPath}
            class="input flex-1 font-mono text-xs"
          />
          <button
            type="button"
            class="btn btn-secondary shrink-0"
            onclick={onBrowseKey}
            title="Browse SSH key file"
          >
            <Key size={13} />
            <span>Browse</span>
          </button>
        </div>
      </div>
    </div>

    <!-- Kerberos Authentication -->
    <div
      class="flex cursor-pointer items-center gap-2.5 rounded-lg border border-border/60 bg-surface-input/40 px-3 py-2 text-xs text-secondary select-none hover:bg-surface-hover/50 transition-colors"
      onclick={() => (modalUseKerberos = !modalUseKerberos)}
      role="presentation"
    >
      <input
        id="c-krb"
        type="checkbox"
        bind:checked={modalUseKerberos}
        onclick={(e) => e.stopPropagation()}
        class="cursor-pointer accent-accent w-4 h-4"
      />
      <label for="c-krb" class="cursor-pointer font-medium text-primary">Enable Kerberos / GSSAPI Authentication</label>
    </div>

    <!-- Bastion Jump Host -->
    <div class="settings-section-title my-1">
      <span>BASTION JUMP HOST (OPTIONAL)</span>
      <span class="flex-1 h-px bg-border"></span>
    </div>

    <div class="flex gap-3">
      <div class="field flex-1">
        <label for="c-bastion" class="field-label">Bastion Hostname</label>
        <input
          id="c-bastion"
          type="text"
          placeholder="bastion.internal.corp"
          bind:value={modalBastion}
          class="input font-mono text-xs"
        />
      </div>
      <div class="field w-40 shrink-0">
        <label for="c-bastion-user" class="field-label">Bastion User</label>
        <input
          id="c-bastion-user"
          type="text"
          placeholder="jumpuser"
          bind:value={modalBastionUser}
          class="input font-mono text-xs"
        />
      </div>
    </div>

    <!-- Tags -->
    <div class="field">
      <label for="c-tags" class="field-label flex items-center gap-1">
        <Tag size={12} class="text-muted" />
        <span>Tags (Separated by commas)</span>
      </label>
      <input
        id="c-tags"
        type="text"
        placeholder="e.g. backend, aws, production"
        bind:value={modalTagsString}
        class="input"
      />
      {#if parsedTags.length > 0}
        <div class="flex flex-wrap gap-1 mt-1.5">
          {#each parsedTags as tag}
            <span class="tag flex items-center gap-1">
              <span>#{tag}</span>
              <button
                type="button"
                class="text-muted hover:text-error cursor-pointer border-none bg-transparent p-0 inline-flex items-center"
                onclick={() => removeTag(tag)}
                title={`Remove #${tag}`}
                aria-label={`Remove #${tag}`}
              >
                <X size={10} />
              </button>
            </span>
          {/each}
        </div>
      {/if}
    </div>
  </div>

    <div class="modal-footer">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={onClose}
      >
        Cancel
      </button>
      <button
        type="button"
        class="btn btn-primary"
        onclick={onSave}
      >
        Save Server
      </button>
    </div>
</ModalShell>
