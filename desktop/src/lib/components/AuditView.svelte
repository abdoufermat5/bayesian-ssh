<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import {
    AlertTriangle,
    Check,
    CheckCircle2,
    Copy,
    Info,
    RefreshCw,
    Shield,
    ShieldAlert,
    ShieldCheck,
    Wrench,
  } from "lucide-svelte";
  import type { AuditFinding, AuditReport } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";

  let report = $state<AuditReport | null>(null);
  let loading = $state(true);
  let fixing = $state(false);
  let activeFilter = $state<"all" | "critical" | "warning" | "info">("all");
  let copiedRemediation = $state<string | null>(null);

  async function runAudit() {
    loading = true;
    try {
      report = await invoke<AuditReport>("run_security_audit");
    } catch (err) {
      notify(`Failed to run security audit: ${err}`, "error");
    } finally {
      loading = false;
    }
  }

  async function fixPermissions() {
    fixing = true;
    try {
      const repaired = await invoke<number>("fix_security_permissions");
      notify(`Successfully repaired permissions for ${repaired} item(s)!`, "success");
      await runAudit();
    } catch (err) {
      notify(`Failed to fix permissions: ${err}`, "error");
    } finally {
      fixing = false;
    }
  }

  function copyCode(code: string) {
    navigator.clipboard.writeText(code);
    copiedRemediation = code;
    notify("Copied remediation command", "success");
    setTimeout(() => {
      if (copiedRemediation === code) copiedRemediation = null;
    }, 2000);
  }

  onMount(() => {
    runAudit();
  });

  const filteredFindings = $derived(
    report?.findings.filter((f) => activeFilter === "all" || f.severity === activeFilter) ?? []
  );

  function getGradeClass(grade: string) {
    if (grade.startsWith("A")) return "text-running border-border-strong bg-surface-elevated shadow-sm";
    if (grade.startsWith("B")) return "text-accent border-border-strong bg-surface-elevated shadow-sm";
    if (grade.startsWith("C")) return "text-warning border-border-strong bg-surface-elevated shadow-sm";
    return "text-error border-border-strong bg-surface-elevated shadow-sm";
  }

  function getSeverityBadge(severity: AuditFinding["severity"]) {
    switch (severity) {
      case "critical":
        return { label: "CRITICAL", class: "badge-error", icon: ShieldAlert };
      case "warning":
        return { label: "WARNING", class: "badge-warning", icon: AlertTriangle };
      case "info":
        return { label: "INFO", class: "badge-subtle", icon: Info };
    }
  }
</script>

