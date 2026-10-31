//! Teams billing commands.

//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.

use super::state::{with_session, TeamsState};
use super::team::resolve;
use crate::commands::CommandError;
use tauri::State;

#[tauri::command(async)]
pub fn teams_billing(
    state: State<'_, TeamsState>,
    team: String,
) -> Result<bssh_teams_proto::api::BillingInfo, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { s.billing(resolve(&team)?).await })
    })
}

/// A Stripe Checkout URL for the team (admin); the page opens it in the browser.
/// Without `seats`, the seats in use, at least one above the free plan.
#[tauri::command(async)]
pub fn teams_billing_checkout(
    state: State<'_, TeamsState>,
    team: String,
    seats: Option<u32>,
    yearly: bool,
) -> Result<String, CommandError> {
    use bssh_teams_proto::api::BillingInterval;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let team_id = resolve(&team)?;
            let seats = match seats {
                Some(n) => n,
                None => {
                    let info = s.billing(team_id).await?;
                    info.seats_used.max(info.free_seats + 1)
                }
            };
            let interval = if yearly {
                BillingInterval::Year
            } else {
                BillingInterval::Month
            };
            s.checkout_url(team_id, seats, interval).await
        })
    })
}

/// The Stripe Customer Portal URL of the team (admin).
#[tauri::command(async)]
pub fn teams_billing_portal(
    state: State<'_, TeamsState>,
    team: String,
) -> Result<String, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { s.portal_url(resolve(&team)?).await })
    })
}
