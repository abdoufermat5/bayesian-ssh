<script lang="ts">
  import { X, FolderOpen, ChevronRight } from "lucide-svelte";
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

  const title = $derived(isEditing ? "Edit host" : "New host");

  // Advanced starts open when the host already uses a jump host.
  let showAdvanced = $state(Boolean(modalBastion?.trim() || modalBastionUser?.trim()));

  // Validation errors appear only after the first submit attempt, then live.
  let attempted = $state(false);
  const nameError = $derived(attempted && !modalName.trim() ? "Name is required" : null);
  const hostError = $derived(attempted && !modalHost.trim() ? "Host is required" : null);

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

  function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    attempted = true;
    if (!modalName.trim()) {
      document.getElementById("c-name")?.focus();
      return;
    }
    if (!modalHost.trim()) {
      document.getElementById("c-host")?.focus();
      return;
    }
    onSave();
  }
</script>

<ModalShell open={true} {title} {onClose} width="form">
  <form class="flex min-h-0 flex-1 flex-col" novalidate onsubmit={handleSubmit}>
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">{title}</h2>
        <p class="modal-subtitle">
          {isEditing ? "Changes apply to new sessions." : "Saved to the active profile."}
        </p>
      </div>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-4">
      <div class="field">
        <label for="c-name" class="field-label">Name</label>
        <input
          id="c-name"
          type="text"
          placeholder="prod-api-1"
          autocomplete="off"
          spellcheck="false"
          bind:value={modalName}
          class="input"
          class:input-invalid={nameError}
          aria-invalid={nameError ? "true" : undefined}
          aria-describedby={nameError ? "c-name-error" : undefined}
          data-autofocus
        />
        {#if nameError}
          <span id="c-name-error" class="field-error">{nameError}</span>
        {/if}
      </div>

      <div class="grid grid-cols-[1fr_96px] gap-3">
        <div class="field">
          <label for="c-host" class="field-label">Host</label>
          <input
            id="c-host"
            type="text"
            placeholder="10.0.0.12 or api.example.com"
            autocomplete="off"
            spellcheck="false"
            bind:value={modalHost}
            class="input input-mono"
            class:input-invalid={hostError}
            aria-invalid={hostError ? "true" : undefined}
            aria-describedby={hostError ? "c-host-error" : undefined}
          />
          {#if hostError}
            <span id="c-host-error" class="field-error">{hostError}</span>
          {/if}
        </div>
        <div class="field">
          <label for="c-port" class="field-label">Port</label>
          <input
            id="c-port"
            type="number"
            min="1"
            max="65535"
            placeholder="22"
            bind:value={modalPort}
            class="input input-mono tabular-nums"
          />
        </div>
      </div>

      <div class="field">
        <label for="c-user" class="field-label">User</label>
        <input
          id="c-user"
          type="text"
          placeholder="Default user"
          autocomplete="off"
          spellcheck="false"
          bind:value={modalUser}
          class="input input-mono"
        />
      </div>

      <div class="field">
        <label for="c-key" class="field-label">Identity file</label>
        <div class="flex gap-2">
          <input
            id="c-key"
            type="text"
            placeholder="~/.ssh/id_ed25519"
            autocomplete="off"
            spellcheck="false"
            bind:value={modalKeyPath}
            class="input input-mono flex-1"
          />
          <button type="button" class="btn btn-secondary" onclick={onBrowseKey}>
            <FolderOpen size={14} />
            Browse
          </button>
        </div>
        <span class="field-hint">Leave empty to use the SSH agent or your default keys.</span>
      </div>

      <label
        for="c-krb"
        class="flex cursor-pointer items-center justify-between gap-4 rounded-md border border-border px-3 py-2.5"
      >
        <span class="flex min-w-0 flex-col gap-0.5">
          <span class="text-sm text-primary">Kerberos (GSSAPI)</span>
          <span class="text-xs text-muted">Authenticate with your Kerberos ticket.</span>
        </span>
        <input id="c-krb" type="checkbox" class="switch" bind:checked={modalUseKerberos} />
      </label>

      <div class="field">
        <label for="c-tags" class="field-label">Tags</label>
        <input
          id="c-tags"
          type="text"
          placeholder="prod, aws, backend"
          autocomplete="off"
          spellcheck="false"
          bind:value={modalTagsString}
          class="input"
        />
        {#if parsedTags.length > 0}
          <div class="flex flex-wrap gap-1">
            {#each parsedTags as tag (tag)}
              <span class="tag gap-1 pr-0.5">
                {tag}
                <button
                  type="button"
                  class="inline-flex size-4 cursor-pointer items-center justify-center rounded-xs text-muted transition-colors hover:text-error"
                  onclick={() => removeTag(tag)}
                  aria-label={`Remove tag ${tag}`}
                >
                  <X size={10} />
                </button>
              </span>
            {/each}
          </div>
        {:else}
          <span class="field-hint">Separate tags with commas.</span>
        {/if}
      </div>

      <div class="rounded-md border border-border">
        <button
          type="button"
          class="flex h-9 w-full cursor-pointer items-center gap-1.5 rounded-md px-3 text-left text-sm text-secondary transition-colors hover:text-primary"
          aria-expanded={showAdvanced}
          aria-controls="c-advanced"
          onclick={() => (showAdvanced = !showAdvanced)}
        >
          <ChevronRight
            size={14}
            class="text-muted transition-transform duration-150 {showAdvanced ? 'rotate-90' : ''}"
          />
          Advanced
          {#if !showAdvanced && modalBastion.trim()}
            <span class="ml-auto truncate font-mono text-xs text-muted">via {modalBastion}</span>
          {/if}
        </button>
        {#if showAdvanced}
          <div id="c-advanced" class="grid grid-cols-[1fr_160px] gap-3 border-t border-border-subtle px-3 pb-3 pt-3">
            <div class="field">
              <label for="c-bastion" class="field-label">Jump host</label>
              <input
                id="c-bastion"
                type="text"
                placeholder="bastion.example.com"
                autocomplete="off"
                spellcheck="false"
                bind:value={modalBastion}
                class="input input-mono"
              />
            </div>
            <div class="field">
              <label for="c-bastion-user" class="field-label">Jump user</label>
              <input
                id="c-bastion-user"
                type="text"
                placeholder="Same as user"
                autocomplete="off"
                spellcheck="false"
                bind:value={modalBastionUser}
                class="input input-mono"
              />
            </div>
            <span class="field-hint col-span-2">Sessions connect to the jump host first, then to this host.</span>
          </div>
        {/if}
      </div>
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={onClose}>Cancel</button>
      <button type="submit" class="btn btn-primary">{isEditing ? "Save changes" : "Add host"}</button>
    </div>
  </form>
</ModalShell>
