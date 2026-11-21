<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { CircleCheck, CircleX, Download, RefreshCw, ShieldAlert, ShieldCheck } from "lucide-svelte";
  import CustomSelect from "$lib/components/ui/CustomSelect.svelte";
  import type { Connection, TeamAudit, TeamMember, TeamSummary } from "$lib/types";
  import { raiseAttention, resolveAttention, TEAM_ATTENTION } from "$lib/stores/attention.svelte";
  import {
    AUDIT_CHECK_LABEL,
    AUDIT_EVENT_LABEL,
    auditCsv,
    auditVerdict,
    downloadText,
    formatUtc,
    plural,
  } from "./teams";

  let {
    team,
    members,
    teamConnections,
  }: { team: TeamSummary; members: TeamMember[] | null; teamConnections: Connection[] } = $props();

  let member = $state("");
  let host = $state("");
  let since = $state("");
  let log = $state.raw<TeamAudit | null>(null);
  let verifiedAt = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const verdict = $derived(log ? auditVerdict(log) : null);
  const filtered = $derived(!!(member || host || since));
  const memberOptions = $derived([
    { value: "", label: "Everyone" },
    ...(members ?? []).map((m) => ({ value: m.email, label: m.email })),
  ]);
  const hostOptions = $derived([
    { value: "", label: "Every host" },
    ...teamConnections.map((c) => ({ value: c.name, label: c.name })),
  ]);
  const cliLine = $derived(
    [
      `bssh teams audit ${team.name}`,
      host && `--host ${host}`,
      member && `--member ${member}`,
      since && `--since ${since}`,
    ]
      .filter(Boolean)
      .join(" "),
  );

  /** Verify on open and whenever a filter changes; the newest request wins. */
  let requestSeq = 0;
  async function verify() {
    const seq = ++requestSeq;
    busy = true;
    error = null;
    try {
      const result = await invoke<TeamAudit>("teams_audit", {
        team: team.team_id,
        host: host || null,
        member: member || null,
        since: since ? `${since}T00:00:00Z` : null,
      });
      if (seq !== requestSeq) return;
      log = result;
      verifiedAt = new Date().toISOString();
      const id = TEAM_ATTENTION.auditFailed(team.team_id);
      if (!result.clean) {
        raiseAttention({
          id,
          severity: "error",
          title: `${team.name}: audit log failed verification`,
          body: "Treat this log as tampered until you've checked it.",
          action: { label: "Open audit log", tab: "teams", teamId: team.team_id },
        });
      } else {
        // The whole log is verified whatever the filters show, so any clean
        // run clears a failure found earlier.
        resolveAttention(id);
      }
    } catch (err) {
      if (seq === requestSeq) error = String(err);
    } finally {
      if (seq === requestSeq) busy = false;
    }
  }

  $effect(() => {
    void [member, host, since];
    void verify();
  });

  function exportCsv() {
    if (!log) return;
    downloadText(`bssh-teams-audit-${team.name}-${new Date().toISOString().slice(0, 10)}.csv`, auditCsv(log));
  }
</script>

<div class="flex flex-col gap-3">
  <div class="flex flex-wrap items-end gap-2">
    <div class="field">
      <label class="field-label" for="audit-member">Member</label>
      <CustomSelect id="audit-member" ariaLabel="Member" class="w-56" options={memberOptions} bind:value={member} />
    </div>
    <div class="field">
      <label class="field-label" for="audit-host">Host</label>
      <CustomSelect id="audit-host" ariaLabel="Host" class="w-56" options={hostOptions} bind:value={host} />
    </div>
    <div class="field">
      <label class="field-label" for="audit-since">Since (UTC)</label>
      <input id="audit-since" class="input w-40" type="date" bind:value={since} />
    </div>
    <div class="ml-auto flex gap-2">
      <button type="button" class="btn btn-secondary" onclick={verify} disabled={busy}>
        <RefreshCw size={14} class={busy ? "animate-spin" : ""} />
        Verify again
      </button>
      <button type="button" class="btn btn-secondary" onclick={exportCsv} disabled={!log || log.lines.length === 0}>
        <Download size={14} />
        Export CSV
      </button>
    </div>
  </div>

  {#if error}
    <div class="alert alert-error" role="alert">Couldn't load the audit log: {error}</div>
  {:else if !log}
    <div class="skeleton h-14 w-full rounded-md" aria-busy="true" aria-label="Verifying the audit log"></div>
  {:else}
    {#if verdict === "verified"}
      <div class="alert alert-success" role="status">
        <ShieldCheck size={16} class="mt-0.5 shrink-0" />
        <div class="flex flex-col gap-0.5">
          <span class="alert-title">Verified on this device{verifiedAt ? `, ${formatUtc(verifiedAt)}` : ""}</span>
          <span class="text-secondary">
            {plural(log.verified_events, "event")}, {plural(log.chains, "device chain")}, the whole log, complete from
            each device's first event.{log.complete
              ? ""
              : ` The list shows only the newest ${plural(log.lines.length, "event")}; the rest were verified too.`}
          </span>
        </div>
      </div>
    {:else}
      <div class="alert alert-error" role="alert">
        <ShieldAlert size={16} class="mt-0.5 shrink-0" />
        <div class="flex flex-col gap-1">
          <span class="alert-title">Verification failed: treat this log as tampered</span>
          <span>
            {plural(log.invalid_events, "event")} failed the check · {plural(log.problems.length, "chain problem")}.
          </span>
          {#if log.problems.length}
            <ul class="m-0 list-disc pl-5">
              {#each log.problems as problem (problem)}<li>{problem}</li>{/each}
            </ul>
          {/if}
          <span>
            Don't trust the rows below without checking. Run <code>bssh teams audit {team.name}</code> on another admin
            machine and compare.
          </span>
        </div>
      </div>
    {/if}

    {#if log.lines.length === 0}
      <p class="m-0 type-meta">
        {filtered ? "No events match these filters." : "No events yet. Connections show up here after members sync."}
      </p>
    {:else}
      <div class="table-wrap overflow-x-auto">
        <table class="data-table">
          <thead>
            <tr>
              <th>Check</th>
              <th>Time (UTC)</th>
              <th>Member · device</th>
              <th>Event</th>
              <th>Host</th>
              <th>From</th>
            </tr>
          </thead>
          <tbody>
            {#each log.lines as line, i (i)}
              {@const ok = line.check === "valid"}
              <tr>
                <td class:text-error={!ok}>
                  <span class="inline-flex items-center gap-1 whitespace-nowrap {ok ? 'text-success' : ''}">
                    {#if ok}<CircleCheck size={13} />{:else}<CircleX size={13} />{/if}
                    {AUDIT_CHECK_LABEL[line.check]}
                  </span>
                </td>
                <td class="whitespace-nowrap font-mono text-xs" title="Received {formatUtc(line.received_at, true)}">
                  {formatUtc(line.occurred_at).replace(" UTC", "")}
                </td>
                <td>{line.who}{line.device_name ? ` · ${line.device_name}` : ""}</td>
                <td>{AUDIT_EVENT_LABEL[line.kind]}</td>
                <td>{line.host ?? "–"}</td>
                <td class="font-mono text-xs">{line.client_ip ?? "–"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  {/if}
  <p class="m-0 type-meta">
    Every event is verified here, not trusted from the server. CLI: <code class="select-text">{cliLine}</code>
  </p>
</div>
