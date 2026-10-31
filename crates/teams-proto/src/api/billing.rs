//! Billing types.
//!
//! Payment happens on the billing provider's pages (checkout and portal): the
//! server only hands out their URLs, and learns the outcome from the provider.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    /// Up to `free_seats` members, no subscription.
    Free,
    /// A paid subscription for `paid_seats` seats.
    Team,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingInterval {
    Month,
    Year,
}

/// `GET /v1/orgs/{org_id}/billing` (any member of the organization), or
/// `GET /v1/teams/{team_id}/billing` (any team member) for the team's
/// organization. With billing, seats are counted across the organization:
/// distinct members of its teams plus pending invites to new people.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingInfo {
    /// False on a server without billing (self-hosted): `seat_limit` is fixed.
    pub billing_enabled: bool,
    pub plan: Plan,
    pub free_seats: u32,
    /// Seats of the subscription; 0 without one.
    pub paid_seats: u32,
    /// What invites are checked against: members plus pending invites.
    pub seat_limit: u32,
    pub seats_used: u32,
    /// The provider's subscription status (`active`, `past_due`, `canceled`…).
    pub status: Option<String>,
    pub interval: Option<BillingInterval>,
    pub current_period_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    /// More members than the free seats and no active subscription: pushes
    /// and invites fail with `PaymentRequired`/`SeatLimit` until paid.
    pub read_only: bool,
    /// The organization this billing belongs to; `None` from a server without
    /// organizations.
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

/// `POST /v1/orgs/{org_id}/billing/checkout` (owner): start a subscription.
/// `POST /v1/teams/{team_id}/billing/checkout` does the same for the team's
/// organization; the caller must own it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutRequest {
    /// At least the seats in use (members plus pending invites).
    pub seats: u32,
    pub interval: BillingInterval,
}

/// Response of `POST …/billing/checkout` and `POST …/billing/portal` (admin):
/// a page of the billing provider to open in a browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingRedirect {
    pub url: String,
}
