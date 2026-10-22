<script lang="ts" module>
  import { HOST_SHORTCUTS } from "$lib/shortcuts";

  /** Label of a host shortcut, from the shortcut table. */
  export function hostLabel(id: string): string {
    return HOST_SHORTCUTS.find((s) => s.id === id)?.label ?? id;
  }

  /** First key combination of a host shortcut, for hints ("Shift+Enter"). */
  export function hostKeys(id: string): string {
    const combo = HOST_SHORTCUTS.find((s) => s.id === id)?.combos[0];
    return combo ? combo.join("+") : "";
  }
</script>

<script lang="ts">
  import {
    Copy,
    CopyPlus,
    FolderOpen,
    MoreHorizontal,
    Pencil,
    Share2,
    SquareTerminal,
    Trash2,
  } from "lucide-svelte";
  import type { Connection } from "$lib/types";
  import AnchoredPopover from "./AnchoredPopover.svelte";

  interface Props {
    conn: Connection;
    connecting: boolean;
    /** Another connect is in flight. */
    busy: boolean;
    showFiles: boolean;
    /** Team name when this host is a team copy: sharing then updates it there. */
    teamName: string | null;
    onConnect: () => void;
    onFiles: () => void;
    onCopy: () => void;
    onEdit: () => void;
    onDuplicate: () => void;
    onShare: () => void;
    onDelete: () => void;
  }

  let {
    conn,
    connecting,
    busy,
    showFiles,
    teamName,
    onConnect,
    onFiles,
    onCopy,
    onEdit,
    onDuplicate,
    onShare,
    onDelete,
  }: Props = $props();

  let menuOpen = $state(false);
  let moreButton = $state<HTMLButtonElement>();
  let menuEl = $state<HTMLDivElement>();

  const shareLabel = $derived(teamName ? `Update in ${teamName}` : hostLabel("host-share"));

  type Item = { id: string; label: string; keys?: string; icon: typeof Copy; danger?: boolean; run: () => void };
  const items = $derived<Item[]>([
    { id: "copy", label: hostLabel("host-copy"), keys: hostKeys("host-copy"), icon: Copy, run: onCopy },
    { id: "edit", label: hostLabel("host-edit"), keys: hostKeys("host-edit"), icon: Pencil, run: onEdit },
    { id: "duplicate", label: "Duplicate", icon: CopyPlus, run: onDuplicate },
    { id: "share", label: shareLabel, keys: hostKeys("host-share"), icon: Share2, run: onShare },
    { id: "delete", label: hostLabel("host-delete"), keys: hostKeys("host-delete"), icon: Trash2, danger: true, run: onDelete },
  ]);

  function menuItems(): HTMLElement[] {
    return Array.from(menuEl?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? []);
  }

  // Focus the first item once the menu is mounted.
  $effect(() => {
    if (menuOpen && menuEl) requestAnimationFrame(() => menuItems()[0]?.focus());
  });

  function onMenuKeydown(e: KeyboardEvent) {
    const list = menuItems();
    const i = list.indexOf(document.activeElement as HTMLElement);
    let next = -1;
    if (e.key === "ArrowDown") next = (i + 1) % list.length;
    else if (e.key === "ArrowUp") next = (i - 1 + list.length) % list.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = list.length - 1;
    if (next === -1) return;
    e.preventDefault();
    e.stopPropagation();
    list[next]?.focus();
  }

  function run(item: Item) {
    menuOpen = false;
    // Back to the row (the grid's tab stop) so host keys keep working.
    (moreButton?.closest<HTMLElement>("[data-host-focus]") ?? moreButton)?.focus();
    item.run();
  }
</script>

<div class="host-actions flex items-center justify-end gap-0.5">
  <button
    type="button"
    class="btn-icon host-connect"
    tabindex="-1"
    onclick={(e) => {
      e.stopPropagation();
      onConnect();
    }}
    ondblclick={(e) => e.stopPropagation()}
    disabled={busy}
    title="{hostLabel('host-connect')} ({hostKeys('host-connect')})"
    aria-label="{hostLabel('host-connect')} to {conn.name}"
  >
    {#if connecting}
      <span class="spinner size-3"></span>
    {:else}
      <SquareTerminal size={14} />
    {/if}
  </button>
  {#if showFiles}
    <button
      type="button"
      class="btn-icon"
      tabindex="-1"
      onclick={(e) => {
        e.stopPropagation();
        onFiles();
      }}
      ondblclick={(e) => e.stopPropagation()}
      title="{hostLabel('host-files')} ({hostKeys('host-files')})"
      aria-label="{hostLabel('host-files')} on {conn.name}"
    >
      <FolderOpen size={14} />
    </button>
  {/if}
  <button
    bind:this={moreButton}
    type="button"
    class="btn-icon"
    tabindex="-1"
    onclick={(e) => {
      e.stopPropagation();
      menuOpen = !menuOpen;
    }}
    ondblclick={(e) => e.stopPropagation()}
    aria-haspopup="menu"
    aria-expanded={menuOpen}
    title="More actions"
    aria-label="More actions for {conn.name}"
  >
    <MoreHorizontal size={14} />
  </button>
</div>

{#if menuOpen && moreButton}
  <AnchoredPopover
    anchor={moreButton}
    role="menu"
    label="Actions for {conn.name}"
    width={232}
    bind:element={menuEl}
    onClose={() => (menuOpen = false)}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div onkeydown={onMenuKeydown}>
      {#each items as item (item.id)}
        {#if item.danger}<div class="menu-separator" role="separator"></div>{/if}
        <button
          type="button"
          role="menuitem"
          tabindex="-1"
          class="menu-item {item.danger ? 'menu-item-danger' : ''}"
          onclick={(e) => {
            e.stopPropagation();
            run(item);
          }}
          ondblclick={(e) => e.stopPropagation()}
        >
          <item.icon size={14} aria-hidden="true" />
          <span class="min-w-0 flex-1 truncate">{item.label}</span>
          {#if item.keys}<kbd class="kbd" aria-hidden="true">{item.keys}</kbd>{/if}
        </button>
      {/each}
    </div>
  </AnchoredPopover>
{/if}

<style>
  /* Connect is the primary action on the row the user is on. */
  :global(tr:hover) .host-connect:not(:disabled),
  :global(tr.is-selected) .host-connect:not(:disabled),
  :global(.host-card:hover) .host-connect:not(:disabled),
  :global(.host-card.is-selected) .host-connect:not(:disabled) {
    background: var(--color-accent-strong);
    color: var(--color-on-accent);
  }
</style>
