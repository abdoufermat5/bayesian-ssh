<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    /** Viewport position of the menu's top-left corner (clamped on screen). */
    x: number;
    y: number;
    label: string;
    /** `dialog` for a small form (e.g. rename), `menu` for a list of actions. */
    role?: "menu" | "dialog";
    onClose: () => void;
    children: Snippet;
  }

  let { x, y, label, role = "menu", onClose, children }: Props = $props();

  let menuEl = $state<HTMLDivElement>();
  let style = $state("position: fixed; top: 0px; left: 0px; visibility: hidden;");

  const ITEM_SELECTOR = "button:not(:disabled), input";

  function items(): HTMLElement[] {
    return menuEl ? Array.from(menuEl.querySelectorAll<HTMLElement>(ITEM_SELECTOR)) : [];
  }

  $effect(() => {
    if (!menuEl) return;
    const rect = menuEl.getBoundingClientRect();
    const pad = 6;
    const left = Math.max(pad, Math.min(x, window.innerWidth - rect.width - pad));
    const top = Math.max(pad, Math.min(y, window.innerHeight - rect.height - pad));
    style = `position: fixed; top: ${top}px; left: ${left}px; visibility: visible;`;
    requestAnimationFrame(() => items()[0]?.focus());
  });

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp" && e.key !== "Home" && e.key !== "End") return;
    const list = items();
    if (list.length === 0) return;
    e.preventDefault();
    e.stopPropagation();
    const current = list.indexOf(document.activeElement as HTMLElement);
    const next =
      e.key === "Home"
        ? 0
        : e.key === "End"
          ? list.length - 1
          : (current + (e.key === "ArrowDown" ? 1 : -1) + list.length) % list.length;
    list[next].focus();
  }

  function onWindowPointerDown(e: PointerEvent) {
    if (menuEl && e.target instanceof Node && menuEl.contains(e.target)) return;
    onClose();
  }
</script>

<svelte:window onpointerdown={onWindowPointerDown} onblur={onClose} onresize={onClose} />

<div
  bind:this={menuEl}
  class="popover fixed z-[200] min-w-[220px] max-h-[calc(100vh-12px)] overflow-y-auto shadow-xl outline-none"
  {style}
  {role}
  aria-label={label}
  tabindex="-1"
  onkeydown={onKeydown}
>
  {@render children()}
</div>
