//! `bssh ai setup`. Owner: W5.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsSetup, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai setup: not implemented yet")
}
