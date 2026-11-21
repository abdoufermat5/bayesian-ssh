<script lang="ts">
  import { X, AlertTriangle } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import { MOD } from "$lib/shortcuts";

  interface Props {
    text: string;
    onConfirm: () => void;
    onCancel: () => void;
  }

  let { text, onConfirm, onCancel }: Props = $props();

  // Parse lines after trimming one trailing newline
  const lines = $derived.by(() => {
    let s = text;
    if (s.endsWith("\r\n")) {
      s = s.slice(0, -2);
    } else if (s.endsWith("\n") || s.endsWith("\r")) {
      s = s.slice(0, -1);
    }
    return s.split(/\r\n|\r|\n/);
  });

  const lineCount = $derived(lines.length);
  const previewLines = $derived(lines.slice(0, 20));
  const remainingCount = $derived(lines.length > 20 ? lines.length - 20 : 0);
  const heading = $derived(`Paste ${lineCount} ${lineCount === 1 ? "line" : "lines"}?`);

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !e.altKey) {
      e.preventDefault();
      e.stopPropagation();
      onConfirm();
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<ModalShell open={true} title={heading} onClose={onCancel} width="md" role="alertdialog">
  <div class="modal-header">
    <div class="min-w-0">
      <h2 id="paste-confirm-title" class="modal-title break-words">
        {heading}
      </h2>
    </div>
    <button
      type="button"
      class="modal-close"
      onclick={onCancel}
      aria-label="Close"
    >
      <X size={16} />
    </button>
  </div>

  <div class="modal-body flex flex-col gap-3">
    <div class="alert alert-warning" id="paste-confirm-warning" role="alert">
      <AlertTriangle size={15} class="mt-0.5 shrink-0 text-warning" />
      <span class="text-xs leading-relaxed">
        Each line will run as a command. Are you sure you want to paste?
      </span>
    </div>

    <div class="code-block max-h-60 overflow-y-auto whitespace-pre-wrap break-all font-mono text-xs leading-relaxed">
      {previewLines.join("\n")}
      {#if remainingCount > 0}
        <div class="mt-1.5 text-muted italic">…and {remainingCount} more</div>
      {/if}
    </div>
  </div>

  <div class="modal-footer">
    <span class="mr-auto text-xs text-muted">
      <kbd class="kbd">{MOD}</kbd> + <kbd class="kbd">Enter</kbd> to paste
    </span>
    <button
      type="button"
      class="btn btn-secondary"
      onclick={onCancel}
      data-autofocus
    >
      Cancel
    </button>
    <button
      type="button"
      class="btn btn-primary"
      onclick={onConfirm}
      title="Paste ({MOD}+Enter)"
    >
      Paste
    </button>
  </div>
</ModalShell>
