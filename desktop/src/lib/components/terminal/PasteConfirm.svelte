<script lang="ts">
  import { X, AlertTriangle } from "lucide-svelte";

  interface Props {
    text: string;
    onConfirm: () => void;
    onCancel: () => void;
  }

  let { text, onConfirm, onCancel }: Props = $props();

  let panelRef = $state<HTMLElement | null>(null);
  let cancelButtonRef = $state<HTMLButtonElement | null>(null);
  let pasteButtonRef = $state<HTMLButtonElement | null>(null);

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

  function handleCancel() {
    onCancel();
  }

  function handleConfirm() {
    onConfirm();
  }

  const FOCUSABLE_SELECTOR = [
    "a[href]",
    "button:not([disabled])",
    "input:not([disabled])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    '[tabindex]:not([tabindex="-1"])',
  ].join(", ");

  function getFocusableElements(): HTMLElement[] {
    if (!panelRef) return [];
    return Array.from(panelRef.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
      (el) => el.offsetParent !== null || el === document.activeElement,
    );
  }

  $effect(() => {
    // Default focus to Cancel button
    const raf = requestAnimationFrame(() => {
      cancelButtonRef?.focus();
    });

    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    function handleKeydown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        handleCancel();
        return;
      }

      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        handleConfirm();
        return;
      }

      if (e.key === "Tab") {
        const focusables = getFocusableElements();
        if (focusables.length === 0) {
          e.preventDefault();
          panelRef?.focus();
          return;
        }
        const active = document.activeElement as HTMLElement | null;
        const first = focusables[0];
        const last = focusables[focusables.length - 1];

        if (e.shiftKey && (active === first || !panelRef?.contains(active))) {
          e.preventDefault();
          last.focus();
        } else if (!e.shiftKey && (active === last || !panelRef?.contains(active))) {
          e.preventDefault();
          first.focus();
        }
      }
    }

    window.addEventListener("keydown", handleKeydown, true);

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("keydown", handleKeydown, true);
      document.body.style.overflow = previousOverflow;
    };
  });
</script>

<div
  class="modal-overlay"
  role="presentation"
  onpointerdown={(e) => {
    if (e.target === e.currentTarget) handleCancel();
  }}
>
  <div
    bind:this={panelRef}
    class="modal-panel max-w-[480px] outline-none"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="paste-confirm-title"
    aria-describedby="paste-confirm-warning"
    tabindex="-1"
  >
    <div class="modal-header">
      <div class="min-w-0">
        <h2 id="paste-confirm-title" class="modal-title break-words">
          Paste {lineCount} {lineCount === 1 ? "line" : "lines"}?
        </h2>
      </div>
      <button
        type="button"
        class="modal-close"
        onclick={handleCancel}
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
      <button
        type="button"
        class="btn btn-secondary"
        onclick={handleCancel}
        bind:this={cancelButtonRef}
        data-autofocus
      >
        Cancel
      </button>
      <button
        type="button"
        class="btn btn-primary"
        onclick={handleConfirm}
        bind:this={pasteButtonRef}
      >
        Paste
      </button>
    </div>
  </div>
</div>
