//! `bssh ai replay`. Owner: W6.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsReplay, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai replay: not implemented yet")
}
