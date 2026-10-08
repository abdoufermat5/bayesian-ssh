<script lang="ts" module>
  // Open shells in mount order; the last one is topmost. Used so Escape /
  // Tab only reach the topmost dialog when focus sits outside every dialog.
  const openStack: symbol[] = [];
  let bodyOverflowBeforeModals = "";
</script>

<script lang="ts">
  import type { Snippet } from "svelte";

  type ModalWidth = "sm" | "md" | "form" | "lg" | "full";

  interface Props {
    open: boolean;
    title?: string;
    onClose: () => void;
    closeOnEscape?: boolean;
    closeOnBackdrop?: boolean;
    /** sm 400px (confirms), md 480px, form 560px, lg 768px, full = whole window. */
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
    sm: "max-w-[400px]",
    md: "max-w-[480px]",
    form: "max-w-[560px]",
    lg: "max-w-3xl",
    full: "h-full max-h-none max-w-none rounded-none border-none shadow-none",
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
    return Array.from(panelRef.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)).filter(
      (el) => el.offsetParent !== null || el === document.activeElement,
    );
  }

  /** Initial focus: explicit [data-autofocus], else the first field, else the
   *  first control that isn't the header close button, else the panel. */
  function focusInitial() {
    if (!panelRef) return;
    const explicit = panelRef.querySelector<HTMLElement>("[data-autofocus]");
    if (explicit) return explicit.focus();
    const focusables = getFocusableElements();
    const field = focusables.find((el) => el.matches("input, textarea, select"));
    const target = field ?? focusables.find((el) => !el.classList.contains("modal-close"));
    (target ?? panelRef).focus();
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

    const id = Symbol("modal");
    if (openStack.length === 0) bodyOverflowBeforeModals = document.body.style.overflow;
    openStack.push(id);
    const previousActive = document.activeElement as HTMLElement | null;
    document.body.style.overflow = "hidden";

    function handleKeydown(e: KeyboardEvent) {
      const active = document.activeElement as HTMLElement | null;
      // Focus inside a dialog: that dialog owns the key. Focus elsewhere
      // (body, backdrop click): only the topmost shell reacts.
      const containingDialog = active?.closest('[role="dialog"]');
      const ownsFocus = containingDialog
        ? containingDialog === panelRef
        : openStack[openStack.length - 1] === id;

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
      if (focusables.length === 0) {
        e.preventDefault();
        panelRef?.focus();
        return;
      }
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
    const raf = requestAnimationFrame(focusInitial);

    return () => {
      window.removeEventListener("keydown", handleKeydown);
      cancelAnimationFrame(raf);
      const idx = openStack.indexOf(id);
      if (idx !== -1) openStack.splice(idx, 1);
      if (openStack.length === 0) document.body.style.overflow = bodyOverflowBeforeModals;
      if (previousActive?.isConnected) previousActive.focus({ preventScroll: true });
    };
  });
</script>

{#if open}
  <div
    class="modal-overlay {overlayClass}"
    style={overlayStyle}
    role="presentation"
    onpointerdown={(e) => {
      if (closeOnBackdrop && e.target === e.currentTarget) onClose();
    }}
  >
    <div
      bind:this={panelRef}
      class="modal-panel outline-none {WIDTH_CLASSES[width]} {panelClass}"
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
