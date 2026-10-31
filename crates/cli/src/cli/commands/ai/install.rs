//! `bssh ai install`.

use crate::cli::parser_ai::AiArgsInstall;
use crate::config::AppConfig;
use crate::services::ai::install::{run_install, Client, InstallOptions};
use crate::services::ai::policy;
use crate::services::ai::types::Profile;
use anyhow::{anyhow, Context, Result};

pub async fn execute(args: AiArgsInstall, config: AppConfig) -> Result<()> {
    let client = Client::parse(&args.client)?;
    let home = dirs::home_dir().ok_or_else(|| anyhow!("cannot find your home directory"))?;
    let bin = std::env::current_exe().context("cannot find the bssh binary path")?;
    let ai_dir = crate::services::ai::ai_dir(&config);
    // Installing an entry for a policy that does not exist would only fail later, inside the
    // agent; say so now. Uninstalling does not need one.
    let profile = if args.uninstall {
        Profile::Observe
    } else {
        policy::resolve(&config, &args.policy)
            .map_err(|e| anyhow!("policy '{}': {e}", args.policy))?
            .profile
    };

    // Hosts known to bssh, for the guard's fallback snapshot.
    let mut known = Vec::new();
    if args.guard && !args.uninstall {
        match crate::services::SshService::new(config.clone()) {
            Ok(svc) => {
                if let Ok(conns) = svc.list_connections(None, false).await {
                    for c in conns {
                        known.push(c.name);
                        known.push(c.host);
                    }
                    known.sort();
                    known.dedup();
                }
            }
            Err(e) => tracing::warn!("could not read connections for the guard snapshot: {e}"),
        }
    }

    let report = run_install(&InstallOptions {
        client,
        policy: &args.policy,
        profile,
        environment: &config.environment,
        approval_timeout_secs: config.ai.approval_timeout_secs,
        guard: args.guard,
        dry_run: args.dry_run,
        uninstall: args.uninstall,
        home: &home,
        bssh_bin: &bin,
        ai_dir: &ai_dir,
        known_hosts: &known,
    })?;
    print!("{report}");
    Ok(())
}
