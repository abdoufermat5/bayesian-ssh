<script lang="ts">
  import type { Terminal } from "@xterm/xterm";
  import type { TerminalMenuActions } from "$lib/utils/terminal-context-menu";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";
  import { TERMINAL_SHORTCUTS } from "$lib/shortcuts";
  import {
    Copy,
    ClipboardPaste,
    TextSelect,
    Search,
    Eraser,
    Download,
  } from "lucide-svelte";

  interface Props {
    x: number;
    y: number;
    term: Terminal;
    actions: TerminalMenuActions;
    onClose: (refocusTerminal?: boolean) => void;
  }

  let { x, y, term, actions, onClose }: Props = $props();

  let menuEl = $state<HTMLDivElement>();
  let style = $state("position: fixed; top: 0px; left: 0px; visibility: hidden;");

  function getShortcutHint(id: string): string | undefined {
    const item = TERMINAL_SHORTCUTS.find((s) => s.id === id);
    const combo = item?.combos[0];
    return combo ? combo.join("+") : undefined;
  }

  interface MenuItem {
    id: string;
    label: string;
    icon: typeof Copy;
    shortcut?: string;
    disabled?: boolean;
    action: () => void;
  }

  const hasSelection = $derived(term.hasSelection());

  const sections = $derived<MenuItem[][]>([
    [
      {
        id: "copy",
        label: "Copy",
        icon: Copy,
        shortcut: getShortcutHint("term-copy"),
        disabled: !hasSelection,
        action: () => {
          const selection = term.getSelection();
          if (selection) {
            copyTextWithFallback(selection);
          }
          term.clearSelection();
          onClose(true);
        },
      },
      {
        id: "paste",
        label: "Paste",
        icon: ClipboardPaste,
        shortcut: getShortcutHint("term-paste"),
        disabled: false,
        action: () => {
          onClose(false);
          actions.paste();
        },
      },
      {
        id: "select-all",
        label: "Select all",
        icon: TextSelect,
        disabled: false,
        action: () => {
          term.selectAll();
          onClose(true);
        },
      },
    ],
    [
      {
        id: "find",
        label: "Find",
        icon: Search,
        shortcut: getShortcutHint("term-find"),
        disabled: false,
        action: () => {
          onClose(false);
          actions.find();
        },
      },
    ],
    [
      {
        id: "clear",
        label: "Clear",
        icon: Eraser,
        disabled: false,
        action: () => {
          term.clear();
          onClose(true);
        },
      },
      {
        id: "export-scrollback",
        label: "Export scrollback",
        icon: Download,
        disabled: false,
        action: () => {
          onClose(true);
          actions.exportScrollback();
        },
      },
    ],
  ]);

  const indexedSections = $derived.by(() => {
    let flatIndex = 0;
    return sections.map((sec) =>
      sec.map((item) => ({
        ...item,
        flatIndex: flatIndex++,
      }))
    );
  });

  let focusedIndex = $state<number>(0);

  function getActionableElements(): HTMLButtonElement[] {
    if (!menuEl) return [];
    return Array.from(menuEl.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]:not(:disabled)'));
  }

  $effect(() => {
    if (!menuEl) return;
    const rect = menuEl.getBoundingClientRect();
    const width = rect.width || 210;
    const height = rect.height || 220;
    const pad = 6;
    const left = Math.max(pad, Math.min(x, window.innerWidth - width - pad));
    const top = Math.max(pad, Math.min(y, window.innerHeight - height - pad));
    style = `position: fixed; top: ${top}px; left: ${left}px; visibility: visible;`;

    requestAnimationFrame(() => {
      const actionable = getActionableElements();
      if (actionable.length > 0) {
        actionable[0].focus();
      }
    });
  });

  function onKeydown(e: KeyboardEvent) {
    const actionable = getActionableElements();
    if (actionable.length === 0) return;

    const currentEl = document.activeElement as HTMLButtonElement | null;
    const currentIndex = currentEl ? actionable.indexOf(currentEl) : -1;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      const nextIndex = currentIndex === -1 ? 0 : (currentIndex + 1) % actionable.length;
      actionable[nextIndex]?.focus();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      const prevIndex = currentIndex === -1 ? actionable.length - 1 : (currentIndex - 1 + actionable.length) % actionable.length;
      actionable[prevIndex]?.focus();
    } else if (e.key === "Home") {
      e.preventDefault();
      e.stopPropagation();
      actionable[0]?.focus();
    } else if (e.key === "End") {
      e.preventDefault();
      e.stopPropagation();
      actionable[actionable.length - 1]?.focus();
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      e.stopPropagation();
      if (currentEl && actionable.includes(currentEl)) {
        currentEl.click();
      } else if (actionable.length > 0) {
        actionable[0].click();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose(true);
    } else if (e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      onClose(true);
    }
  }

  function onPointerDown(e: PointerEvent) {
    const target = e.target as Node | null;
    if (target && menuEl && menuEl.contains(target)) return;
    onClose(false);
  }

  function onBlur(e: FocusEvent) {
    if (e.target === window || !e.target || e.target === document) {
      onClose(false);
    }
  }

  function onResize() {
    onClose(false);
  }

  function onScroll(e: Event) {
    if (menuEl && e.target instanceof Node && menuEl.contains(e.target)) return;
    onClose(false);
  }
</script>

<svelte:window
  onpointerdown={onPointerDown}
  onblur={onBlur}
  onresize={onResize}
  onscrollcapture={onScroll}
/>

<div
  bind:this={menuEl}
  class="popover fixed z-[200] min-w-[210px] max-h-[calc(100vh-12px)] overflow-y-auto shadow-xl outline-none"
  {style}
  role="menu"
  aria-label="Terminal actions"
  tabindex="-1"
  onkeydown={onKeydown}
>
  {#each indexedSections as section, sIndex (sIndex)}
    {#if sIndex > 0}
      <div class="menu-separator" role="separator" aria-orientation="horizontal"></div>
    {/if}
    {#each section as item (item.id)}
      <button
        type="button"
        role="menuitem"
        tabindex={focusedIndex === item.flatIndex ? 0 : -1}
        disabled={item.disabled}
        aria-disabled={item.disabled ? "true" : undefined}
        class="menu-item group"
        onclick={(e) => {
          e.stopPropagation();
          if (!item.disabled) item.action();
        }}
        onfocus={() => {
          if (!item.disabled) focusedIndex = item.flatIndex;
        }}
      >
        <item.icon size={14} class="shrink-0 text-muted transition-colors group-hover:text-primary" aria-hidden="true" />
        <span class="min-w-0 flex-1 truncate">{item.label}</span>
        {#if item.shortcut}
          <kbd class="kbd ml-3 shrink-0" aria-hidden="true">{item.shortcut}</kbd>
        {/if}
      </button>
    {/each}
  {/each}
</div>

<style>
  .menu-item:focus-visible,
  .menu-item:focus {
    outline: none;
    background-color: var(--color-hover-on-floating);
    background-image: linear-gradient(var(--color-selected), var(--color-selected));
    box-shadow: inset 2px 0 0 var(--color-accent);
    color: var(--color-primary);
  }
  .menu-item:disabled {
    cursor: not-allowed;
    color: var(--color-disabled-fg);
    background-color: transparent !important;
    background-image: none !important;
    box-shadow: none !important;
    pointer-events: none;
  }
  .menu-item:disabled :global(svg) {
    color: var(--color-disabled-fg) !important;
    opacity: 0.6;
  }
  .menu-item:disabled .kbd {
    opacity: 0.4;
  }
</style>
