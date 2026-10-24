//! `bssh ai token`. Owner: W8.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsToken, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai token: not implemented yet")
}
