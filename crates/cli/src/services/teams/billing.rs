//! What a team's billing state means for the user: why a team is read-only
//! and what to do about it. Pure, derived from the server's `BillingInfo`.

use bssh_teams_proto::api::BillingInfo;

/// Why a team refuses changes (the server's `read_only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyReason {
    /// A live subscription whose paid seats are fewer than the members.
    NotEnoughSeats,
    /// No live subscription and more members than the free seats.
    NoSubscription,
}

/// True for the statuses of a subscription that still holds its seats:
/// `past_due` means a payment failed and Stripe is retrying.
pub fn is_live(status: Option<&str>) -> bool {
    matches!(status, Some("active" | "trialing" | "past_due"))
}

/// `None` when the team accepts changes. The member count isn't needed:
/// `seats_used` includes pending invites, and the status tells the two cases apart.
pub fn read_only_reason(info: &BillingInfo) -> Option<ReadOnlyReason> {
    if !info.read_only {
        return None;
    }
    Some(if is_live(info.status.as_deref()) {
        ReadOnlyReason::NotEnoughSeats
    } else {
        ReadOnlyReason::NoSubscription
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bssh_teams_proto::api::Plan;

    fn info(status: Option<&str>, read_only: bool) -> BillingInfo {
        BillingInfo {
            billing_enabled: true,
            plan: if status.is_some() {
                Plan::Team
            } else {
                Plan::Free
            },
            free_seats: 3,
            paid_seats: 2,
            seat_limit: 2,
            seats_used: 4,
            status: status.map(str::to_string),
            interval: None,
            current_period_end: None,
            cancel_at_period_end: false,
            read_only,
            org_id: None,
        }
    }

    #[test]
    fn writable_team_has_no_reason() {
        assert_eq!(read_only_reason(&info(Some("active"), false)), None);
        assert_eq!(read_only_reason(&info(None, false)), None);
    }

    #[test]
    fn live_subscription_means_not_enough_seats() {
        for status in ["active", "trialing", "past_due"] {
            assert_eq!(
                read_only_reason(&info(Some(status), true)),
                Some(ReadOnlyReason::NotEnoughSeats),
                "{status}"
            );
        }
    }

    #[test]
    fn no_live_subscription_means_no_subscription() {
        for status in [
            None,
            Some("canceled"),
            Some("unpaid"),
            Some("incomplete_expired"),
        ] {
            assert_eq!(
                read_only_reason(&info(status, true)),
                Some(ReadOnlyReason::NoSubscription),
                "{status:?}"
            );
        }
    }
}
