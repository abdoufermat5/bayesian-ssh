<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { CreditCard, ExternalLink, RefreshCw, TriangleAlert } from "lucide-svelte";
  import type { OrgSummary, TeamBillingInfo, TeamSummary } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";
  import { readOnlyReason } from "$lib/utils/billing";

  /**
   * The card of a team (`team`), or of an organization (`orgId`, on its own page).
   * A team of an organization shows the organization's seats and sends owners
   * to the organization's checkout and portal.
   */
  let {
    team,
    orgId,
    isAdmin = false,
  }: {
    team?: TeamSummary;
    orgId?: string;
    isAdmin?: boolean;
  } = $props();

  let org = $state.raw<OrgSummary | null>(null);

  let info = $state.raw<TeamBillingInfo | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let upgrading = $state(false);
  let seats = $state(0);
  let yearly = $state(false);

  /** The fewest seats a subscription can have: what's in use, and more than the free plan. */
  const minSeats = $derived(info ? Math.max(info.seats_used, info.free_seats + 1) : 1);
  const reason = $derived(info ? readOnlyReason(info) : null);
  /** The organization that owns the seats, when the server has them. */
  const seatsOrgId = $derived(orgId ?? info?.org_id ?? null);
  /** Who may subscribe and open the portal: an owner of the organization, else an admin of a team without one. */
  const canManage = $derived(seatsOrgId ? org?.my_role === "owner" : isAdmin);
  const orgOwners = $derived(org?.label ?? "its owners");
  const who = $derived(seatsOrgId ? "an organization owner" : "an admin");
  const seatsBlocker = $derived.by(() => {
    if (!Number.isInteger(seats) || seats < minSeats) return `At least ${minSeats} seats`;
    if (seats > 1000) return "At most 1000 seats";
    return null;
  });

  async function load() {
    busy = true;
    error = null;
    try {
      info = orgId
        ? await invoke<TeamBillingInfo>("teams_org_billing", { org: orgId })
        : await invoke<TeamBillingInfo>("teams_billing", { team: team!.team_id });
      const id = orgId ?? info.org_id;
      org = id ? await invoke<OrgSummary>("teams_org", { org: id }).catch(() => null) : null;
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    void team?.team_id;
    void orgId;
    void load();
  });

  function startUpgrade() {
    seats = minSeats;
    yearly = false;
    upgrading = true;
  }

  async function open(command: "teams_billing_checkout" | "teams_billing_portal") {
    busy = true;
    try {
      const checkout = command === "teams_billing_checkout";
      const url = seatsOrgId
        ? await invoke<string>(
            checkout ? "teams_org_billing_checkout" : "teams_org_billing_portal",
            checkout ? { org: seatsOrgId, seats, yearly } : { org: seatsOrgId },
          )
        : await invoke<string>(
            command,
            checkout ? { team: team!.team_id, seats, yearly } : { team: team!.team_id },
          );
      await openUrl(url).catch(() => window.open(url, "_blank", "noopener,noreferrer"));
      upgrading = false;
      notify(
        command === "teams_billing_checkout"
          ? "Checkout opened in your browser. The seats update once the payment goes through: refresh this card then."
          : "Billing portal opened in your browser",
        "info",
      );
    } catch (err) {
      notify(String(err), "error");
    } finally {
      busy = false;
    }
  }

  function day(iso: string): string {
    return new Date(iso).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
  }
</script>

