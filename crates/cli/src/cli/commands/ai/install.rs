//! `bssh ai install`. Owner: W6.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsInstall, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai install: not implemented yet")
}
