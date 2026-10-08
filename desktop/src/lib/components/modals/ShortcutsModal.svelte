<script lang="ts">
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";

  interface Props {
    show: boolean;
    onClose: () => void;
  }

  let { show, onClose }: Props = $props();

  const MOD =
    typeof navigator !== "undefined" && /mac/i.test(navigator.platform || navigator.userAgent) ? "⌘" : "Ctrl";

  /** Each shortcut lists alternative key combos; each combo is a list of keys. */
  type Shortcut = { action: string; combos: string[][] };
  type Section = { title: string; note?: string; items: Shortcut[] };

  // Mirrors +page.svelte (palette / help), appState.handleGlobalKeydown
  // (filter, new host, views, terminal font) and ConnectionsView (host list).
  const columns: Section[][] = [
    [
      {
        title: "General",
        items: [
          { action: "Command palette", combos: [[MOD, "K"]] },
          { action: "Keyboard shortcuts", combos: [["?"], ["F1"], [MOD, "/"]] },
          { action: "Close dialog or clear filter", combos: [["Esc"]] },
        ],
      },
      {
        title: "Views",
        items: [
          { action: "Hosts", combos: [["1"]] },
          { action: "Terminals", combos: [["2"]] },
          { action: "Keys", combos: [["3"]] },
          { action: "Security audit", combos: [["4"]] },
          { action: "History", combos: [["5"]] },
          { action: "Settings", combos: [["6"]] },
        ],
      },
    ],
    [
      {
        title: "Hosts",
        items: [
          { action: "Focus filter", combos: [["/"]] },
          { action: "New host", combos: [["N"], [MOD, "N"]] },
          { action: "Move selection", combos: [["↑", "↓"], ["J", "K"]] },
          { action: "Connect to selected host", combos: [["Enter"]] },
          { action: "Edit selected host", combos: [[MOD, "E"]] },
        ],
      },
      {
        title: "Terminal",
        note: "In the Terminals view, when the terminal isn't focused.",
        items: [
          { action: "Increase font size", combos: [[MOD, "+"]] },
          { action: "Decrease font size", combos: [[MOD, "−"]] },
          { action: "Reset font size", combos: [[MOD, "0"]] },
        ],
      },
    ],
  ];
</script>

{#if show}
  <ModalShell open={show} title="Keyboard shortcuts" {onClose} width="lg">
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Keyboard shortcuts</h2>
        <p class="modal-subtitle">Single-key shortcuts work when no text field is focused.</p>
      </div>
      <button type="button" class="modal-close" onclick={onClose} aria-label="Close">
        <X size={16} />
      </button>
    </div>

    <div class="modal-body grid grid-cols-1 gap-x-8 gap-y-5 sm:grid-cols-2">
      {#each columns as column, ci (ci)}
        <div class="flex min-w-0 flex-col gap-5">
          {#each column as section (section.title)}
            <section class="flex flex-col gap-1">
              <h3 class="section-label m-0 mb-1">{section.title}</h3>
              <ul class="m-0 flex list-none flex-col p-0">
                {#each section.items as item (item.action)}
                  <li class="flex h-8 items-center justify-between gap-4 border-b border-border-subtle last:border-b-0">
                    <span class="truncate text-sm text-secondary">{item.action}</span>
                    <span class="flex shrink-0 items-center gap-1.5">
                      {#each item.combos as combo, i (i)}
                        {#if i > 0}<span class="text-xs text-muted">or</span>{/if}
                        <span class="flex items-center gap-0.5">
                          {#each combo as key, ki (ki)}
                            <kbd class="kbd">{key}</kbd>
                          {/each}
                        </span>
                      {/each}
                    </span>
                  </li>
                {/each}
              </ul>
              {#if section.note}
                <p class="m-0 mt-1 text-xs text-muted">{section.note}</p>
              {/if}
            </section>
          {/each}
        </div>
      {/each}
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-secondary" onclick={onClose} data-autofocus>Close</button>
    </div>
  </ModalShell>
{/if}
