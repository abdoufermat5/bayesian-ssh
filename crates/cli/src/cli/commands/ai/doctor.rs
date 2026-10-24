//! `bssh ai doctor`. Owner: W5.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsDoctor, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai doctor: not implemented yet")
}
