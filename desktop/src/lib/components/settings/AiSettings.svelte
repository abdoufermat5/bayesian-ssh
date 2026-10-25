<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AiPolicy, AiSettingsView, AiTier } from "$lib/types";

  const TIER_LABEL: Record<AiTier, string> = {
    "in-client": "In the agent's client",
    "out-of-band": "This app or bssh ai approve",
    "two-person": "Two approvals",
  };

  const PROFILE_LABEL: Record<AiPolicy["profile"], string> = {
    observe: "Observe: read-only tools",
    operate: "Operate: changes need approval",
    oncall: "On-call: unattended with remote approval",
  };

  let view = $state.raw<AiSettingsView | null>(null);
  let loadError = $state<string | null>(null);

  onMount(async () => {
    try {
      view = await invoke<AiSettingsView>("ai_settings");
    } catch (err) {
      loadError = String(err);
    }
  });

  const names = $derived(view ? Object.keys(view.policies).sort() : []);

  function minutes(secs: number): string {
    return secs % 60 === 0 ? `${secs / 60} min` : `${secs} s`;
  }
</script>

{#snippet values(items: string[], mono = false)}
  {#if items.length === 0}
    <span class="system-value">none</span>
  {:else}
    <span class="flex flex-wrap justify-end gap-1">
      {#each items as item (item)}
        <span class="tag {mono ? 'font-mono' : ''}">{item}</span>
      {/each}
    </span>
  {/if}
{/snippet}

<div class="settings-page">
  {#if loadError}
    <div class="alert alert-error" role="alert">Could not read the AI settings: {loadError}</div>
  {:else if !view}
    <div class="skeleton h-24 w-full rounded-lg" aria-busy="true" aria-label="Loading AI settings"></div>
  {:else}
    <section>
      <h2 class="settings-group-title">Approvals</h2>
      <div class="settings-group">
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Unanswered requests expire after</span>
            <span class="setting-meta">An approval nobody answers counts as denied.</span>
          </div>
          <div class="setting-control"><span class="system-value">{minutes(view.approval_timeout_secs)}</span></div>
        </div>
        <div class="setting-row">
          <div class="setting-row-main">
            <span class="setting-title">Record agent output</span>
            <span class="setting-meta">Keep what commands print, like interactive sessions.</span>
          </div>
          <div class="setting-control"><span class="system-value">{view.record_output ? "On" : "Off"}</span></div>
        </div>
      </div>
    </section>

    {#if names.length === 0}
      <section>
        <h2 class="settings-group-title">Policies</h2>
        <div class="settings-group">
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">No policy defined</span>
              <span class="setting-meta">
                Agents run with the built-in observe profile until you add one under
                <span class="mono">ai.policies</span> in this profile's config.json.
              </span>
            </div>
          </div>
        </div>
      </section>
    {/if}

    {#each names as name (name)}
      {@const p = view.policies[name]}
      <section aria-label="Policy {name}">
        <h2 class="settings-group-title">Policy: {name}</h2>
        <div class="settings-group">
          <div class="setting-row">
            <div class="setting-row-main"><span class="setting-title">Profile</span></div>
            <div class="setting-control"><span class="system-value">{PROFILE_LABEL[p.profile] ?? p.profile}</span></div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Hosts in scope</span>
              <span class="setting-meta">Hosts with any of these tags, plus the named hosts.</span>
            </div>
            <div class="setting-control flex-col items-end gap-1.5">
              {@render values(p.scope.tags)}
              {#if p.scope.hosts.length > 0}{@render values(p.scope.hosts)}{/if}
            </div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Who approves</span>
              <span class="setting-meta">Default channel for anything that asks.</span>
            </div>
            <div class="setting-control"><span class="system-value">{TIER_LABEL[p.approval.default] ?? p.approval.default}</span></div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Approve in the agent's client</span>
              <span class="setting-meta">Tags where low-risk, reversible changes may be confirmed there.</span>
            </div>
            <div class="setting-control">{@render values(p.approval.in_client)}</div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Two approvals needed</span>
              <span class="setting-meta">Tags that need a second person.</span>
            </div>
            <div class="setting-control">{@render values(p.approval.two_person)}</div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Two approvals on hosts</span>
              <span class="setting-meta">Connection-name globs that need a second person, whatever their tags say.</span>
            </div>
            <div class="setting-control">{@render values(p.approval.two_person_hosts)}</div>
          </div>
        </div>

        <div class="settings-group mt-3">
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Trusted tag sources</span>
              <span class="setting-meta">Inventory sources whose tags may widen scope or allow in-client approval. Tags from any other source can only tighten the policy.</span>
            </div>
            <div class="setting-control">{@render values(view.trusted_tag_sources)}</div>
          </div>
        </div>

        <div class="settings-group mt-3">
          <div class="setting-row">
            <div class="setting-row-main"><span class="setting-title">Readable paths</span></div>
            <div class="setting-control">{@render values(p.read_paths, true)}</div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main"><span class="setting-title">Writable paths</span></div>
            <div class="setting-control">{@render values(p.write_paths, true)}</div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main"><span class="setting-title">Never accessible</span></div>
            <div class="setting-control">{@render values(p.deny_paths, true)}</div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Blocked commands</span>
              <span class="setting-meta">Matched on the parsed command, not the raw text.</span>
            </div>
            <div class="setting-control">{@render values(p.deny, true)}</div>
          </div>
        </div>

        <div class="settings-group mt-3">
          <div class="setting-row">
            <div class="setting-row-main"><span class="setting-title">Calls per minute</span></div>
            <div class="setting-control"><span class="system-value tabular-nums">{p.budgets.calls_per_min}</span></div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main"><span class="setting-title">Changes per run</span></div>
            <div class="setting-control"><span class="system-value tabular-nums">{p.budgets.changes_per_run}</span></div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main"><span class="setting-title">Run time limit</span></div>
            <div class="setting-control"><span class="system-value">{p.budgets.run_ttl}</span></div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Pause after denials</span>
              <span class="setting-meta">Denials within one minute that pause the run.</span>
            </div>
            <div class="setting-control"><span class="system-value tabular-nums">{p.budgets.max_denials_per_min}</span></div>
          </div>
          <div class="setting-row">
            <div class="setting-row-main">
              <span class="setting-title">Hide secrets from the agent</span>
              <span class="setting-meta">Vault values and common token patterns are replaced before output is returned.</span>
            </div>
            <div class="setting-control">
              <span class="system-value">
                {p.redact.vault_secrets ? "Vault values, " : ""}patterns: {p.redact.patterns}
              </span>
            </div>
          </div>
        </div>
      </section>
    {/each}

    <p class="-mt-3 type-meta">
      Read-only. Policies live in this profile's config.json under <span class="mono">ai.policies</span>.
    </p>
  {/if}
</div>
