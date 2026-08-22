<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { ShieldCheck, ShieldAlert, AlertTriangle, Info, RefreshCw, CheckCircle2, Wrench } from "lucide-svelte";
  import type { AuditReport, AuditFinding } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";

  let report = $state<AuditReport | null>(null);
  let loading = $state(true);
  let fixing = $state(false);
  let activeFilter = $state<"all" | "critical" | "warning" | "info">("all");

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

  onMount(() => {
    runAudit();
  });

  const filteredFindings = $derived(
    report?.findings.filter((f) => activeFilter === "all" || f.severity === activeFilter) ?? []
  );

  function getGradeColor(grade: string) {
    if (grade.startsWith("A")) return "text-running border-success/30 bg-success/10";
    if (grade.startsWith("B")) return "text-blue-400 border-blue-500/30 bg-blue-500/10";
    if (grade.startsWith("C")) return "text-warning border-warning/30 bg-warning/10";
    return "text-error border-error/30 bg-error/10";
  }

  function getSeverityBadge(severity: AuditFinding["severity"]) {
    switch (severity) {
      case "critical":
        return { label: "CRITICAL", class: "text-error bg-error/15 border-error/30", icon: ShieldAlert };
      case "warning":
        return { label: "WARNING", class: "text-warning bg-warning/15 border-warning/30", icon: AlertTriangle };
      case "info":
        return { label: "INFO", class: "text-blue-400 bg-blue-500/15 border-blue-500/30", icon: Info };
    }
  }
</script>

<div class="flex flex-col flex-1 h-full min-h-0 overflow-y-auto p-6 bg-surface text-primary">
  <!-- Header -->
  <div class="flex items-center justify-between mb-6 pb-4 border-b border-border">
    <div>
      <h1 class="text-xl font-bold tracking-tight flex items-center gap-2 text-primary">
        <ShieldCheck class="text-accent" size={24} />
        Security Auditor
      </h1>
      <p class="text-xs text-muted mt-1">
        Comprehensive automated audit of your SSH identity files, strict host key settings, private key permissions, and connection freshness.
      </p>
    </div>
    <div class="flex items-center gap-2">
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-emerald-600/20 text-running border border-success/30 text-xs font-semibold cursor-pointer transition-all hover:bg-success/30 shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
        onclick={fixPermissions}
        disabled={loading || fixing}
      >
        <Wrench size={14} class={fixing ? "animate-spin" : ""} />
        Fix All Insecure Permissions
      </button>
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-semibold cursor-pointer transition-all hover:opacity-90 shadow-sm disabled:opacity-50 disabled:cursor-not-allowed"
        onclick={runAudit}
        disabled={loading}
      >
        <RefreshCw size={14} class={loading ? "animate-spin" : ""} />
        Re-run Audit
      </button>
    </div>
  </div>

  {#if loading}
    <div class="flex flex-col items-center justify-center py-20 text-muted">
      <RefreshCw class="animate-spin mb-3 text-accent" size={32} />
      <span class="text-xs font-medium">Auditing system configuration & key permissions...</span>
    </div>
  {:else if report}
    <!-- Score Overview Banner -->
    <div class="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
      <!-- Score Circle Card -->
      <div class="p-5 rounded-2xl border border-border bg-surface-input/50 flex flex-col items-center justify-center text-center">
        <div class="text-xs font-semibold text-muted uppercase tracking-wider mb-2">Security Score</div>
        <div class="text-4xl font-extrabold text-primary mb-1">{report.score}<span class="text-sm text-muted font-normal">/100</span></div>
        <span class="px-2.5 py-0.5 rounded-full text-xs font-bold border mt-1 {getGradeColor(report.grade)}">
          Grade: {report.grade} ({report.rating})
        </span>
      </div>

      <!-- Critical Card -->
      <div class="p-5 rounded-2xl border border-border bg-surface-input/50 flex flex-col justify-between">
        <div class="flex items-center justify-between">
          <span class="text-xs font-semibold text-muted uppercase tracking-wider">Critical Issues</span>
          <ShieldAlert class="text-error" size={20} />
        </div>
        <div class="text-3xl font-extrabold text-error mt-2">{report.total_critical}</div>
        <span class="text-[11px] text-muted mt-1">Requires immediate attention</span>
      </div>

      <!-- Warning Card -->
      <div class="p-5 rounded-2xl border border-border bg-surface-input/50 flex flex-col justify-between">
        <div class="flex items-center justify-between">
          <span class="text-xs font-semibold text-muted uppercase tracking-wider">Warnings</span>
          <AlertTriangle class="text-warning" size={20} />
        </div>
        <div class="text-3xl font-extrabold text-warning mt-2">{report.total_warning}</div>
        <span class="text-[11px] text-muted mt-1">Recommended fixes</span>
      </div>

      <!-- Info Card -->
      <div class="p-5 rounded-2xl border border-border bg-surface-input/50 flex flex-col justify-between">
        <div class="flex items-center justify-between">
          <span class="text-xs font-semibold text-muted uppercase tracking-wider">Informational</span>
          <Info class="text-blue-400" size={20} />
        </div>
        <div class="text-3xl font-extrabold text-blue-400 mt-2">{report.total_info}</div>
        <span class="text-[11px] text-muted mt-1">Best-practice suggestions</span>
      </div>
    </div>

    <!-- Filter Tabs & Findings -->
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-1.5 bg-surface-input p-1 rounded-xl border border-border">
        {#each [
          { id: "all", label: `All (${report.findings.length})` },
          { id: "critical", label: `Critical (${report.total_critical})` },
          { id: "warning", label: `Warnings (${report.total_warning})` },
          { id: "info", label: `Info (${report.total_info})` },
        ] as filter}
          <button
            class="px-3 py-1 rounded-lg text-xs font-semibold cursor-pointer transition-all border-none disabled:opacity-50 disabled:cursor-not-allowed
              {activeFilter === filter.id ? 'bg-accent text-white shadow-sm' : 'text-muted hover:text-primary hover:bg-white/5'}"
            onclick={() => (activeFilter = filter.id as typeof activeFilter)}
          >
            {filter.label}
          </button>
        {/each}
      </div>
    </div>

    <!-- Findings List -->
    {#if filteredFindings.length === 0}
      <div class="flex flex-col items-center justify-center py-16 border border-dashed border-border rounded-xl text-muted">
        <CheckCircle2 size={40} class="text-running mb-2 opacity-80" />
        <span class="text-sm font-semibold text-primary">No Findings in this Category</span>
        <span class="text-xs text-muted mt-1">Your SSH environment meets security requirements for this filter.</span>
      </div>
    {:else}
      <div class="space-y-3">
        {#each filteredFindings as finding}
          {@const badge = getSeverityBadge(finding.severity)}
          <div class="p-4 rounded-xl border border-border bg-surface-input/50 space-y-2 transition-all hover:border-border-hover">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <span class="flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-extrabold uppercase tracking-wider border {badge.class}">
                  <badge.icon size={11} />
                  {badge.label}
                </span>
                <span class="font-bold text-sm text-primary">{finding.title}</span>
              </div>
            </div>
            <p class="text-xs text-muted leading-relaxed">{finding.description}</p>
            <div class="flex items-start gap-1.5 p-2.5 rounded-lg bg-black/20 border border-white/5 text-xs text-accent font-mono">
              <span class="font-bold text-primary shrink-0 font-sans">Remediation:</span> {finding.remediation}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
