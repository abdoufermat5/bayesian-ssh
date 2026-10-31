//! `bssh teams agent`: agent identities and their approval requests.

use super::passphrase::{passphrase, Passphrase};
use super::sync::print_sync;
use crate::cli::parser::TeamsAgentCommands;
use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::services::teams::relay::ApprovalAnswer;
use crate::services::teams::session::{self, Session};
use crate::services::teams::TeamsStore;
use anyhow::Result;

/// The approval request `bssh teams agent approve`/`deny` acts on.
struct Approval<'a> {
    team: &'a str,
    request: &'a str,
    reason: Option<String>,
}

/// Runs an `agent` subcommand.
///
/// # Errors
/// Fails when the team or agent cannot be resolved, or the server refuses the
/// change.
pub(super) async fn execute(
    command: TeamsAgentCommands,
    store: &TeamsStore,
    config: &AppConfig,
) -> Result<()> {
    let db = store.database();
    match command {
        TeamsAgentCommands::Add { team, name, policy } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
            let made = s
                .agent_add(team_id, &source.name, &name, policy.as_deref())
                .await?;
            println!(
                "✅ Agent '{name}' created in {} (takes a seat).",
                source.name
            );
            println!("   Policy '{}' signed by you.", made.policy);
            println!("   Device fingerprint {}", made.fingerprint);
            println!(
                "   Credential (mode 0600, holds its private keys): {}",
                made.credential_path.display()
            );
            println!();
            println!("Run it:   bssh ai serve --agent {}/{name}", source.name);
            println!("Token:    bssh ai token {}/{name} --ttl 1h", source.name);
        }
        TeamsAgentCommands::List { team } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
            let agents = s.agent_list(team_id).await?;
            if agents.is_empty() {
                println!(
                    "No agents in {}. Create one: bssh teams agent add {} <name>",
                    source.name, source.name
                );
            }
            for a in agents {
                println!(
                    "{:<24} {}  since {}",
                    a.name,
                    crate::services::teams::crypto::fingerprint(&a.device.ed25519_public),
                    a.created_at.format("%Y-%m-%d")
                );
            }
        }
        TeamsAgentCommands::Remove { team, name } => {
            let (team_id, source) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
            if !confirm(
                &format!(
                    "Remove agent '{name}' from {} and rotate the team key?",
                    source.name
                ),
                false,
            )? {
                return Ok(());
            }
            let sync = s.agent_remove(team_id, &name).await?;
            println!("✅ Agent '{name}' removed; its seat is free.");
            print_sync(&source.name, &sync);
        }
        TeamsAgentCommands::Policy { team, name, policy } => {
            let (team_id, _) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
            let v = s.agent_set_policy(team_id, &name, &policy).await?;
            println!("✅ Policy '{policy}' signed for '{name}' (version {v}).");
        }
        TeamsAgentCommands::Pending { team } => {
            let (team_id, _) = session::resolve_team(db, &team)?;
            let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
            let pending = s.pending_approvals(team_id).await?;
            if pending.is_empty() {
                println!("Nothing waiting for you.");
            }
            for r in pending {
                println!(
                    "{}  agent {}  tier {:?}  expires {}",
                    r.payload.request.request_id,
                    r.payload.agent,
                    r.payload.request.tier,
                    r.expires_at.format("%H:%M:%S")
                );
                println!(
                    "    {}",
                    crate::services::ai::teams::describe(&r.payload.request)
                );
            }
        }
        TeamsAgentCommands::Approve {
            team,
            request,
            reason,
        } => {
            let approval = Approval {
                team: &team,
                request: &request,
                reason,
            };
            decide(store, config, approval, ApprovalAnswer::Approve).await?;
        }
        TeamsAgentCommands::Deny {
            team,
            request,
            reason,
        } => {
            let approval = Approval {
                team: &team,
                request: &request,
                reason,
            };
            decide(store, config, approval, ApprovalAnswer::Deny).await?;
        }
    }
    Ok(())
}

/// Sends an approval or denial and prints what happens next.
///
/// # Errors
/// Fails when the team cannot be resolved or the server refuses the decision.
async fn decide(
    store: &TeamsStore,
    config: &AppConfig,
    approval: Approval<'_>,
    decision: ApprovalAnswer,
) -> Result<()> {
    let (team_id, _) = session::resolve_team(store.database(), approval.team)?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    let req = s
        .decide_approval(team_id, approval.request, decision, approval.reason)
        .await?;
    let r = &req.payload.request;
    let what = crate::services::ai::teams::describe(r);
    if decision == ApprovalAnswer::Deny {
        println!("🚫 Denied {} ({what}).", r.request_id);
    } else if r.tier == crate::services::ai::types::Tier::TwoPerson {
        // The gateway counts approvers: one admin approving twice still counts once.
        println!("✅ Your approval of {} ({what}) is sent.", r.request_id);
        println!("   It needs two admins: it runs once another admin approves too.");
    } else {
        println!(
            "✅ Approved {} ({what}): the agent's gateway runs it now.",
            r.request_id
        );
    }
    Ok(())
}
