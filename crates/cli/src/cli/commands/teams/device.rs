//! `bssh teams device`: revoking a device for good.

use super::passphrase::{Passphrase, passphrase};
use crate::cli::parser::TeamsDeviceCommands;
use crate::cli::utils::confirm;
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::session::{self, Session};
use anyhow::{Result, anyhow};

/// Revokes the device named on the command line, confirming first.
///
/// # Errors
/// Fails when Teams is not set up on this machine, the confirmation prompt
/// cannot be read, or the server refuses the revocation.
pub(super) async fn execute(
    command: TeamsDeviceCommands,
    store: &TeamsStore,
    config: &AppConfig,
) -> Result<()> {
    let db = store.database();
    match command {
        TeamsDeviceCommands::Revoke { device_id } => {
            let identity = session::load_identity(db)?
                .ok_or_else(|| anyhow!("Teams isn't set up on this machine (`bssh teams init`)"))?;
            let this_device = identity.device_id == device_id;
            let prompt = if this_device {
                format!(
                    "Revoke THIS device ({device_id})? It is locked out for good and this \
                     machine's Teams identity is removed. This can't be undone."
                )
            } else {
                format!(
                    "Revoke device {device_id}? It is locked out for good. This can't be undone."
                )
            };
            if !confirm(&prompt, false)? {
                println!("Cancelled.");
                return Ok(());
            }
            let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
            let removed_identity = s.revoke_device(device_id).await?;
            println!(
                "🚫 Revoked device {device_id}: it can no longer sign in or sync, and it stays \
                 revoked."
            );
            println!("{}", rotation_hint(&session::local_team_names(db)?));
            if removed_identity {
                println!(
                    "   This machine's Teams identity is removed. Team connections you already \
                     synced stay as local connections."
                );
            }
        }
    }
    Ok(())
}

/// What follows a revocation: the affected teams must move to a new team key,
/// which an admin's sync does. Names the teams this machine knows, since the
/// server decides which are affected.
fn rotation_hint(teams: &[(uuid::Uuid, String)]) -> String {
    let mut text = String::from(
        "   Every team the device's account is in now needs a new team key, so the device can't \
         read what is written from now on. An admin posts it the next time they run \
         `bssh teams sync`",
    );
    match teams {
        [] => text.push('.'),
        [(_, name)] => text.push_str(&format!(
            "; as an admin, run it now: `bssh teams sync {name}`."
        )),
        _ => text.push_str("; as an admin, run it now: `bssh teams sync` (all teams)."),
    }
    text
}
