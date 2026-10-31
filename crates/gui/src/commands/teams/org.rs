//! Organization commands and their wire shapes.

//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.

use super::state::{open_store, with_session, TeamsState};
use crate::commands::CommandError;
use bayesian_ssh::services::teams::client::TeamsError;
use bayesian_ssh::services::teams::orgs;
use bayesian_ssh::services::teams::session::{self};
use bssh_teams_proto::api::{OrgInfo, OrgRole};
use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::State;

// ── Organizations ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct OrgTeamDto {
    pub(super) team_id: String,
    /// The local name when this machine knows the team.
    pub(super) name: Option<String>,
    pub(super) short_id: String,
}

#[derive(Serialize)]
pub struct OrgOwnerDto {
    pub(super) account_id: String,
    pub(super) email: String,
}

#[derive(Serialize)]
pub struct OrgDto {
    pub(super) org_id: String,
    pub(super) short_id: String,
    /// The owners' emails: organizations have no name on the server.
    pub(super) label: String,
    pub(super) my_role: OrgRole,
    pub(super) owners: Vec<OrgOwnerDto>,
    pub(super) teams: Vec<OrgTeamDto>,
    pub(super) created_at: DateTime<Utc>,
}

pub(super) fn org_dto(org: &OrgInfo, local: &[(uuid::Uuid, String)]) -> OrgDto {
    OrgDto {
        org_id: org.org_id.to_string(),
        short_id: orgs::short_id(&org.org_id),
        label: orgs::label(org),
        my_role: org.my_role,
        owners: org
            .owners
            .iter()
            .map(|o| OrgOwnerDto {
                account_id: o.account_id.to_string(),
                email: o.email.clone(),
            })
            .collect(),
        teams: org
            .team_ids
            .iter()
            .map(|id| OrgTeamDto {
                team_id: id.to_string(),
                name: local.iter().find(|(t, _)| t == id).map(|(_, n)| n.clone()),
                short_id: orgs::short_id(id),
            })
            .collect(),
        created_at: org.created_at,
    }
}

pub(super) fn local_names() -> anyhow::Result<Vec<(uuid::Uuid, String)>> {
    let (store, _) = open_store()?;
    session::local_team_names(store.database())
}

pub(super) fn parse_org(org: &str) -> Result<uuid::Uuid, CommandError> {
    uuid::Uuid::parse_str(org.trim()).map_err(|_| CommandError::from("Unknown organization id."))
}

pub(super) fn is_org_unsupported(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<TeamsError>(),
        Some(TeamsError::OrgUnsupported)
    )
}

/// The organizations this account owns or has a team in. `None` when the
/// server has no organizations (an older server): the page hides that UI.
#[tauri::command(async)]
pub fn teams_orgs(state: State<'_, TeamsState>) -> Result<Option<Vec<OrgDto>>, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            match s.list_orgs().await {
                Ok(list) => {
                    let local = local_names()?;
                    Ok(Some(list.iter().map(|o| org_dto(o, &local)).collect()))
                }
                Err(e) if is_org_unsupported(&e) => Ok(None),
                Err(e) => Err(e),
            }
        })
    })
}

#[tauri::command(async)]
pub fn teams_org(state: State<'_, TeamsState>, org: String) -> Result<OrgDto, CommandError> {
    let org_id = parse_org(&org)?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { Ok(org_dto(&s.org_info(org_id).await?, &local_names()?)) })
    })
}

#[tauri::command(async)]
pub fn teams_org_create(state: State<'_, TeamsState>) -> Result<OrgDto, CommandError> {
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { Ok(org_dto(&s.create_org().await?, &local_names()?)) })
    })
}

#[tauri::command(async)]
pub fn teams_org_add_owner(
    state: State<'_, TeamsState>,
    org: String,
    email: String,
) -> Result<OrgDto, CommandError> {
    let org_id = parse_org(&org)?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            Ok(org_dto(
                &s.add_org_owner(org_id, &email).await?,
                &local_names()?,
            ))
        })
    })
}

/// Remove an owner by email. Returns the organization as it is afterwards.
#[tauri::command(async)]
pub fn teams_org_remove_owner(
    state: State<'_, TeamsState>,
    org: String,
    email: String,
) -> Result<OrgDto, CommandError> {
    let org_id = parse_org(&org)?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            s.remove_org_owner(org_id, &email).await?;
            Ok(org_dto(&s.org_info(org_id).await?, &local_names()?))
        })
    })
}

#[tauri::command(async)]
pub fn teams_org_billing(
    state: State<'_, TeamsState>,
    org: String,
) -> Result<bssh_teams_proto::api::BillingInfo, CommandError> {
    let org_id = parse_org(&org)?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { s.org_billing(org_id).await })
    })
}

/// A Stripe Checkout URL for the organization (owner); the page opens it in the browser.
#[tauri::command(async)]
pub fn teams_org_billing_checkout(
    state: State<'_, TeamsState>,
    org: String,
    seats: Option<u32>,
    yearly: bool,
) -> Result<String, CommandError> {
    use bssh_teams_proto::api::BillingInterval;
    let org_id = parse_org(&org)?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move {
            let seats = match seats {
                Some(n) => n,
                None => {
                    let info = s.org_billing(org_id).await?;
                    info.seats_used.max(info.free_seats + 1)
                }
            };
            let interval = if yearly {
                BillingInterval::Year
            } else {
                BillingInterval::Month
            };
            s.org_checkout_url(org_id, seats, interval).await
        })
    })
}

/// The Stripe Customer Portal URL of the organization (owner).
#[tauri::command(async)]
pub fn teams_org_billing_portal(
    state: State<'_, TeamsState>,
    org: String,
) -> Result<String, CommandError> {
    let org_id = parse_org(&org)?;
    with_session(state.unlocked()?, move |s| {
        Box::pin(async move { s.org_portal_url(org_id).await })
    })
}
