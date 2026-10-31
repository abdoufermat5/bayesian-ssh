//! The Teams passphrase, read from the environment or prompted for on the
//! terminal by every command that opens a session.

use anyhow::{bail, Result};
use zeroize::Zeroizing;

/// Env var read instead of prompting, for scripts.
const PASSPHRASE_ENV: &str = "BSSH_TEAMS_PASSPHRASE";

/// Which passphrase [`passphrase`] asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Passphrase {
    /// First setup: choose one, then repeat it.
    New,
    /// Opening keys that already exist.
    Existing,
}

/// Reads the Teams passphrase, from [`PASSPHRASE_ENV`] when it is set and
/// non-empty, else from the terminal.
///
/// # Errors
/// Fails when the terminal cannot be read, and for a new passphrase when it is
/// shorter than eight characters or the repetition differs.
pub(crate) fn passphrase(mode: Passphrase) -> Result<Zeroizing<String>> {
    if let Ok(p) = std::env::var(PASSPHRASE_ENV)
        && !p.is_empty()
    {
        return Ok(Zeroizing::new(p));
    }
    let prompt = match mode {
        Passphrase::New => "Choose a passphrase for this device's Teams keys: ",
        Passphrase::Existing => "Teams passphrase: ",
    };
    let p = Zeroizing::new(rpassword::prompt_password(prompt)?);
    if mode == Passphrase::New {
        if p.chars().count() < 8 {
            bail!("use at least 8 characters");
        }
        let again = Zeroizing::new(rpassword::prompt_password("Repeat it: ")?);
        if *again != *p {
            bail!("the passphrases don't match");
        }
    }
    Ok(p)
}
