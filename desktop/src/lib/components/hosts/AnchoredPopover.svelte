<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    /** The control that opened it; focus returns there on Escape. */
    anchor: HTMLElement;
    onClose: () => void;
    role?: "menu" | "dialog";
    label: string;
    /** Popover width in px; it right-aligns with the anchor. */
    width?: number;
    /** Bound to the popover element, for focus management by the owner. */
    element?: HTMLDivElement;
    children: Snippet;
  }

  let { anchor, onClose, role = "dialog", label, width = 200, element = $bindable(), children }: Props = $props();

  // Fixed positioning: host rows sit inside `overflow: hidden` table wrappers
  // that would clip an absolutely positioned popover.
  let style = $state("visibility: hidden");

  $effect(() => {
    const el = element;
    if (!el) return;
    const r = anchor.getBoundingClientRect();
    const h = el.offsetHeight;
    const below = r.bottom + 4 + h <= window.innerHeight - 8;
    const top = below ? r.bottom + 4 : Math.max(8, r.top - 4 - h);
    const left = Math.min(Math.max(8, r.right - width), window.innerWidth - width - 8);
    style = `top: ${top}px; left: ${left}px; width: ${width}px`;
  });

  function onPointerDown(e: PointerEvent) {
    const t = e.target as Node | null;
    if (t && (element?.contains(t) || anchor.contains(t))) return;
    onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      anchor.focus();
    } else if (e.key === "Tab") {
      onClose();
    }
  }

  function onScroll(e: Event) {
    if (element && e.target instanceof Node && element.contains(e.target)) return;
    onClose();
  }
</script>

<svelte:window onpointerdown={onPointerDown} onresize={onClose} onscrollcapture={onScroll} />

<div
  bind:this={element}
  class="popover fixed"
  {style}
  {role}
  aria-label={label}
  tabindex="-1"
  onkeydown={onKeydown}
>
  {@render children()}
</div>
