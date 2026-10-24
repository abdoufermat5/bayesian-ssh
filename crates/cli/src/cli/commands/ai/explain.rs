//! `bssh ai explain`. Owner: W2.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsExplain, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai explain: not implemented yet")
}
