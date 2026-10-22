<script lang="ts">
  import { X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import {
    IS_MAC,
    GLOBAL_SHORTCUTS,
    HOST_SHORTCUTS,
    TERMINAL_SHORTCUTS,
    VIEW_ORDER,
    viewDigit,
    type Shortcut,
  } from "$lib/shortcuts";
  import { VIEW_META } from "$lib/viewMeta";

  interface Props {
    show: boolean;
    onClose: () => void;
  }

  let { show, onClose }: Props = $props();

  const viewRows: Shortcut[] = VIEW_ORDER.flatMap((tab) => {
    const digit = viewDigit(tab);
    if (!digit || tab === "settings") return [];
    return [{ id: `view-${tab}`, label: VIEW_META[tab].label, combos: [[digit]], scope: "global" as const }];
  });

  type Section = { title: string; note?: string; items: Shortcut[] };

  const general = GLOBAL_SHORTCUTS.filter((s) => s.id === "palette" || s.id === "palette-terminal" || s.id === "settings");
  const hostsGlobal = GLOBAL_SHORTCUTS.filter((s) => s.id === "filter" || s.id === "new-host");

  const columns: Section[][] = [
    [
      { title: "General", items: general },
      { title: "Views", items: viewRows },
    ],
    [
      { title: "Hosts", items: [...hostsGlobal, ...HOST_SHORTCUTS] },
      {
        title: "Terminal",
        note: IS_MAC
          ? "Terminal shortcuts use Cmd. Turn on Smart copy in Settings to copy while text is selected."
          : "Plain Ctrl+C, Ctrl+V, Ctrl+F and Ctrl+K go to the shell. Turn on Smart copy in Settings to copy with Ctrl+C while text is selected.",
        items: TERMINAL_SHORTCUTS,
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
                {#each section.items as item (item.id)}
                  <li class="flex h-8 items-center justify-between gap-4 border-b border-border-subtle last:border-b-0">
                    <span class="truncate text-sm text-secondary">{item.label}</span>
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
