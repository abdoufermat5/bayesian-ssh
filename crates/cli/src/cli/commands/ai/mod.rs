//! `bssh ai ...` dispatch. One file per subcommand; see each file's owner.

use crate::cli::parser_ai::AiCommands;
use crate::config::AppConfig;
use anyhow::Result;

pub mod approve;
pub mod ca;
pub mod doctor;
pub mod explain;
pub mod install;
pub mod log;
pub mod mcp;
pub mod policy;
pub mod replay;
pub mod serve;
pub mod setup;
pub mod status;
pub mod stop;
pub mod token;

pub async fn execute(command: AiCommands, config: AppConfig) -> Result<()> {
    match command {
        AiCommands::Mcp(a) => mcp::execute(a, config).await,
        AiCommands::Explain(a) => explain::execute(a, config).await,
        AiCommands::Policy(a) => policy::execute(a, config).await,
        AiCommands::Approve(a) => approve::execute(a, config).await,
        AiCommands::Stop(a) => stop::execute(a, config).await,
        AiCommands::Status(a) => status::execute(a, config).await,
        AiCommands::Log(a) => log::execute(a, config).await,
        AiCommands::Replay(a) => replay::execute(a, config).await,
        AiCommands::Install(a) => install::execute(a, config).await,
        AiCommands::Setup(a) => setup::execute(a, config).await,
        AiCommands::Doctor(a) => doctor::execute(a, config).await,
        AiCommands::Ca(a) => ca::execute(a, config).await,
        AiCommands::Serve(a) => serve::execute(a, config).await,
        AiCommands::Token(a) => token::execute(a, config).await,
    }
}
