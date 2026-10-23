<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { ArrowLeft, Building2, UserMinus, UserPlus } from "lucide-svelte";
  import type { OrgSummary } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import TeamBilling from "./TeamBilling.svelte";
  import { plural } from "./teams";

  let {
    org,
    onBack,
    onOpenTeam,
    onChanged,
  }: {
    org: OrgSummary;
    onBack: () => void;
    /** Open the page of a team this machine knows. */
    onOpenTeam: (teamId: string) => void;
    /** Owners changed: the parent reloads the organizations. */
    onChanged: (org: OrgSummary) => void;
  } = $props();

  let email = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let removing = $state<string | null>(null);

  const isOwner = $derived(org.my_role === "owner");
  const emailOk = $derived(/^\S+@\S+$/.test(email.trim()));

  async function addOwner() {
    const value = email.trim().toLowerCase();
    if (!emailOk || busy) return;
    busy = true;
    error = null;
    try {
      const next = await invoke<OrgSummary>("teams_org_add_owner", { org: org.org_id, email: value });
      email = "";
      onChanged(next);
      notify(`${value} is now an owner`, "success");
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  async function removeOwner(target: string) {
    busy = true;
    error = null;
    try {
      const next = await invoke<OrgSummary>("teams_org_remove_owner", { org: org.org_id, email: target });
      removing = null;
      onChanged(next);
      notify(`${target} is no longer an owner`, "success");
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex flex-col gap-4">
  <div class="flex flex-wrap items-center gap-2.5">
    <button type="button" class="btn btn-ghost btn-sm" onclick={onBack}>
      <ArrowLeft size={14} />
      All teams
    </button>
    <h2 class="m-0 min-w-0 truncate text-lg font-semibold text-primary">{org.label}</h2>
    <span class="badge {isOwner ? 'badge-accent' : 'badge-neutral'}">{isOwner ? "Owner" : "Member"}</span>
    <span class="type-meta">Organization {org.short_id} · {plural(org.teams.length, "team")}</span>
  </div>

  {#if error}<div class="alert alert-error" role="alert">{error}</div>{/if}

  <section class="flex flex-col gap-2" aria-labelledby="org-owners-heading">
    <h3 id="org-owners-heading" class="section-heading flex items-center gap-1.5">
      <Building2 size={14} /> Owners
    </h3>
    <div class="panel flex flex-col gap-3 px-4 py-3">
      <ul class="m-0 flex list-none flex-col gap-1.5 p-0">
        {#each org.owners as owner (owner.account_id)}
          <li class="flex flex-wrap items-center justify-between gap-2">
            <span class="type-body text-primary">{owner.email}</span>
            {#if isOwner && org.owners.length > 1}
              {#if removing === owner.email}
                <span class="flex items-center gap-2">
                  <span class="type-meta">Remove {owner.email} as owner?</span>
                  <button type="button" class="btn btn-danger btn-sm" onclick={() => removeOwner(owner.email)} disabled={busy}>
                    Confirm remove
                  </button>
                  <button type="button" class="btn btn-ghost btn-sm" onclick={() => (removing = null)}>Cancel</button>
                </span>
              {:else}
                <button
                  type="button"
                  class="btn btn-ghost btn-sm"
                  onclick={() => (removing = owner.email)}
                  aria-label="Remove owner {owner.email}"
                >
                  <UserMinus size={14} />
                  Remove
                </button>
              {/if}
            {/if}
          </li>
        {/each}
      </ul>
      {#if isOwner}
        <form
          class="flex flex-wrap items-end gap-2 border-t border-border-subtle pt-3"
          onsubmit={(e) => {
            e.preventDefault();
            void addOwner();
          }}
        >
          <label class="flex min-w-64 flex-col gap-1">
            <span class="field-label">Add an owner</span>
            <input class="input" type="email" bind:value={email} placeholder="member@example.com" />
          </label>
          <button type="submit" class="btn btn-secondary btn-sm" disabled={busy || !emailOk}>
            <UserPlus size={14} />
            Add owner
          </button>
        </form>
        <p class="m-0 type-meta">
          The person must be a member of one of this organization's teams. Owners manage billing and owners and create
          teams here. CLI: <code class="select-text">bssh org owners add {org.short_id} EMAIL</code>
        </p>
      {:else}
        <p class="m-0 type-meta">Only owners can change owners and billing.</p>
      {/if}
    </div>
  </section>

  <section class="flex flex-col gap-2" aria-labelledby="org-teams-heading">
    <h3 id="org-teams-heading" class="section-heading">Teams</h3>
    <div class="panel px-4 py-3">
      {#if org.teams.length === 0}
        <p class="m-0 type-meta">No teams yet: create one from the Teams page and choose this organization.</p>
      {:else}
        <ul class="m-0 flex list-none flex-col gap-1.5 p-0">
          {#each org.teams as t (t.team_id)}
            <li class="flex items-center gap-2">
              {#if t.name}
                <button type="button" class="link font-medium" onclick={() => onOpenTeam(t.team_id)}>{t.name}</button>
              {:else}
                <span class="font-mono text-sm text-secondary">{t.short_id}</span>
                <span class="type-meta">not on this machine</span>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </section>

  <TeamBilling orgId={org.org_id} />
</div>
