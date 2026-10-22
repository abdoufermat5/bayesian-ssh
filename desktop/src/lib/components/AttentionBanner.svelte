<script lang="ts">
  import { AlertCircle, AlertTriangle, ChevronDown, ChevronRight, Info, X } from "lucide-svelte";
  import {
    dismissAttention,
    getAttention,
    requestTeamFocus,
    type AttentionItem,
  } from "$lib/stores/attention.svelte";
  import type { AppTab } from "$lib/types";

  interface Props {
    onTabChange: (tab: AppTab) => void;
  }

  let { onTabChange }: Props = $props();

  let expanded = $state<Record<string, boolean>>({});

  function act(item: AttentionItem) {
    const action = item.action;
    if (!action) return;
    if (action.teamId) requestTeamFocus(action.teamId);
    onTabChange(action.tab);
  }

  const tone = {
    error: { cls: "text-error", label: "Error" },
    warning: { cls: "text-warning", label: "Warning" },
    info: { cls: "text-accent", label: "Notice" },
  } as const;
</script>

{#if getAttention().length > 0}
  <div class="flex flex-col gap-px border-b border-border bg-chrome" aria-label="Needs attention">
    {#each getAttention() as item (item.id)}
      {@const t = tone[item.severity]}
      <div
        class="flex flex-col px-4 py-1.5 text-sm"
        role={item.severity === "error" ? "alert" : "status"}
      >
        <div class="flex min-h-7 items-center gap-2">
          <span class="flex shrink-0 {t.cls}">
            {#if item.severity === "error"}
              <AlertCircle size={15} />
            {:else if item.severity === "warning"}
              <AlertTriangle size={15} />
            {:else}
              <Info size={15} />
            {/if}
            <span class="sr-only">{t.label}:</span>
          </span>
          <span class="min-w-0 flex-1 truncate text-primary" title={item.title}>{item.title}</span>
          {#if item.body}
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              aria-expanded={!!expanded[item.id]}
              aria-label={expanded[item.id] ? "Hide details" : "Show details"}
              onclick={() => (expanded[item.id] = !expanded[item.id])}
            >
              {#if expanded[item.id]}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
            </button>
          {/if}
          {#if item.action}
            <button type="button" class="btn btn-secondary btn-sm" onclick={() => act(item)}>
              {item.action.label}
            </button>
          {/if}
          {#if item.dismissible}
            <button
              type="button"
              class="btn-icon btn-icon-sm"
              aria-label="Dismiss"
              onclick={() => dismissAttention(item.id)}
            >
              <X size={14} />
            </button>
          {/if}
        </div>
        {#if item.body && expanded[item.id]}
          <p class="pb-1 pl-[23px] text-xs leading-snug text-secondary">{item.body}</p>
        {/if}
      </div>
    {/each}
  </div>
{/if}