<div class="flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-surface text-[13px] text-primary select-none">
  <div class="view-header">
    <div class="flex min-w-0 items-center gap-3">
      <div class="icon-tile rounded-md">
        <ShieldCheck size={16} />
      </div>
      <div class="min-w-0">
        <h2 class="m-0 flex items-center gap-2 truncate text-sm font-bold tracking-tight text-primary">
          Security Audit
          {#if report}
            <span class="badge {report.total_critical > 0 ? 'badge-error' : report.total_warning > 0 ? 'badge-warning' : 'badge-success'}">
              {report.grade} · {report.score}/100
            </span>
          {/if}
        </h2>
        <p class="m-0 truncate text-xs text-muted">SSH identity permissions and host-key posture</p>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <button
        type="button"
        class="btn btn-secondary"
        onclick={fixPermissions}
        disabled={loading || fixing}
        title="Repair insecure SSH file permissions"
      >
        <Wrench size={13} class={fixing ? "animate-spin" : ""} />
        <span class="hidden sm:inline">Fix All Permissions</span>
        <span class="sm:hidden">Fix</span>
      </button>

      <button
        type="button"
        class="btn btn-secondary"
        onclick={runAudit}
        disabled={loading}
      >
        <RefreshCw size={13} class={loading ? "animate-spin" : ""} />
        <span class="hidden sm:inline">Re-scan</span>
      </button>
    </div>
  </div>

  <div class="view-content">
    {#if loading}
      <div class="empty-state">
        <RefreshCw size={24} class="animate-spin text-accent" />
        <span class="text-xs font-semibold text-primary">Auditing system security posture...</span>
        <span class="text-xs text-muted">Inspecting identity files, permissions, and SSH configuration</span>
      </div>
    {:else if report}
      <div class="panel mb-4 flex flex-wrap items-center justify-between gap-3 px-4 py-3">
        <div class="flex min-w-0 items-center gap-3">
          <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-md border font-mono text-lg font-black {getGradeClass(report.grade)}">
            {report.grade}
          </div>
          <div class="min-w-0">
            <h3 class="m-0 truncate text-sm font-semibold text-primary">{report.rating}</h3>
            <p class="m-0 font-mono text-xs text-muted">{report.score}/100 score</p>
          </div>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <span class="badge {report.total_critical > 0 ? 'badge-error' : 'badge-subtle'}">
            <ShieldAlert size={11} /> {report.total_critical} critical
          </span>
          <span class="badge {report.total_warning > 0 ? 'badge-warning' : 'badge-subtle'}">
            <AlertTriangle size={11} /> {report.total_warning} warnings
          </span>
          <span class="badge badge-subtle">
            <CheckCircle2 size={11} /> {report.total_info} info
          </span>
        </div>
      </div>

      <div class="mb-3 flex flex-wrap items-center gap-1.5 border-b border-border/80 pb-2">
        <button
          type="button"
          class="filter-chip {activeFilter === 'all' ? 'filter-chip-active' : 'filter-chip-idle'}"
          onclick={() => (activeFilter = "all")}
        >
          All ({report.findings.length})
        </button>

        {#if report.total_critical > 0}
          <button
            type="button"
            class="filter-chip {activeFilter === 'critical' ? 'border-error/40 bg-error/15 text-error' : 'filter-chip-idle'}"
            onclick={() => (activeFilter = "critical")}
          >
            Critical ({report.total_critical})
          </button>
        {/if}

        {#if report.total_warning > 0}
          <button
            type="button"
            class="filter-chip {activeFilter === 'warning' ? 'border-warning/40 bg-warning/15 text-warning' : 'filter-chip-idle'}"
            onclick={() => (activeFilter = "warning")}
          >
            Warnings ({report.total_warning})
          </button>
        {/if}

        {#if report.total_info > 0}
          <button
            type="button"
            class="filter-chip {activeFilter === 'info' ? 'filter-chip-active' : 'filter-chip-idle'}"
            onclick={() => (activeFilter = "info")}
          >
            Info ({report.total_info})
          </button>
        {/if}
      </div>

      <div class="space-y-2">
        {#each filteredFindings as finding}
          {@const badge = getSeverityBadge(finding.severity)}
          <article class="panel overflow-hidden">
            <div class="flex items-start justify-between gap-3 px-4 py-3">
              <div class="min-w-0 flex-1">
                <div class="mb-1.5 flex min-w-0 items-center gap-2">
                  <span class="badge {badge.class}">
                <badge.icon size={11} />
                {badge.label}
              </span>
                  <h4 class="m-0 truncate text-sm font-semibold text-primary">{finding.title}</h4>
                </div>
                <p class="m-0 text-xs leading-relaxed text-secondary">{finding.description}</p>
            </div>

          {#if finding.remediation}
                <button
                  type="button"
                  class="btn-icon shrink-0"
                  onclick={() => copyCode(finding.remediation)}
                  title="Copy remediation command"
                >
                  {#if copiedRemediation === finding.remediation}
                    <Check size={12} class="text-running" />
                  {:else}
                    <Copy size={12} />
                  {/if}
                </button>
              {/if}
            </div>

            {#if finding.remediation}
              <div class="border-t border-border/70 bg-surface-terminal px-4 py-2 font-mono text-xs text-secondary flex items-center justify-between overflow-x-auto select-text">
                <span class="font-mono text-accent select-text whitespace-nowrap" title={finding.remediation}>{finding.remediation}</span>
              </div>
            {/if}
          </article>
        {:else}
          <div class="empty-state empty-state-dashed">
            <div class="empty-state-icon">
              <CheckCircle2 size={22} class="text-running" />
            </div>
            <p class="empty-state-title">No findings in this category</p>
            <p class="empty-state-desc">All checked parameters are compliant.</p>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
