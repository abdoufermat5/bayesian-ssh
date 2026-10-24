//! `bssh ai policy`. Owner: W2.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsPolicy, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai policy: not implemented yet")
}
