<script lang="ts">
  import {
    AppWindow,
    ArrowLeft,
    ArrowRight,
    Columns2,
    Group,
    Minimize2,
    Rows2,
    Ungroup,
    Unlink,
    X,
  } from "lucide-svelte";
  import FloatingMenu from "$lib/components/terminal/FloatingMenu.svelte";
  import {
    addTabToGroup,
    createTabGroup,
    detachTab,
    disconnectTab,
    getTerminalState,
    moveTab,
    popOutTab,
    removeTabFromGroup,
    splitTab,
    unsplitTab,
    type TerminalTab,
  } from "$lib/stores/terminal.svelte";
  import type { SplitSide } from "$lib/utils/split-layout";
  import { tabGroupColorVar, tabGroupLabel } from "$lib/utils/tab-groups";

  interface Props {
    tab: TerminalTab;
    x: number;
    y: number;
    /** The tab is a pane of some split. */
    inSplit: boolean;
    onClose: () => void;
    /** Open the host launcher to start a session on `side` of this (active) tab. */
    onSplitNew: (side: SplitSide) => void;
    /** A group was started with this tab; the caller opens its editor at `at`. */
    onGroupCreated: (groupId: string, at: { x: number; y: number }) => void;
  }

  let { tab, x, y, inSplit, onClose, onSplitNew, onGroupCreated }: Props = $props();

  const terminalState = getTerminalState();
  const index = $derived(terminalState.tabs.findIndex((t) => t.id === tab.id));
  const activeId = $derived(terminalState.activeTabId);
  const otherGroups = $derived(terminalState.groups.filter((g) => g.id !== tab.groupId));

  /** Read `tab` before closing: the menu unmounts with it. */
  function run(action: () => void | Promise<void>) {
    const done = action();
    onClose();
    void done;
  }

  function newGroup() {
    const at = { x, y };
    const groupId = createTabGroup(tab.id);
    onClose();
    if (groupId) onGroupCreated(groupId, at);
  }

  function splitNew(side: SplitSide) {
    onClose();
    onSplitNew(side);
  }

  /** One place left or right in the strip. */
  function shift(step: 1 | -1) {
    const tabs = terminalState.tabs;
    if (step < 0) moveTab(tab.id, tabs[index - 1].id);
    else moveTab(tab.id, tabs[index + 2]?.id ?? null);
  }
</script>

<FloatingMenu {x} {y} label="Actions for {tab.name}" {onClose}>
  <button type="button" class="menu-item" role="menuitem" onclick={newGroup}>
    <Group size={14} />
    Add to new group
  </button>
  {#each otherGroups as group (group.id)}
    <button type="button" class="menu-item" role="menuitem" onclick={() => run(() => addTabToGroup(tab.id, group.id))}>
      <span class="tab-group-dot mx-0.5" style:--group-color={tabGroupColorVar(group.color)} aria-hidden="true"></span>
      <span class="min-w-0 truncate">Add to {tabGroupLabel(group)}</span>
    </button>
  {/each}
  {#if tab.groupId}
    <button type="button" class="menu-item" role="menuitem" onclick={() => run(() => removeTabFromGroup(tab.id))}>
      <Ungroup size={14} />
      Remove from group
    </button>
  {/if}
  <div class="menu-separator" role="separator"></div>
  {#if tab.id === activeId}
    <button type="button" class="menu-item" role="menuitem" onclick={() => splitNew("right")}>
      <Columns2 size={14} />
      Split right…
    </button>
    <button type="button" class="menu-item" role="menuitem" onclick={() => splitNew("bottom")}>
      <Rows2 size={14} />
      Split down…
    </button>
  {:else if activeId}
    <button type="button" class="menu-item" role="menuitem" onclick={() => run(() => splitTab(tab.id, activeId, "right"))}>
      <Columns2 size={14} />
      Show right of current tab
    </button>
    <button type="button" class="menu-item" role="menuitem" onclick={() => run(() => splitTab(tab.id, activeId, "bottom"))}>
      <Rows2 size={14} />
      Show below current tab
    </button>
  {/if}
  {#if inSplit}
    <button type="button" class="menu-item" role="menuitem" onclick={() => run(() => unsplitTab(tab.id))}>
      <Minimize2 size={14} />
      Remove from split
    </button>
  {/if}
  <div class="menu-separator" role="separator"></div>
  <button type="button" class="menu-item" role="menuitem" disabled={index <= 0} onclick={() => run(() => shift(-1))}>
    <ArrowLeft size={14} />
    Move left
  </button>
  <button
    type="button"
    class="menu-item"
    role="menuitem"
    disabled={index === terminalState.tabs.length - 1}
    onclick={() => run(() => shift(1))}
  >
    <ArrowRight size={14} />
    Move right
  </button>
  <div class="menu-separator" role="separator"></div>
  <button type="button" class="menu-item" role="menuitem" onclick={() => run(() => popOutTab(tab.id))}>
    <AppWindow size={14} />
    Open in new window
  </button>
  <button type="button" class="menu-item" role="menuitem" onclick={() => run(() => detachTab(tab.id))}>
    <Unlink size={14} />
    Run in background
  </button>
  <button type="button" class="menu-item menu-item-danger" role="menuitem" onclick={() => run(() => disconnectTab(tab.id))}>
    <X size={14} />
    Close session
  </button>
</FloatingMenu>
