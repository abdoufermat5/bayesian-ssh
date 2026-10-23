import type { TeamBillingInfo } from "$lib/types";

/** Why a team refuses changes: too few paid seats on a live subscription, or no subscription at all. */
export type ReadOnlyReason = "not_enough_seats" | "no_subscription";

/** Subscription statuses that still hold their seats; `past_due` is a failed payment Stripe is retrying. */
export function isLiveStatus(status: string | null): boolean {
  return status === "active" || status === "trialing" || status === "past_due";
}

/** Mirrors the CLI: derived from `status` and `read_only`, since `seats_used` also counts pending invites. */
export function readOnlyReason(info: TeamBillingInfo): ReadOnlyReason | null {
  if (!info.read_only) return null;
  return isLiveStatus(info.status) ? "not_enough_seats" : "no_subscription";
}
