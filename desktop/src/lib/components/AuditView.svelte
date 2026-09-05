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
    if (grade.startsWith("A")) return "text-running border-success/30 bg-success/10";
    if (grade.startsWith("B")) return "text-accent border-accent/30 bg-accent/10";
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
        return { label: "INFO", class: "text-accent bg-accent/15 border-accent/30", icon: Info };
    }
  }
</script>

<div class="flex flex-col flex-1 h-full min-h-0 overflow-y-auto px-6 py-5 bg-surface text-primary select-none scrollbar-none">
  <!-- Header Bar -->
  <div class="flex items-center justify-between gap-4 pb-4 border-b border-border shrink-0 flex-wrap">
    <div class="flex items-center gap-3">
      <div class="w-8 h-8 rounded-xl bg-accent/15 border border-accent/30 flex items-center justify-center text-accent shrink-0">
        <ShieldCheck size={18} />
      </div>
      <div>
        <h2 class="text-sm font-bold text-primary tracking-tight m-0">Security Auditor</h2>
        <p class="text-[11px] text-muted m-0">Identity files, strict host keys, file permissions, and posture</p>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <button
        type="button"
        class="btn bg-running/15 text-running border border-running/30 hover:bg-running/25 shadow-sm"
        onclick={fixPermissions}
        disabled={loading || fixing}
      >
        <Wrench size={13} class={fixing ? "animate-spin" : ""} />
        <span>Fix All Permissions</span>
      </button>

      <button
        type="button"
        class="btn btn-secondary"
        onclick={runAudit}
        disabled={loading}
      >
        <RefreshCw size={13} class={loading ? "animate-spin" : ""} />
        <span>Re-scan</span>
      </button>
    </div>
  </div>

  {#if loading}
    <div class="py-24 flex flex-col items-center justify-center text-muted gap-2">
      <RefreshCw size={28} class="text-accent animate-spin" />
      <span class="text-xs font-semibold text-primary">Auditing system security posture...</span>
      <span class="text-[11px]">Inspecting identity files, file permissions, and SSH configuration</span>
    </div>
  {:else if report}
    <!-- Scorecard & Overview Tiles -->
    <div class="grid grid-cols-1 md:grid-cols-4 gap-3.5 my-4 shrink-0">
      <!-- Grade Card -->
      <div class="rounded-xl border p-4 flex items-center gap-4 bg-surface-input/50 {getGradeClass(report.grade)}">
        <div class="w-14 h-14 rounded-2xl flex items-center justify-center border font-mono text-2xl font-black shrink-0 {getGradeClass(report.grade)}">
          {report.grade}
        </div>
        <div class="min-w-0">
          <span class="eyebrow text-[10px]">Posture Rating</span>
          <h3 class="text-base font-bold truncate leading-tight mt-0.5">{report.rating}</h3>
          <span class="text-[11px] font-mono text-muted">{report.score}/100 score</span>
        </div>
      </div>

      <!-- Critical Card -->
      <div class="metric-tile">
        <span class="eyebrow flex items-center justify-between text-error">
          Critical Issues
          <ShieldAlert size={12} />
        </span>
        <div class="flex items-baseline gap-2 mt-0.5">
          <span class="text-xl font-bold tracking-tight {report.total_critical > 0 ? 'text-error' : 'text-primary'}">
            {report.total_critical}
          </span>
          <span class="text-[11px] text-muted">require attention</span>
        </div>
      </div>

      <!-- Warning Card -->
      <div class="metric-tile">
        <span class="eyebrow flex items-center justify-between text-warning">
          Warnings
          <AlertTriangle size={12} />
        </span>
        <div class="flex items-baseline gap-2 mt-0.5">
          <span class="text-xl font-bold tracking-tight {report.total_warning > 0 ? 'text-warning' : 'text-primary'}">
            {report.total_warning}
          </span>
          <span class="text-[11px] text-muted">best practices</span>
        </div>
      </div>

      <!-- Info Card -->
      <div class="metric-tile">
        <span class="eyebrow flex items-center justify-between text-accent">
          Passed / Info
          <CheckCircle2 size={12} />
        </span>
        <div class="flex items-baseline gap-2 mt-0.5">
          <span class="text-xl font-bold tracking-tight text-primary">
            {report.total_info}
          </span>
          <span class="text-[11px] text-muted">checks recorded</span>
        </div>
      </div>
    </div>

    <!-- Findings Filter Tabs -->
    <div class="flex items-center gap-1.5 pb-2 border-b border-border/80 shrink-0">
      <button
        type="button"
        class="text-xs px-2.5 py-1 rounded-lg font-semibold transition-colors cursor-pointer border
          {activeFilter === 'all'
            ? 'border-accent/40 bg-accent/15 text-accent'
            : 'border-transparent text-muted hover:text-primary hover:bg-surface-hover'}"
        onclick={() => (activeFilter = "all")}
      >
        All Findings ({report.findings.length})
      </button>

      {#if report.total_critical > 0}
        <button
          type="button"
          class="text-xs px-2.5 py-1 rounded-lg font-semibold transition-colors cursor-pointer border
            {activeFilter === 'critical'
              ? 'border-error/40 bg-error/15 text-error'
              : 'border-transparent text-muted hover:text-error hover:bg-error/10'}"
          onclick={() => (activeFilter = "critical")}
        >
          Critical ({report.total_critical})
        </button>
      {/if}

      {#if report.total_warning > 0}
        <button
          type="button"
          class="text-xs px-2.5 py-1 rounded-lg font-semibold transition-colors cursor-pointer border
            {activeFilter === 'warning'
              ? 'border-warning/40 bg-warning/15 text-warning'
              : 'border-transparent text-muted hover:text-warning hover:bg-warning/10'}"
          onclick={() => (activeFilter = "warning")}
        >
          Warnings ({report.total_warning})
        </button>
      {/if}

      {#if report.total_info > 0}
        <button
          type="button"
          class="text-xs px-2.5 py-1 rounded-lg font-semibold transition-colors cursor-pointer border
            {activeFilter === 'info'
              ? 'border-accent/40 bg-accent/15 text-accent'
              : 'border-transparent text-muted hover:text-primary hover:bg-surface-hover'}"
          onclick={() => (activeFilter = "info")}
        >
          Informational ({report.total_info})
        </button>
      {/if}
    </div>

    <!-- Findings List -->
    <div class="space-y-3 pt-3 flex-1">
      {#each filteredFindings as finding}
        {@const badge = getSeverityBadge(finding.severity)}
        <div class="rounded-xl border border-border bg-surface-input/30 p-4 transition-all hover:border-border-hover">
          <div class="flex items-start justify-between gap-3 mb-2">
            <div class="flex items-center gap-2.5 min-w-0">
              <span class="badge-pill {badge.class}">
                <badge.icon size={11} />
                {badge.label}
              </span>
              <h4 class="text-xs font-bold text-primary truncate m-0">{finding.title}</h4>
            </div>
          </div>

          <p class="text-xs text-secondary leading-relaxed mb-3 m-0">
            {finding.description}
          </p>

          {#if finding.remediation}
            <div class="p-2.5 rounded-lg bg-surface border border-border/80 flex items-center justify-between gap-2 font-mono text-[11px] text-accent">
              <span class="truncate">{finding.remediation}</span>
              <button
                type="button"
                class="p-1 text-muted hover:text-primary rounded hover:bg-surface-hover border-none bg-transparent cursor-pointer shrink-0"
                onclick={() => copyCode(finding.remediation)}
                title="Copy command"
              >
                {#if copiedRemediation === finding.remediation}
                  <Check size={12} class="text-running" />
                {:else}
                  <Copy size={12} />
                {/if}
              </button>
            </div>
          {/if}
        </div>
      {:else}
        <div class="py-16 flex flex-col items-center justify-center text-muted gap-2 border border-dashed border-border rounded-xl">
          <CheckCircle2 size={32} class="text-running" />
          <p class="text-xs font-semibold text-primary">No findings in this category</p>
          <p class="text-[11px]">All checked parameters are compliant.</p>
        </div>
      {/each}
    </div>
  {/if}
</div>
