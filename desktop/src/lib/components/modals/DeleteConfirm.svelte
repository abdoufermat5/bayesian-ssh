<script lang="ts">
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    /** Defaults to "Delete <label>?". */
    title?: string;
    confirmLabel?: string;
    label: string;
    subtitle: string;
    warning?: string;
    onCancel: () => void;
    onConfirm: () => void;
  }

  let {
    title,
    confirmLabel = "Delete",
    label,
    subtitle,
    warning = "This can't be undone.",
    onCancel,
    onConfirm,
  }: Props = $props();

  const heading = $derived(title ?? `Delete ${label}?`);
  // Callers pass either a sentence ("All hosts … removed.") or a literal value
  // (user@host:port, a shell command). Sentences read as prose; values as code.
  const subtitleIsProse = $derived(/\s/.test(subtitle.trim()) && /[.!?]$/.test(subtitle.trim()));
</script>

<ModalShell open={true} title={heading} onClose={onCancel} width="sm">
  <div class="modal-header">
    <h2 class="modal-title break-words">{heading}</h2>
    <button type="button" class="modal-close" onclick={onCancel} aria-label="Close">
      <X size={16} />
    </button>
  </div>

  <div class="modal-body flex flex-col gap-2.5">
    {#if title}
      <p class="m-0 break-all text-sm font-medium text-primary">{label}</p>
    {/if}
    {#if subtitle}
      {#if subtitleIsProse}
        <p class="m-0 text-sm leading-relaxed text-secondary">{subtitle}</p>
      {:else}
        <div class="code-block max-h-40 whitespace-pre-wrap break-all">{subtitle}</div>
      {/if}
    {/if}
    {#if warning}
      <p class="m-0 text-xs leading-relaxed text-muted">{warning}</p>
    {/if}
  </div>

  <div class="modal-footer">
    <button type="button" class="btn btn-secondary" onclick={onCancel} data-autofocus>Cancel</button>
    <button type="button" class="btn btn-danger" onclick={onConfirm}>{confirmLabel}</button>
  </div>
</ModalShell>
