//! `bssh teams init` and `bssh teams whoami`: this machine's account, device
//! and keys.

use super::passphrase::{Passphrase, passphrase};
use crate::config::AppConfig;
use crate::services::teams::TeamsStore;
use crate::services::teams::session::{self, Session};
use anyhow::{Result, anyhow, bail};
use std::io::{self, IsTerminal, Write};

/// The arguments of `bssh teams init`.
pub(super) struct InitRequest {
    pub server: String,
    pub email: String,
    pub device_name: Option<String>,
    pub invite: Option<String>,
    pub email_code: Option<String>,
}

/// Registers this machine, verifies the email when the server asks, and prints
/// the next steps.
///
/// # Errors
/// Fails when the account cannot be created, the email code is missing, or the
/// identity cannot be unlocked afterwards.
pub(super) async fn init(
    store: &TeamsStore,
    config: &AppConfig,
    request: InitRequest,
) -> Result<()> {
    let pass = passphrase(Passphrase::New)?;
    let name = request.device_name.unwrap_or_else(default_device_name);
    // Verifies the email on a terminal when the server asks for it;
    // without one the error says to rerun with `--email-code`.
    let identity = session::init_interactive(
        store,
        &session::InitParams {
            server_url: &request.server,
            email: &request.email,
            device_name: &name,
            passphrase: &pass,
            invite_token: request.invite.as_deref(),
            email_code: request.email_code.as_deref(),
        },
        &mut TerminalEmailCodePrompt::new(),
    )
    .await?;
    let s = Session::open(store, config, &pass)?;
    println!(
        "✅ Teams is set up for {} on {}",
        identity.email, identity.server_url
    );
    println!("   Device '{name}', fingerprint {}", s.my_fingerprint());
    println!();
    println!("Next: `bssh teams create <name>`, or join a team you were invited to with");
    println!("      `bssh teams join <TOKEN> --fingerprint <ADMIN FINGERPRINT>`.");
    Ok(())
}

/// Prints this machine's account, device and fingerprint.
///
/// # Errors
/// Fails when Teams is not set up here or the identity cannot be unlocked.
pub(super) async fn whoami(store: &TeamsStore, config: &AppConfig) -> Result<()> {
    let db = store.database();
    let identity = session::load_identity(db)?
        .ok_or_else(|| anyhow!("Teams isn't set up on this machine (`bssh teams init`)"))?;
    let s = Session::open(store, config, &passphrase(Passphrase::Existing)?)?;
    println!("Account:     {}", identity.email);
    println!("Server:      {}", identity.server_url);
    println!("Device:      {}", identity.device_id);
    println!("Fingerprint: {}", s.my_fingerprint());
    Ok(())
}

fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "device".to_string())
}

/// Whether a terminal is attached, checked once when the prompt is built.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalMode {
    /// Both stdin and stdout are terminals: prompt for the code.
    Interactive,
    /// No terminal: report non-interactive so the server mails the code.
    Headless,
}

/// Asks for the code that verifies the account's email, on the terminal only.
/// An empty line means "mail me a new code" (see [`session::EmailCodePrompt`]).
struct TerminalEmailCodePrompt {
    mode: TerminalMode,
}

impl TerminalEmailCodePrompt {
    fn new() -> Self {
        let mode = if io::stdin().is_terminal() && io::stdout().is_terminal() {
            TerminalMode::Interactive
        } else {
            TerminalMode::Headless
        };
        Self { mode }
    }
}

impl session::EmailCodePrompt for TerminalEmailCodePrompt {
    fn terminal(&self) -> bool {
        self.mode == TerminalMode::Interactive
    }

    fn read_code(&mut self, notice: Option<&str>) -> Result<Option<String>> {
        if let Some(notice) = notice {
            println!("{notice}");
        }
        print!("Verification code (Enter to get a new one): ");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().read_line(&mut line)? == 0 {
            // A closed stdin never means consent: don't mail codes in a loop.
            bail!("no verification code was entered; rerun with `--email-code <CODE>`");
        }
        let code = line.trim();
        Ok((!code.is_empty()).then(|| code.to_string()))
    }
}
