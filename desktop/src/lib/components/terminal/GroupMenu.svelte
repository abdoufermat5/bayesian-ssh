<script lang="ts">
  import { Layers, Ungroup, X } from "lucide-svelte";
  import FloatingMenu from "$lib/components/terminal/FloatingMenu.svelte";
  import { renameTabGroup, setTabGroupColor } from "$lib/stores/tab-groups.svelte";
  import {
    closeTabGroup,
    getTerminalState,
    toggleTabGroupCollapsed,
    ungroupTabGroup,
  } from "$lib/stores/terminal.svelte";
  import { TAB_GROUP_COLORS, tabGroupColorVar, type TabGroup } from "$lib/utils/tab-groups";

  interface Props {
    group: TabGroup;
    x: number;
    y: number;
    onClose: () => void;
  }

  let { group, x, y, onClose }: Props = $props();

  const terminalState = getTerminalState();
  const count = $derived(terminalState.tabs.filter((t) => t.groupId === group.id).length);

  /** Read `group` before closing: the menu unmounts with it. */
  function run(action: () => void | Promise<void>) {
    const done = action();
    onClose();
    void done;
  }
</script>

<FloatingMenu role="dialog" {x} {y} label="Edit group" {onClose}>
  <div class="flex flex-col gap-2 p-1.5">
    <input
      class="input h-8"
      type="text"
      placeholder="Name this group"
      aria-label="Group name"
      value={group.name}
      oninput={(e) => renameTabGroup(group.id, e.currentTarget.value)}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onClose();
        }
      }}
    />
    <div class="flex items-center gap-1.5 px-0.5" role="radiogroup" aria-label="Group colour">
      {#each TAB_GROUP_COLORS as color (color)}
        <button
          type="button"
          role="radio"
          class="tab-group-swatch"
          style:--group-color={tabGroupColorVar(color)}
          aria-checked={group.color === color}
          aria-label={color}
          title={color}
          onclick={() => setTabGroupColor(group.id, color)}
        ></button>
      {/each}
    </div>
  </div>
  <div class="menu-separator"></div>
  <button type="button" class="menu-item" onclick={() => run(() => toggleTabGroupCollapsed(group.id))}>
    <Layers size={14} />
    {group.collapsed ? "Expand group" : "Collapse group"}
  </button>
  <button type="button" class="menu-item" onclick={() => run(() => ungroupTabGroup(group.id))}>
    <Ungroup size={14} />
    Ungroup
  </button>
  {#if count > 0}
    <button type="button" class="menu-item menu-item-danger" onclick={() => run(() => closeTabGroup(group.id))}>
      <X size={14} />
      Close {count} {count === 1 ? "session" : "sessions"}
    </button>
  {/if}
</FloatingMenu>

<style>
  .tab-group-swatch {
    width: 1.25rem;
    height: 1.25rem;
    border-radius: 9999px;
    background: var(--group-color);
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.2);
  }
  .tab-group-swatch[aria-checked="true"] {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
