<script lang="ts">
  import { Users } from "lucide-svelte";
  import type { SourceFilterValue, SourceInfo } from "./SourceBadge.svelte";

  interface Props {
    /** Sources to offer, in display order (Local and All are added here). */
    sources: SourceInfo[];
    value: SourceFilterValue;
    onChange: (value: SourceFilterValue) => void;
    /** Item count per filter value ("all", "local", source id), when known. */
    counts?: Record<string, number>;
  }

  let { sources, value, onChange, counts }: Props = $props();

  const options = $derived([
    { value: "all", label: "All", team: false },
    { value: "local", label: "Local", team: false },
    ...sources.map((s) => ({ value: s.id, label: s.name, team: s.kind === "team" })),
  ]);
</script>

<div class="flex min-w-0 items-center gap-1.5 overflow-x-auto scrollbar-none" role="group" aria-label="Filter by source">
  <span class="section-label mr-1 shrink-0" aria-hidden="true">Source</span>
  {#each options as opt (opt.value)}
    {@const active = value === opt.value}
    <button
      type="button"
      class="chip {active ? 'chip-active' : ''}"
      onclick={() => onChange(active && opt.value !== "all" ? "all" : opt.value)}
      aria-pressed={active}
      title={opt.team ? `Hosts shared in the team ${opt.label}` : undefined}
    >
      {#if opt.team}
        <Users size={12} aria-hidden="true" />
        <span class="sr-only">Team</span>
      {/if}
      {opt.label}
      {#if counts && counts[opt.value] !== undefined}
        <span class="tabular-nums text-muted">{counts[opt.value]}</span>
      {/if}
    </button>
  {/each}
</div>