<section class="flex flex-col gap-2" aria-labelledby="team-billing-heading">
  <div class="flex items-center justify-between gap-2">
    <h3 id="team-billing-heading" class="section-heading flex items-center gap-1.5">
      <CreditCard size={14} /> Plan and seats
    </h3>
    <button type="button" class="btn btn-ghost btn-sm" onclick={load} disabled={busy} aria-label="Refresh plan and seats">
      <RefreshCw size={14} class={busy ? "animate-spin" : ""} />
      Refresh
    </button>
  </div>

  {#if error}
    <div class="alert alert-error" role="alert">Couldn't load the plan: {error}</div>
  {:else if info === null}
    <div class="skeleton h-14 w-full rounded-md" aria-label="Loading plan and seats"></div>
  {:else}
    <div class="panel flex flex-col gap-3 px-4 py-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div class="flex min-w-0 flex-col gap-0.5">
          {#if !info.billing_enabled}
            <span class="type-body text-primary">{info.seats_used} of {info.seat_limit} seats used</span>
            <span class="type-meta">This server has no billing: its operator sets the seats.</span>
          {:else}
            <span class="type-body text-primary">
              {#if info.plan === "team"}
                Team plan · {info.paid_seats} seats{info.interval === "year" ? ", billed yearly" : info.interval === "month" ? ", billed monthly" : ""}
              {:else}
                Free plan · {info.free_seats} seats
              {/if}
            </span>
            <span class="type-meta">
              {info.seats_used} of {info.seat_limit} seats used ({seatsOrgId ? "people in its teams" : "members"} and pending invites){#if info.current_period_end && info.plan === "team"}
                · {info.cancel_at_period_end ? "ends" : "renews"} {day(info.current_period_end)}{/if}
            </span>
          {/if}
        </div>
        {#if canManage && info.billing_enabled}
          {#if info.plan === "team" || info.status}
            <button type="button" class="btn btn-secondary btn-sm" onclick={() => open("teams_billing_portal")} disabled={busy}>
              <ExternalLink size={14} />
              Manage billing
            </button>
          {/if}
          {#if info.plan === "free" && !upgrading}
            <button type="button" class="btn btn-primary btn-sm" onclick={startUpgrade} disabled={busy}>
              Upgrade…
            </button>
          {/if}
        {/if}
      </div>

      <div
        class="h-1.5 overflow-hidden rounded-full bg-surface-active"
        role="meter"
        aria-label="Seats used"
        aria-valuemin={0}
        aria-valuemax={info.seat_limit}
        aria-valuenow={Math.min(info.seats_used, info.seat_limit)}
      >
        <div
          class="h-full rounded-full {info.seats_used >= info.seat_limit ? 'bg-warning' : 'bg-accent'}"
          style="width: {info.seat_limit ? Math.min(100, (info.seats_used / info.seat_limit) * 100) : 0}%"
        ></div>
      </div>

      {#if seatsOrgId}
        <p class="m-0 type-meta" data-testid="org-seats-note">
          {#if orgId}
            Seats are shared by the organization's teams: someone in several of them takes one seat.
          {:else}
            Seats are shared by the team's organization ({orgOwners}): someone in several of its teams takes one seat.
          {/if}
          {#if !canManage && org}Billing is managed by its owners: {orgOwners}.{/if}
        </p>
      {/if}

      {#if info.plan === "team" && info.cancel_at_period_end && info.current_period_end}
        <p class="m-0 type-meta">
          Ends on {day(info.current_period_end)}, then {orgId ? "the organization" : "the team"} falls back to the free plan.
        </p>
      {/if}
      {#if info.status === "past_due"}
        <p class="m-0 type-meta text-warning">
          Payment failed; Stripe is retrying. {canManage ? "Update the card with Manage billing." : `${who[0].toUpperCase()}${who.slice(1)} can update the card.`}
        </p>
      {/if}
      {#if reason}
        <div class="alert alert-warning" role="alert">
          <TriangleAlert size={16} class="shrink-0" />
          <div class="flex flex-col gap-1">
            <span class="alert-title">{orgId ? "This organization is read-only" : "This team is read-only"}</span>
            {#if reason === "not_enough_seats"}
              <span>
                It has more members than paid seats ({info.paid_seats}). Everyone can still sync and connect; changes
                are refused until {canManage ? "you add seats with Manage billing or remove members" : `${who} adds seats or removes members`}.
              </span>
            {:else}
              <span>
                It has no active subscription and more members than the {info.free_seats} free seats. Everyone can
                still sync and connect; changes are refused until {canManage ? "you subscribe with Upgrade… or remove members down to the free seats" : `${who} subscribes or removes members`}.
              </span>
            {/if}
          </div>
        </div>
      {:else if info.billing_enabled && info.plan === "free" && info.seats_used >= info.seat_limit}
        <p class="m-0 type-meta">
          All free seats are taken{canManage ? ": upgrade to invite more people" : `: ${who} can upgrade`}.
        </p>
      {/if}

      {#if upgrading}
        <form
          class="flex flex-col gap-3 border-t border-border-subtle pt-3"
          onsubmit={(e) => {
            e.preventDefault();
            if (!seatsBlocker) void open("teams_billing_checkout");
          }}
        >
          <div class="flex flex-wrap items-end gap-4">
            <label class="flex flex-col gap-1">
              <span class="field-label">Seats</span>
              <input class="input w-24" type="number" min={minSeats} max="1000" bind:value={seats} />
            </label>
            <fieldset class="m-0 flex gap-3 border-0 p-0">
              <legend class="field-label mb-1">Billing</legend>
              <label class="flex items-center gap-1.5 type-body">
                <input type="radio" name="billing-interval" checked={!yearly} onchange={() => (yearly = false)} /> Monthly
              </label>
              <label class="flex items-center gap-1.5 type-body">
                <input type="radio" name="billing-interval" checked={yearly} onchange={() => (yearly = true)} /> Yearly
              </label>
            </fieldset>
          </div>
          <p class="m-0 type-meta">
            Every seat of {seatsOrgId ? "the organization" : "the team"} is billed, from the first. Payment happens on Stripe's page in your browser; this app
            never sees your card. CLI: <code class="select-text">{seatsOrgId ? `bssh org billing ${org?.short_id ?? seatsOrgId.slice(0, 8)}` : `bssh teams billing ${team?.name}`} --checkout --seats {seats}{yearly ? " --yearly" : ""}</code>
          </p>
          <div class="flex items-center gap-2">
            <button type="submit" class="btn btn-primary btn-sm" disabled={busy || !!seatsBlocker} title={seatsBlocker ?? undefined}>
              <ExternalLink size={14} />
              Continue to payment
            </button>
            <button type="button" class="btn btn-ghost btn-sm" onclick={() => (upgrading = false)}>Cancel</button>
            {#if seatsBlocker}<span class="type-meta">{seatsBlocker}</span>{/if}
          </div>
        </form>
      {/if}
    </div>
  {/if}
</section>
