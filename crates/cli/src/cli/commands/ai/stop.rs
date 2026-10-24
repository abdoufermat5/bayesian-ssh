//! `bssh ai stop`. Owner: W3.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsStop, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai stop: not implemented yet")
}
