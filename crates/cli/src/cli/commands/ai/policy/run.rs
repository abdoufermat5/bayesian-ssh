//! `bssh ai policy <list|show|set|test>` dispatch.

use super::evaluate::{TestRun, run_test};
use super::render::{render_list, render_show};
use super::set::{ScopeArgs, SetPolicy, set_policy};
use crate::cli::parser_ai::{AiArgsPolicy, AiPolicyCommands};
use crate::config::AppConfig;
use crate::services::ai::policy;
use crate::services::ai::ui::{Style, Tone};
use anyhow::Result;

/// Runs one `bssh ai policy` subcommand.
///
/// # Errors
///
/// Returns the subcommand's error: an unknown policy or profile, an invalid test file, a
/// policy that would reach no host, or a failed config write.
pub async fn execute(args: AiArgsPolicy, mut config: AppConfig) -> Result<()> {
    match args.command {
        AiPolicyCommands::List => {
            print!("{}", render_list(&Style::terminal(), &config.ai.policies));
            Ok(())
        }
        AiPolicyCommands::Set {
            name,
            profile,
            tag,
            host,
            all_hosts,
        } => set_policy(
            &Style::terminal(),
            &mut config,
            SetPolicy {
                name: &name,
                profile: profile.as_deref(),
                scope: ScopeArgs {
                    tags: tag,
                    hosts: host,
                    all_hosts,
                },
            },
        ),
        AiPolicyCommands::Show { name } => {
            let policy = policy::resolve(&config, &name)
                .map_err(|e| anyhow::anyhow!("policy '{name}': {e}"))?;
            print!(
                "{}",
                render_show(
                    &Style::terminal(),
                    &name,
                    &policy,
                    &config.ai.trusted_tag_sources
                )
            );
            let warnings = policy::validation_warnings(&config, &name);
            if !warnings.is_empty() {
                let style = Style::terminal();
                println!("\n{}", style.paint(Tone::Strong, "Warnings"));
                for warning in warnings {
                    println!("  {}  {warning}", style.paint(Tone::Warn, "warn"));
                }
            }
            Ok(())
        }
        AiPolicyCommands::Test { file, policy, host } => {
            run_test(
                &Style::terminal(),
                &config,
                TestRun {
                    file: &file,
                    policy: &policy,
                    host: host.as_deref(),
                },
            )
            .await
        }
    }
}
