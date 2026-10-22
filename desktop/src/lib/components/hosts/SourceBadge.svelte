<script lang="ts" module>
  import { invoke } from "@tauri-apps/api/core";
  import type { Source, SourceKind, TeamsStatus } from "$lib/types";

  /** Where a host comes from: null for hosts added here (local). */
  export interface SourceInfo {
    id: string;
    /** Source name; for a team source, the team name. */
    name: string;
    kind: SourceKind;
    /** Set for team sources. */
    teamId?: string;
  }

  /** "all", "local", or a source id. */
  export type SourceFilterValue = string;

  /**
   * Inventory sources keyed by id, with team names and ids from
   * `teams_status` (local only, works while Teams is locked).
   */
  export async function loadSourceIndex(): Promise<Record<string, SourceInfo>> {
    const sources = (await invoke<Source[] | null>("list_sources").catch(() => null)) ?? [];
    const index: Record<string, SourceInfo> = {};
    for (const s of sources) index[s.id] = { id: s.id, name: s.name, kind: s.kind };
    if (sources.some((s) => s.kind === "team")) {
      const status = await invoke<TeamsStatus | null>("teams_status").catch(() => null);
      for (const t of status?.teams ?? []) {
        index[t.source_id] = { id: t.source_id, name: t.name, kind: "team", teamId: t.team_id };
      }
    }
    return index;
  }

  /** The source of a host, or null when it is local (or its source is unknown). */
  export function sourceOf(
    sourceId: string | undefined | null,
    index: Record<string, SourceInfo>,
  ): SourceInfo | null {
    if (!sourceId) return null;
    return index[sourceId] ?? { id: sourceId, name: "Synced", kind: "ssh-config" };
  }

  export function matchesSource(
    sourceId: string | undefined | null,
    filter: SourceFilterValue,
  ): boolean {
    if (filter === "all") return true;
    if (filter === "local") return !sourceId;
    return sourceId === filter;
  }
</script>

<script lang="ts">
  import { RefreshCw, Users } from "lucide-svelte";

  interface Props {
    source: SourceInfo | null;
    /** Extra text for the tooltip (e.g. the external id of a synced host). */
    detail?: string;
  }

  let { source, detail }: Props = $props();
</script>

{#if !source}
  <span class="text-xs text-muted">Local</span>
{:else if source.kind === "team"}
  <span
    class="badge badge-neutral max-w-full"
    title={`Team ${source.name}: shared with bssh Teams; connecting is recorded in the team's signed audit log${detail ? ` · ${detail}` : ""}`}
  >
    <Users size={11} class="shrink-0" aria-hidden="true" />
    <span class="sr-only">Team</span>
    <span class="truncate">{source.name}</span>
  </span>
{:else}
  <span
    class="badge badge-neutral max-w-full"
    title={`Synced from ${source.name}${detail ? ` · ${detail}` : ""}`}
  >
    <RefreshCw size={10} class="shrink-0" aria-hidden="true" />
    <span class="sr-only">Synced from</span>
    <span class="truncate">{source.name}</span>
  </span>
{/if}
