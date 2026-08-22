<script lang="ts">
  import { Command, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    show: boolean;
    onClose: () => void;
  }

  let { show, onClose }: Props = $props();

  const shortcuts = [
    { key: "/  or  Ctrl+K", description: "Focus search bar" },
    { key: "N  or  Ctrl+N", description: "Open Add Connection modal" },
    { key: "1", description: "Switch to Hosts / Connections tab" },
    { key: "2", description: "Switch to Terminals tab" },
    { key: "3", description: "Switch to Keys tab" },
    { key: "4", description: "Switch to Security Audit tab" },
    { key: "5", description: "Switch to History / Logs tab" },
    { key: "6", description: "Switch to Settings tab" },
    { key: "↑ / ↓", description: "Navigate host list" },
    { key: "Enter", description: "Connect to selected host" },
    { key: "Ctrl + E", description: "Edit selected host" },
    { key: "Escape", description: "Close active modal / Clear search" },
    { key: "?  or  F1", description: "Toggle keyboard shortcuts guide" },
  ];
</script>

{#if show}
  <ModalShell
    open={show}
    title="Keyboard Shortcuts"
    onClose={onClose}
    width="md"
    panelClass="p-6 space-y-5"
  >
    <!-- Header -->
    <div class="flex items-center justify-between pb-3 border-b border-border">
        <h2 class="text-base font-bold text-primary flex items-center gap-2 m-0">
          <Command size={18} class="text-accent" />
          Keyboard Shortcuts & Productivity
        </h2>
        <button
          type="button"
          class="btn-icon"
          onclick={onClose}
          title="Close"
          aria-label="Close"
        >
          <X size={16} />
        </button>
      </div>

      <!-- Shortcuts Table -->
      <div class="space-y-2 max-h-80 overflow-y-auto pr-1">
        {#each shortcuts as shortcut}
          <div class="flex items-center justify-between gap-4 p-2 rounded-lg bg-surface-input/60 border border-border/50 text-xs">
            <span class="text-secondary font-medium">{shortcut.description}</span>
            <kbd class="kbd kbd-accent shrink-0">{shortcut.key}</kbd>
          </div>
        {/each}
      </div>

      <!-- Footer -->
      <div class="pt-2 border-t border-border flex justify-between items-center text-[11px] text-muted">
        <span>Press <kbd class="kbd">Esc</kbd> to close</span>
        <button
          type="button"
          class="btn btn-primary"
          onclick={onClose}
        >
          Got it
        </button>
      </div>
  </ModalShell>
{/if}
