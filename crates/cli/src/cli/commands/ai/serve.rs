//! `bssh ai serve`. Owner: W8.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsServe, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai serve: not implemented yet")
}
