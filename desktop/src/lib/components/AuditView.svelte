<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { Check, Copy, RefreshCw, ShieldAlert, ShieldCheck, Wrench } from "lucide-svelte";
  import type { AuditFinding, AuditReport } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { copyTextWithFallback } from "$lib/utils/terminal-xterm";

  type Severity = AuditFinding["severity"];
  type Filter = "all" | Severity;

  const SEVERITY: Record<Severity, { label: string; badge: string; rank: number }> = {
    critical: { label: "Critical", badge: "badge-error", rank: 0 },
    warning: { label: "Warning", badge: "badge-warning", rank: 1 },
    info: { label: "Info", badge: "badge-neutral", rank: 2 },
  };

  let report = $state.raw<AuditReport | null>(null);
  let loading = $state(true);
  let loadError = $state<string | null>(null);
  let fixing = $state(false);
  let activeFilter = $state<Filter>("all");
  let copiedRemediation = $state<string | null>(null);

  async function runAudit() {
    loading = true;
    try {
      report = await invoke<AuditReport>("run_security_audit");
      loadError = null;
    } catch (err) {
      loadError = String(err);
      notify(`Security audit failed: ${err}`, "error");
    } finally {
      loading = false;
    }
  }

  async function fixPermissions() {
    fixing = true;
    try {
      const repaired = await invoke<number>("fix_security_permissions");
      notify(
        repaired > 0
          ? `Fixed permissions on ${repaired} ${repaired === 1 ? "file" : "files"}`
          : "Permissions were already correct",
        "success",
      );
      await runAudit();
    } catch (err) {
      notify(`Failed to fix permissions: ${err}`, "error");
    } finally {
      fixing = false;
    }
  }

  function copyCode(code: string) {
    copyTextWithFallback(code);
    copiedRemediation = code;
    notify("Copied command", "success");
    setTimeout(() => {
      if (copiedRemediation === code) copiedRemediation = null;
    }, 2000);
  }

  onMount(() => {
    runAudit();
  });

  // Most severe first; stable within a severity.
  const sortedFindings = $derived(
    report ? [...report.findings].sort((a, b) => SEVERITY[a.severity].rank - SEVERITY[b.severity].rank) : [],
  );
  const filteredFindings = $derived(
    activeFilter === "all" ? sortedFindings : sortedFindings.filter((f) => f.severity === activeFilter),
  );
  const filters = $derived<{ id: Filter; label: string; count: number }[]>([
    { id: "all", label: "All", count: report?.findings.length ?? 0 },
    { id: "critical", label: "Critical", count: report?.total_critical ?? 0 },
    { id: "warning", label: "Warnings", count: report?.total_warning ?? 0 },
    { id: "info", label: "Info", count: report?.total_info ?? 0 },
  ]);

  function gradeColor(grade: string): string {
    if (grade.startsWith("A")) return "text-success";
    if (grade.startsWith("B")) return "text-primary";
    if (grade.startsWith("C")) return "text-warning";
    return "text-error";
  }
</script>

