<script lang="ts">
  import type { Snippet } from "svelte";

  type ModalWidth = "sm" | "md" | "lg" | "full";

  interface Props {
    open: boolean;
    title?: string;
    onClose: () => void;
    closeOnEscape?: boolean;
    closeOnBackdrop?: boolean;
    width?: ModalWidth;
    /** Extra classes appended to the panel (e.g. max-height overrides). */
    panelClass?: string;
    /** Inline styles for the panel (e.g. resizable width/height). */
    panelStyle?: string;
    /** Extra classes appended to the fixed overlay (e.g. top offset). */
    overlayClass?: string;
    /** Inline styles for the fixed overlay. */
    overlayStyle?: string;
    children: Snippet;
  }

  let {
    open,
    title,
    onClose,
    closeOnEscape = true,
    closeOnBackdrop = true,
    width = "md",
    panelClass = "",
    panelStyle = "",
    overlayClass = "",
    overlayStyle = "",
    children,
  }: Props = $props();

  // Read only inside event/callback handlers so the $effect never re-runs on it.
  let panelRef = $state<HTMLElement | null>(null);

  const WIDTH_CLASSES: Record<ModalWidth, string> = {
    sm: "w-full max-w-sm",
    md: "w-full max-w-md",
    lg: "w-full max-w-3xl",
    full: "w-full h-full max-w-none rounded-none border-none shadow-none",
  };

  const FOCUSABLE_SELECTOR = [
    "a[href]",
    "button:not([disabled])",
    "input:not([disabled])",
    "select:not([disabled])",
    "textarea:not([disabled])",
    '[tabindex]:not([tabindex="-1"])',
    "audio[controls]",
    "video[controls]",
  ].join(", ");

  function getFocusableElements(): HTMLElement[] {
    if (!panelRef) return [];
    return Array.from(panelRef.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
  }

  // Nested popups (e.g. CustomSelect dropdowns) handle Escape themselves;
  // the shell must not swallow that key press and close the whole dialog.
  function isInsideNestedPopup(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    return (
      target.closest('[role="listbox"]') !== null ||
      target.getAttribute("aria-expanded") === "true"
    );
  }

  $effect(() => {
    if (!open) return;

    const previousActive = document.activeElement as HTMLElement | null;
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";

    function handleKeydown(e: KeyboardEvent) {
      const active = document.activeElement as HTMLElement | null;
      // If focus lives inside a nested dialog (e.g. Onboarding's passphrase
      // prompt), that dialog owns Escape/Tab handling — defer to it.
      const containingDialog = active?.closest('[role="dialog"]');
      const ownsFocus = !containingDialog || containingDialog === panelRef;

      if (e.key === "Escape") {
        if (closeOnEscape && ownsFocus && !isInsideNestedPopup(e.target)) {
          e.preventDefault();
          onClose();
        }
        return;
      }

      // Focus trap: cycle Tab / Shift+Tab among focusable children.
      if (e.key !== "Tab" || !ownsFocus) return;
      const focusables = getFocusableElements();
      if (focusables.length === 0) return;
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

    window.addEventListener("keydown", handleKeydown);

    // Initial focus on the first focusable child once the panel is laid out.
    const raf = requestAnimationFrame(() => {
      getFocusableElements()[0]?.focus();
    });

    return () => {
      window.removeEventListener("keydown", handleKeydown);
      cancelAnimationFrame(raf);
      document.body.style.overflow = previousOverflow;
      previousActive?.focus();
    };
  });
</script>

{#if open}
  <div
    class="modal-overlay backdrop-blur-sm {overlayClass}"
    style={overlayStyle}
    role="presentation"
    onpointerdown={(e) => {
      if (closeOnBackdrop && e.target === e.currentTarget) onClose();
    }}
  >
    <div
      bind:this={panelRef}
      class="modal-panel {WIDTH_CLASSES[width]} {panelClass}"
      style={panelStyle}
      role="dialog"
      aria-modal="true"
      aria-label={title ?? undefined}
      tabindex="-1"
    >
      {@render children()}
    </div>
  </div>
{/if}
