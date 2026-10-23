//! `bssh org`: organizations as seen from this machine. An organization has
//! no name on the server, so it is labelled by its owners' emails and `<org>`
//! arguments accept an id prefix or one of its team names.

use crate::cli::commands::teams::{open_in_browser, passphrase, print_billing};
use crate::cli::{OrgCommands, OrgOwnerCommands};
use crate::config::AppConfig;
use crate::services::teams::orgs;
use crate::services::teams::session::{self, Session};
use crate::services::teams::TeamsStore;
use anyhow::Result;
use bssh_teams_proto::api::{BillingInterval, OrgInfo, OrgRole};
use uuid::Uuid;

pub async fn execute(command: OrgCommands, config: AppConfig) -> Result<()> {
    let store = TeamsStore::open(&config)?;
    run(command, &store, &config)
        .await
        .map_err(|e| match session::explain(&e) {
            Some(hint) => e.context(hint),
            None => e,
        })
}

async fn run(command: OrgCommands, store: &TeamsStore, config: &AppConfig) -> Result<()> {
    let db = store.database();
    match command {
        OrgCommands::List => {
            let s = Session::open(store, config, &passphrase(false)?)?;
            let visible = s.list_orgs().await?;
            print_list(&visible, &session::local_team_names(db)?);
        }

        OrgCommands::Show { org } => {
            let s = Session::open(store, config, &passphrase(false)?)?;
            let visible = s.list_orgs().await?;
            let local = session::local_team_names(db)?;
            let org_id = orgs::select(&visible, org.as_deref(), &local)?;
            let info = s.org_info(org_id).await?;
            print_show(&info, &local);
            println!();
            print_billing(
                &orgs::label(&info),
                &s.org_billing(org_id).await?,
                &format!("bssh org billing {}", orgs::short_id(&org_id)),
            );
        }

        OrgCommands::Create => {
            let s = Session::open(store, config, &passphrase(false)?)?;
            let org = s.create_org().await?;
            println!(
                "✅ Created organization {} ({}).",
                orgs::label(&org),
                orgs::short_id(&org.org_id)
            );
            println!("   You're its owner.");
            println!();
            println!(
                "Create a team in it:  bssh teams create <name> --org {}",
                orgs::short_id(&org.org_id)
            );
        }

        OrgCommands::Owners { command } => {
            let s = Session::open(store, config, &passphrase(false)?)?;
            let visible = s.list_orgs().await?;
            let local = session::local_team_names(db)?;
            match command {
                OrgOwnerCommands::Add { org, email } => {
                    let org_id = orgs::select(&visible, Some(&org), &local)?;
                    let updated = s.add_org_owner(org_id, &email).await?;
                    println!(
                        "✅ {} is now an owner of organization {}.",
                        email.trim().to_lowercase(),
                        orgs::label(&updated)
                    );
                    println!(
                        "   Owners manage billing and owners, and create teams in the organization."
                    );
                }
                OrgOwnerCommands::Remove { org, email } => {
                    let org_id = orgs::select(&visible, Some(&org), &local)?;
                    let updated = s.remove_org_owner(org_id, &email).await?;
                    println!(
                        "✅ Removed {} as an owner of organization {}.",
                        email.trim().to_lowercase(),
                        orgs::label(&updated)
                    );
                }
            }
        }

        OrgCommands::Billing {
            org,
            checkout,
            seats,
            yearly,
            manage,
        } => {
            let s = Session::open(store, config, &passphrase(false)?)?;
            let visible = s.list_orgs().await?;
            let local = session::local_team_names(db)?;
            let org_id = orgs::select(&visible, org.as_deref(), &local)?;
            let label = orgs::label(
                visible
                    .iter()
                    .find(|o| o.org_id == org_id)
                    .expect("selected from the list"),
            );
            if checkout || manage {
                let url = if manage {
                    s.org_portal_url(org_id).await?
                } else {
                    let info = s.org_billing(org_id).await?;
                    let seats = seats.unwrap_or_else(|| info.seats_used.max(info.free_seats + 1));
                    let interval = if yearly {
                        BillingInterval::Year
                    } else {
                        BillingInterval::Month
                    };
                    s.org_checkout_url(org_id, seats, interval).await?
                };
                println!("Opening the billing page for organization {label} in your browser:");
                println!("  {url}");
                open_in_browser(&url);
                return Ok(());
            }
            print_billing(
                &label,
                &s.org_billing(org_id).await?,
                &format!("bssh org billing {}", orgs::short_id(&org_id)),
            );
        }
    }
    Ok(())
}

fn role_str(role: OrgRole) -> &'static str {
    match role {
        OrgRole::Owner => "owner",
        OrgRole::Member => "member",
    }
}

fn print_list(visible: &[OrgInfo], local: &[(Uuid, String)]) {
    if visible.is_empty() {
        println!("No organizations yet. Create one with `bssh org create`.");
        return;
    }
    println!("👥 Organizations");
    println!(
        "{:<52} {:<7} {:>5}  TEAMS ON THIS MACHINE",
        "LABEL", "ROLE", "TEAMS"
    );
    for org in visible {
        let mine: Vec<&str> = org
            .team_ids
            .iter()
            .filter_map(|id| {
                local
                    .iter()
                    .find(|(local_id, _)| local_id == id)
                    .map(|(_, name)| name.as_str())
            })
            .collect();
        println!(
            "{:<52} {:<7} {:>5}  {}",
            crate::cli::utils::truncate_display(&orgs::label(org), 52),
            role_str(org.my_role),
            org.team_ids.len(),
            if mine.is_empty() {
                "-".to_string()
            } else {
                mine.join(", ")
            }
        );
    }
}

fn print_show(org: &OrgInfo, local: &[(Uuid, String)]) {
    println!("👥 Organization {}", orgs::label(org));
    println!("   Id:        {}", org.org_id);
    println!("   Your role: {}", role_str(org.my_role));
    println!("   Created:   {}", org.created_at.format("%Y-%m-%d"));
    if org.owners.is_empty() {
        println!("   Owners:    none listed");
    } else {
        println!("   Owners:");
        for owner in &org.owners {
            println!("     {}", owner.email);
        }
    }
    if org.team_ids.is_empty() {
        println!(
            "   Teams:     none yet; create one with `bssh teams create <name> --org {}`.",
            orgs::short_id(&org.org_id)
        );
    } else {
        println!("   Teams ({}):", org.team_ids.len());
        for team_id in &org.team_ids {
            println!("     {}", orgs::team_label(*team_id, local));
        }
    }
}
