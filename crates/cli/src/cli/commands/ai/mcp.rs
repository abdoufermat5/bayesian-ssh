//! `bssh ai mcp`. Owner: W4.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsMcp, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai mcp: not implemented yet")
}
