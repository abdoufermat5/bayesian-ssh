<script lang="ts">
  import { Hourglass, UserX } from "lucide-svelte";
  import type { TeamSummary } from "$lib/types";
  import Fingerprint from "./Fingerprint.svelte";

  /** Persistent per-team states that need someone else: shown until they clear. */
  let {
    team,
    waitingForKey,
    removed,
    myFingerprint,
    showName = false,
  }: {
    team: TeamSummary;
    waitingForKey: boolean;
    removed: boolean;
    myFingerprint: string | undefined;
    /** Prefix the team name (on the Teams home, where several teams are listed). */
    showName?: boolean;
  } = $props();
</script>

{#if removed}
  <div class="alert alert-warning" role="status">
    <UserX size={16} class="mt-0.5 shrink-0" />
    <div class="flex flex-col gap-0.5">
      <span class="alert-title">{showName ? `${team.name}: you` : "You"} were removed from this team</span>
      <span class="text-secondary">
        This device can't sync it anymore. Ask an admin of {team.name} if that's a mistake.
      </span>
    </div>
  </div>
{:else if waitingForKey}
  <div class="alert alert-info" role="status">
    <Hourglass size={16} class="mt-0.5 shrink-0 text-accent" />
    <div class="flex min-w-0 flex-col gap-1.5">
      <span class="alert-title">
        {showName ? `${team.name}: waiting` : "Waiting"} for an admin to grant this device the key.
      </span>
      {#if myFingerprint}
        <span>Send them your fingerprint:</span>
        <Fingerprint value={myFingerprint} label="your fingerprint" />
      {/if}
    </div>
  </div>
{/if}
