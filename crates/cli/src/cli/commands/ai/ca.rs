//! `bssh ai ca`. Owner: W5.

use crate::config::AppConfig;
use anyhow::{bail, Result};

pub async fn execute(args: crate::cli::parser_ai::AiArgsCa, config: AppConfig) -> Result<()> {
    let _ = (args, config);
    bail!("bssh ai ca: not implemented yet")
}
