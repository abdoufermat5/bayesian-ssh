<script lang="ts">
  import { ChevronLeft, ChevronRight, X } from "lucide-svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import RiskTags from "$lib/components/ai/RiskTags.svelte";
  import StepList from "$lib/components/ai/StepList.svelte";
  import DiffView from "$lib/components/ai/DiffView.svelte";
  import type { AiApprovalRequest, AiToolCall } from "$lib/types";
  import {
    approvalDialogOpen,
    closeApproval,
    currentApproval,
    decideApproval,
    pendingApprovals,
    stepApproval,
  } from "$lib/stores/ai.svelte";
  import {
    formatCountdown,
    grantBlocker,
    planStepAnalyses,
    shortId,
    argString,
    argStrings,
    stepHosts,
    toolLabel,
    unionTags,
  } from "$lib/utils/ai";

  const GRANT_CHOICES = [5, 15, 30, 60];

  const open = $derived(approvalDialogOpen());
  const current = $derived(currentApproval());
  const total = $derived(pendingApprovals().length);
  const position = $derived(
    current ? pendingApprovals().findIndex((p) => p.request.request_id === current.request.request_id) + 1 : 0,
  );

  let now = $state(Date.now());
  let denyReason = $state("");
  let grantMinutes = $state("15");
  let busy = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!open) return;
    now = Date.now();
    const timer = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(timer);
  });

  // A different request starts with a clean form.
  $effect(() => {
    void current?.request.request_id;
    denyReason = "";
    error = null;
  });

  const request = $derived<AiApprovalRequest | null>(current?.request ?? null);
  const msLeft = $derived(request ? Date.parse(request.expires_at) - now : 0);
  const expired = $derived(request !== null && msLeft <= 0);
  const blocker = $derived(request ? grantBlocker(request) : null);

  const subjectName = $derived(
    !request ? "" : request.subject.kind === "call" ? "command" : request.subject.kind === "plan" ? "change plan" : "access request",
  );

  /** Hosts a single call touches: its host, or `hosts` when the agent sent several. */
  function callHosts(call: AiToolCall): string[] {
    if (call.host) return [call.host];
    return argStrings(call.args, "hosts");
  }

  function callArgs(call: AiToolCall): string {
    try {
      return JSON.stringify(call.args, null, 2) ?? "null";
    } catch {
      return String(call.args);
    }
  }

  /** What the agent wrote to justify the request, if anything. Untrusted. */
  function agentReason(r: AiApprovalRequest): string | null {
    const s = r.subject;
    if (s.kind === "plan") return s.plan.reason || null;
    if (s.kind === "access") return s.reason || null;
    return argString(s.call.args, "reason");
  }

  async function decide(approve: boolean, withGrant = false) {
    if (!current || busy) return;
    busy = true;
    error = null;
    try {
      await decideApproval(
        current.run.run_id,
        current.request.request_id,
        approve,
        approve ? undefined : denyReason,
        withGrant ? Number(grantMinutes) : undefined,
      );
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

{#if current && request}
  <ModalShell {open} title="Approval needed" onClose={closeApproval} width="lg" closeOnBackdrop={false}>
    <div class="modal-header">
      <div class="min-w-0">
        <h2 class="modal-title">Approve this {subjectName}?</h2>
        <p class="modal-subtitle">
          <span class="text-secondary">{request.client || "Unknown agent"}</span> (name reported by the agent) ·
          run <span class="mono">{shortId(request.run_id)}</span> ·
          <span class="tabular-nums" class:text-error={expired}>
            {expired ? "Expired" : `expires in ${formatCountdown(msLeft)}`}
          </span>
        </p>
      </div>
      <div class="flex shrink-0 items-center gap-1">
        {#if total > 1}
          <button type="button" class="btn-icon btn-icon-sm" aria-label="Previous request" onclick={() => stepApproval(-1)}>
            <ChevronLeft size={14} />
          </button>
          <span class="type-meta tabular-nums" aria-live="polite">{position} of {total}</span>
          <button type="button" class="btn-icon btn-icon-sm" aria-label="Next request" onclick={() => stepApproval(1)}>
            <ChevronRight size={14} />
          </button>
        {/if}
        <button type="button" class="modal-close" onclick={closeApproval} aria-label="Close">
          <X size={16} />
        </button>
      </div>
    </div>

    <div class="modal-body flex flex-col gap-4">
      {#if request.subject.kind === "call"}
        {@const call = request.subject.call}
        {@const analysis = request.subject.analysis}
        {@const hosts = callHosts(call)}
        {@const patch = argString(call.args, "patch")}
        <section class="flex flex-col gap-2" aria-label="What will run">
          <h3 class="section-label m-0">{toolLabel(call.tool)}</h3>
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="type-meta">On</span>
            {#each hosts as host (host)}
              <span class="tag">{host}</span>
            {:else}
              <span class="tag">no host</span>
            {/each}
            {#if call.path}
              <span class="type-meta">File</span>
              <span class="mono break-all text-primary">{call.path}</span>
            {/if}
          </div>
          {#if call.command}
            <pre class="code-block m-0 whitespace-pre-wrap break-words" aria-label="Exact command">{call.command}</pre>
          {/if}
          {#if patch}
            <DiffView {patch} label={call.path ?? "the file"} />
          {/if}
          <details>
            <summary class="type-meta cursor-pointer select-none">Arguments exactly as sent</summary>
            <pre class="code-block mt-2 whitespace-pre-wrap break-words">{callArgs(call)}</pre>
          </details>
        </section>

        <section class="flex flex-col gap-2" aria-label="Analyzer">
          <h3 class="section-label m-0">Analyzer</h3>
          {#if analysis}
            <div class="flex flex-wrap items-center gap-1.5">
              <RiskTags tags={unionTags([analysis])} />
              {#if analysis.tags.length === 0}<span class="type-meta">No risk tags.</span>{/if}
            </div>
            {#if analysis.unknown_reason}
              <p class="alert alert-warning m-0">Could not be fully analysed: {analysis.unknown_reason}</p>
            {/if}
            {#if analysis.commands.length > 0}
              <ul class="m-0 flex list-none flex-col gap-1 p-0">
                {#each analysis.commands as cmd, i (i)}
                  <li class="flex flex-wrap items-center gap-1.5 text-xs">
                    <span class="mono break-all text-primary">
                      {[...cmd.wrappers, cmd.program, ...cmd.args].join(" ")}
                    </span>
                    <RiskTags tags={[...cmd.tags]} />
                    {#if cmd.rule}<span class="type-meta">rule {cmd.rule}</span>{/if}
                  </li>
                {/each}
              </ul>
            {/if}
          {:else}
            <p class="type-meta m-0">This call was not analysed.</p>
          {/if}
        </section>
      {:else if request.subject.kind === "plan"}
        {@const plan = request.subject.plan}
        {@const parts = planStepAnalyses(request.subject.analyses, plan.steps, plan.verify, plan.rollback)}
        <section class="flex flex-col gap-2" aria-label="Change plan">
          <h3 class="section-label m-0">Change plan</h3>
          <p class="m-0 text-base font-semibold text-primary">{plan.title}</p>
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="type-meta">On</span>
            {#each stepHosts([...plan.steps, ...plan.verify, ...plan.rollback]) as host (host)}
              <span class="tag">{host}</span>
            {/each}
            <RiskTags tags={unionTags(request.subject.analyses)} />
          </div>
          <p class="type-meta m-0">
            Your answer covers exactly these steps (plan <span class="mono">{request.subject.plan_hash.slice(0, 12)}</span>).
            Anything else is a new request.
          </p>
        </section>
        <StepList label="Steps" steps={plan.steps} analyses={parts.steps} />
        <StepList label="Verify" steps={plan.verify} analyses={parts.verify} />
        <StepList label="Rollback" steps={plan.rollback} analyses={parts.rollback} />
      {:else}
        {@const access = request.subject}
        <section class="flex flex-col gap-2" aria-label="Access request">
          <h3 class="section-label m-0">Wider access</h3>
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="type-meta">Tags</span>
            {#each access.scope.tags as tag (tag)}<span class="tag">{tag}</span>{:else}<span class="type-meta">none</span>{/each}
            <span class="type-meta">Hosts</span>
            {#each access.scope.hosts as host (host)}<span class="tag">{host}</span>{:else}<span class="type-meta">none</span>{/each}
          </div>
          <p class="m-0 text-sm text-primary">For {access.duration}.</p>
        </section>
      {/if}

      {#if agentReason(request)}
        <section class="flex flex-col gap-1 border-l-2 border-border pl-3" aria-label="The agent says">
          <h3 class="type-caption m-0 text-muted">The agent says</h3>
          <p class="m-0 whitespace-pre-wrap break-words text-xs leading-relaxed text-muted">{agentReason(request)}</p>
          <p class="type-meta m-0">Written by the agent and not checked. It can echo text found on a server.</p>
        </section>
      {/if}

      {#if request.tier === "two-person"}
        <p class="type-meta m-0">This needs two approvals. Yours counts as one.</p>
      {:else if request.tier === "in-client"}
        <p class="type-meta m-0">The agent's own client may ask too. Answering here is enough.</p>
      {/if}

      <div class="field">
        <label class="field-label" for="ai-deny-reason">Reason if you deny (optional)</label>
        <input
          id="ai-deny-reason"
          type="text"
          class="input"
          bind:value={denyReason}
          maxlength="500"
          autocomplete="off"
          spellcheck="false"
          disabled={busy || expired}
        />
        <span class="field-hint">Kept in the run's audit trail.</span>
      </div>

      {#if error}
        <div class="alert alert-error" role="alert">{error}</div>
      {/if}
    </div>

    <div class="modal-footer">
      <button type="button" class="btn btn-ghost mr-auto" onclick={closeApproval}>Decide later</button>
      <button type="button" class="btn btn-secondary" disabled={busy || expired} onclick={() => decide(false)} data-autofocus>
        Deny
      </button>
      {#if blocker === null && request.subject.kind === "call"}
        <select class="input w-auto" bind:value={grantMinutes} aria-label="Grant length" disabled={busy || expired}>
          {#each GRANT_CHOICES as m (m)}
            <option value={String(m)}>{m} min</option>
          {/each}
        </select>
        <button
          type="button"
          class="btn btn-secondary"
          disabled={busy || expired}
          onclick={() => decide(true, true)}
          title="Approve, and let this run repeat this kind of call on this host without asking, for the chosen time"
        >
          Approve and allow
        </button>
      {:else if blocker && request.subject.kind === "call"}
        <span class="type-meta mr-1" title={blocker}>No grant offered</span>
      {/if}
      <button type="button" class="btn btn-primary" disabled={busy || expired} onclick={() => decide(true)}>
        Approve
      </button>
    </div>
  </ModalShell>
{/if}
