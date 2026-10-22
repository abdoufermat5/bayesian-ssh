<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { CreditCard, ExternalLink, RefreshCw, TriangleAlert } from "lucide-svelte";
  import type { TeamBillingInfo, TeamSummary } from "$lib/types";
  import { notify } from "$lib/stores/notifications.svelte";

  let {
    team,
    isAdmin,
  }: {
    team: TeamSummary;
    isAdmin: boolean;
  } = $props();

  let info = $state.raw<TeamBillingInfo | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let upgrading = $state(false);
  let seats = $state(0);
  let yearly = $state(false);

  /** The fewest seats a subscription can have: what's in use, and more than the free plan. */
  const minSeats = $derived(info ? Math.max(info.seats_used, info.free_seats + 1) : 1);
  const seatsBlocker = $derived.by(() => {
    if (!Number.isInteger(seats) || seats < minSeats) return `At least ${minSeats} seats`;
    if (seats > 1000) return "At most 1000 seats";
    return null;
  });

  async function load() {
    busy = true;
    error = null;
    try {
      info = await invoke<TeamBillingInfo>("teams_billing", { team: team.team_id });
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    void team.team_id;
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
      const url = await invoke<string>(
        command,
        command === "teams_billing_checkout" ? { team: team.team_id, seats, yearly } : { team: team.team_id },
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
              {info.seats_used} of {info.seat_limit} seats used (members and pending invites){#if info.current_period_end && info.plan === "team"}
                · {info.cancel_at_period_end ? "ends" : "renews"} {day(info.current_period_end)}{/if}
            </span>
          {/if}
        </div>
        {#if isAdmin && info.billing_enabled}
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

      {#if info.status === "past_due"}
        <p class="m-0 type-meta text-warning">The last payment failed; Stripe is retrying. Update the card in Manage billing.</p>
      {/if}
      {#if info.read_only}
        <div class="alert alert-warning" role="alert">
          <TriangleAlert size={16} class="shrink-0" />
          <div class="flex flex-col gap-1">
            <span class="alert-title">This team is read-only</span>
            <span>
              It has more members than the free plan and no active subscription. Everyone can still sync and
              connect; changes are refused until {isAdmin ? "you subscribe or remove members" : "an admin subscribes or removes members"}.
            </span>
          </div>
        </div>
      {:else if info.billing_enabled && info.plan === "free" && info.seats_used >= info.seat_limit}
        <p class="m-0 type-meta">
          All free seats are taken{isAdmin ? ": upgrade to invite more people" : ": an admin can upgrade the team"}.
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
            Every seat of the team is billed, from the first. Payment happens on Stripe's page in your browser; this app
            never sees your card. CLI: <code class="select-text">bssh teams billing {team.name} --checkout --seats {seats}{yearly ? " --yearly" : ""}</code>
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
