//! The `bssh teams` entrypoint: opens the store and dispatches each subcommand
//! to the module that owns it.

use super::{agents, audit, billing, device, invite, members, registry, setup, sharing, sync};
use crate::cli::TeamsCommands;
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::session;
use anyhow::Result;
use bssh_teams_proto::api::{BillingInterval, Role};

/// Runs `bssh teams <command>` after opening the Teams store.
///
/// # Errors
/// Fails when the store cannot be opened or the command fails; session errors
/// gain a hint when [`session::explain`] knows one.
pub async fn execute(command: TeamsCommands, config: AppConfig) -> Result<()> {
    let store = TeamsStore::open(&config)?;
    run(command, &store, &config)
        .await
        .map_err(|e| match session::explain(&e) {
            Some(hint) => e.context(hint),
            None => e,
        })
}

async fn run(command: TeamsCommands, store: &TeamsStore, config: &AppConfig) -> Result<()> {
    match command {
        TeamsCommands::Init {
            server,
            email,
            device_name,
            invite,
            email_code,
        } => {
            setup::init(
                store,
                config,
                setup::InitRequest {
                    server,
                    email,
                    device_name,
                    invite,
                    email_code,
                },
            )
            .await
        }

        TeamsCommands::Whoami => setup::whoami(store, config).await,

        TeamsCommands::Create { name, org } => {
            registry::create(
                store,
                config,
                registry::CreateRequest {
                    name: &name,
                    org: org.as_deref(),
                },
            )
            .await
        }

        TeamsCommands::List => registry::list(store),

        TeamsCommands::Invite { team, email, admin } => {
            let role = if admin { Role::Admin } else { Role::Member };
            invite::invite(
                store,
                config,
                invite::InviteRequest {
                    team: &team,
                    email: &email,
                    role,
                },
            )
            .await
        }

        TeamsCommands::Join {
            token,
            team,
            fingerprint,
        } => {
            invite::join(
                store,
                config,
                invite::JoinRequest {
                    token,
                    team,
                    fingerprint,
                },
            )
            .await
        }

        TeamsCommands::Grant { team, fingerprint } => {
            invite::grant(
                store,
                config,
                invite::GrantRequest {
                    team: &team,
                    fingerprint: fingerprint.as_deref(),
                },
            )
            .await
        }

        TeamsCommands::Sync { team } => sync::execute(store, config, team.as_deref()).await,

        TeamsCommands::Share { connection, team } => {
            sharing::share(
                store,
                config,
                sharing::ShareRequest {
                    connection: &connection,
                    team: &team,
                },
            )
            .await
        }

        TeamsCommands::Unshare { connection } => sharing::unshare(store, config, &connection).await,

        TeamsCommands::Members { team } => members::members(store, config, &team).await,

        TeamsCommands::Uninvite { team, email } => {
            invite::uninvite(
                store,
                config,
                members::MemberRef {
                    team: &team,
                    email: &email,
                },
            )
            .await
        }

        TeamsCommands::Billing {
            team,
            checkout,
            seats,
            yearly,
            manage,
        } => {
            let open = if manage {
                Some(billing::BillingPage::Manage)
            } else if checkout {
                Some(billing::BillingPage::Checkout)
            } else {
                None
            };
            let interval = if yearly {
                BillingInterval::Year
            } else {
                BillingInterval::Month
            };
            billing::execute(
                store,
                config,
                billing::BillingRequest {
                    team: &team,
                    open,
                    seats,
                    interval,
                },
            )
            .await
        }

        TeamsCommands::Audit {
            team,
            host,
            member,
            since,
            limit,
            all,
        } => {
            let window = if all {
                audit::AuditWindow::All
            } else {
                audit::AuditWindow::Newest(limit)
            };
            audit::execute(
                store,
                config,
                audit::AuditRequest {
                    team: &team,
                    host: host.as_deref(),
                    member,
                    since: since.as_deref(),
                    window,
                },
            )
            .await
        }

        TeamsCommands::Device { command } => device::execute(command, store, config).await,

        TeamsCommands::Agent { command } => agents::execute(command, store, config).await,

        TeamsCommands::Remove { team, email } => {
            members::remove(
                store,
                config,
                members::MemberRef {
                    team: &team,
                    email: &email,
                },
            )
            .await
        }
    }
}