<div class="view select-none">
  <header class="view-header">
    <div class="flex min-w-0 items-baseline gap-2.5">
      <h1 class="view-title">Security audit</h1>
      {#if report}
        <span class="text-sm tabular-nums text-muted">
          {report.findings.length} {report.findings.length === 1 ? "finding" : "findings"}
        </span>
      {/if}
    </div>
    <div class="view-actions">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={fixPermissions}
        disabled={loading || fixing}
        title="Set secure modes on ~/.ssh and its key files"
      >
        {#if fixing}<span class="spinner size-3"></span>{:else}<Wrench size={14} />{/if}
        Fix permissions
      </button>
      <button type="button" class="btn btn-ghost" onclick={runAudit} disabled={loading}>
        <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
        Re-scan
      </button>
    </div>
  </header>

  {#if report}
    <div class="view-toolbar">
      <div class="segmented" role="group" aria-label="Filter by severity">
        {#each filters as f (f.id)}
          <button
            type="button"
            class="segmented-item {activeFilter === f.id ? 'segmented-item-active' : ''}"
            onclick={() => (activeFilter = f.id)}
            aria-pressed={activeFilter === f.id}
          >
            {f.label}
            <span class="tabular-nums text-muted">{f.count}</span>
          </button>
        {/each}
      </div>
    </div>
  {:else if loading}
    <div class="view-toolbar" aria-hidden="true">
      <div class="skeleton h-8 w-64 rounded-md"></div>
    </div>
  {/if}

  <div class="view-body">
    {#if !report && loading}
      <div class="flex flex-col gap-3" aria-busy="true" aria-label="Running security audit">
        <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
          {#each [0, 1, 2, 3] as i (i)}
            <div class="stat">
              <div class="skeleton h-3 w-16"></div>
              <div class="skeleton mt-1 h-6 w-12"></div>
            </div>
          {/each}
        </div>
        {#each [0, 1, 2] as i (i)}
          <div class="panel flex flex-col gap-2.5 p-4">
            <div class="flex items-center gap-2">
              <div class="skeleton h-5 w-14"></div>
              <div class="skeleton h-3.5 w-56"></div>
            </div>
            <div class="skeleton h-3 w-3/4"></div>
            <div class="skeleton h-8 w-full"></div>
          </div>
        {/each}
      </div>
    {:else if !report}
      <div class="empty-state h-full">
        <div class="empty-state-icon"><ShieldAlert size={18} /></div>
        <div class="empty-state-title">The audit could not run</div>
        <p class="empty-state-desc">{loadError ?? "Something went wrong while inspecting ~/.ssh."}</p>
        <div class="empty-state-action">
          <button type="button" class="btn btn-secondary" onclick={runAudit}>
            <RefreshCw size={14} />
            Try again
          </button>
        </div>
      </div>
    {:else}
      <div class="flex flex-col gap-3">
        <div class="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <div class="stat">
            <div class="flex items-center justify-between gap-2">
              <span class="stat-label">Score</span>
              <span class="truncate text-xs text-muted">
                Grade <span class="font-semibold {gradeColor(report.grade)}">{report.grade}</span> · {report.rating}
              </span>
            </div>
            <span class="stat-value">
              {report.score}<span class="text-sm font-normal text-muted">/100</span>
            </span>
          </div>
          {#each [
            { label: "Critical", value: report.total_critical, dot: "status-dot-error", color: "text-error" },
            { label: "Warnings", value: report.total_warning, dot: "status-dot-warning", color: "text-warning" },
            { label: "Info", value: report.total_info, dot: "status-dot-offline", color: "text-primary" },
          ] as s (s.label)}
            <div class="stat">
              <span class="stat-label flex items-center gap-1.5">
                <span class="status-dot status-dot-sm {s.value > 0 ? s.dot : 'status-dot-offline'}"></span>
                {s.label}
              </span>
              <span class="stat-value {s.value > 0 ? s.color : ''}">{s.value}</span>
            </div>
          {/each}
        </div>

        {#if filteredFindings.length === 0}
          <div class="empty-state">
            <div class="empty-state-icon"><ShieldCheck size={18} /></div>
            {#if report.findings.length === 0}
              <div class="empty-state-title">No issues found</div>
              <p class="empty-state-desc">Your SSH files and configuration passed every check.</p>
            {:else}
              <div class="empty-state-title">Nothing at this severity</div>
              <p class="empty-state-desc">Pick another filter to see the remaining findings.</p>
            {/if}
          </div>
        {:else}
          <ul class="m-0 flex list-none flex-col gap-2 p-0">
            {#each filteredFindings as finding, i (`${finding.severity}:${finding.title}:${i}`)}
              {@const sev = SEVERITY[finding.severity]}
              <li class="panel flex flex-col gap-2 p-4">
                <div class="flex min-w-0 items-center gap-2.5">
                  <span class="badge {sev.badge}">{sev.label}</span>
                  <h2 class="m-0 truncate text-sm font-medium text-primary" title={finding.title}>{finding.title}</h2>
                </div>
                {#if finding.description}
                  <p class="m-0 text-sm leading-relaxed text-secondary select-text">{finding.description}</p>
                {/if}
                {#if finding.remediation}
                  <div class="code-block flex items-center gap-2 overflow-hidden py-1.5 pr-1.5">
                    <code class="min-w-0 flex-1 overflow-x-auto whitespace-pre select-text" title={finding.remediation}>{finding.remediation}</code>
                    <button
                      type="button"
                      class="btn-icon btn-icon-sm"
                      onclick={() => copyCode(finding.remediation)}
                      aria-label="Copy command"
                      title="Copy command"
                    >
                      {#if copiedRemediation === finding.remediation}
                        <Check size={13} class="text-success" />
                      {:else}
                        <Copy size={13} />
                      {/if}
                    </button>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>
</div>
