<script lang="ts">
  import { X, Trash2, Check } from "lucide-svelte";
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

  const trimmed = $derived(newEnvName.trim());
  const duplicate = $derived(trimmed !== "" && environments.some((env) => env.name === trimmed));

  function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    if (!trimmed || duplicate) return;
    onCreate();
  }
</script>

<ModalShell open={true} title="Profiles" {onClose} width="md">
  <form class="flex min-h-0 flex-1 flex-col" novalidate onsubmit={handleSubmit}>
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Profiles</h2>
        <p class="modal-subtitle">Each profile keeps its own hosts, history and settings.</p>
      </div>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body flex flex-col gap-4">
      <ul class="m-0 flex list-none flex-col divide-y divide-border-subtle overflow-hidden rounded-lg border border-border bg-panel p-0">
        {#each environments as env (env.name)}
          <li class="group flex h-10 items-center gap-2.5 px-3">
            <span class="flex size-4 shrink-0 items-center justify-center text-accent">
              {#if env.is_active}<Check size={14} />{/if}
            </span>
            <span class="min-w-0 flex-1 truncate text-sm {env.is_active ? 'text-primary' : 'text-secondary'}">
              {env.name}
            </span>
            {#if env.is_active}
              <span class="badge badge-accent">Active</span>
            {:else if env.name !== "default"}
              <button
                type="button"
                class="btn-icon btn-icon-sm btn-icon-danger"
                onclick={() => onDelete(env.name)}
                aria-label={`Delete profile ${env.name}`}
                title="Delete profile"
              >
                <Trash2 size={14} />
              </button>
            {/if}
          </li>
        {/each}
      </ul>

      <div class="field">
        <label for="e-name" class="field-label">New profile</label>
        <input
          id="e-name"
          type="text"
          placeholder="staging"
          autocomplete="off"
          spellcheck="false"
          bind:value={newEnvName}
          class="input"
          class:input-invalid={duplicate}
          data-autofocus
        />
        {#if duplicate}
          <span class="field-error">A profile with this name already exists.</span>
        {:else}
          <span class="field-hint">Switch profiles from the sidebar profile menu.</span>
        {/if}
      </div>
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={onClose}>Close</button>
      <button type="submit" class="btn btn-primary" disabled={!trimmed || duplicate}>Create profile</button>
    </div>
  </form>
</ModalShell>
