//! `bssh ai token <team/agent> --ttl 1h`: a short-lived gateway token. Owner: W8.

use crate::cli::parser_ai::AiArgsToken;
use crate::config::AppConfig;
use crate::services::ai::gateway;
use crate::services::teams::agent::find_credential;
use anyhow::Result;

pub async fn execute(args: AiArgsToken, config: AppConfig) -> Result<()> {
    let ttl = gateway::parse_ttl(&args.ttl)?;
    let cred = find_credential(&config, &args.agent)?;
    let token = gateway::issue_token(&cred, ttl, chrono::Utc::now().timestamp())?;
    // Only the token on stdout, so it can be captured: TOKEN=$(bssh ai token ...)
    println!("{token}");
    eprintln!(
        "Valid {} for agent {} with policy '{}'. Send it as `Authorization: Bearer <token>`.",
        args.ttl,
        cred.id(),
        cred.policy
    );
    Ok(())
}
