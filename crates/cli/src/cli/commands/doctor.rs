use crate::config::AppConfig;
use crate::database::Database;
use crate::errors::SilentExit;
use crate::ui::{Glyph, Style, Tone, heading};
use anyhow::Result;
use std::env;
use std::path::PathBuf;

pub async fn execute(config: AppConfig) -> Result<()> {
    let style = Style::terminal();
    let checks = [
        check_configuration(&config),
        check_database(&config),
        check_ssh_client(),
        check_ssh_config(&config),
        check_ssh_agent(&config),
        check_kerberos(&config),
    ];

    let meta = format!("env: {}", config.environment);
    println!("{}", heading(&style, "bssh doctor", Some(&meta)));

    for check in &checks {
        print_check(&style, check);
    }

    let failed_count = checks
        .iter()
        .filter(|check| matches!(check.status, CheckStatus::Failed))
        .count();

    println!();
    if failed_count == 0 {
        println!(
            "{} Summary: no blocking problems found.",
            style.mark(Glyph::Ok)
        );
        Ok(())
    } else {
        println!(
            "{} Summary: {failed_count} blocking problem(s) found.",
            style.mark(Glyph::Bad)
        );
        Err(anyhow::Error::new(SilentExit))
    }
}

#[derive(Clone, Copy)]
enum CheckStatus {
    Ok,
    Warn,
    Failed,
}

struct HealthCheck {
    status: CheckStatus,
    label: &'static str,
    detail: String,
    suggestion: Option<String>,
}

fn ok(label: &'static str, detail: impl Into<String>) -> HealthCheck {
    HealthCheck {
        status: CheckStatus::Ok,
        label,
        detail: detail.into(),
        suggestion: None,
    }
}

fn warn(
    label: &'static str,
    detail: impl Into<String>,
    suggestion: impl Into<String>,
) -> HealthCheck {
    HealthCheck {
        status: CheckStatus::Warn,
        label,
        detail: detail.into(),
        suggestion: Some(suggestion.into()),
    }
}

fn failed(
    label: &'static str,
    detail: impl Into<String>,
    suggestion: impl Into<String>,
) -> HealthCheck {
    HealthCheck {
        status: CheckStatus::Failed,
        label,
        detail: detail.into(),
        suggestion: Some(suggestion.into()),
    }
}

fn print_check(style: &Style, check: &HealthCheck) {
    let (mark, badge) = match check.status {
        CheckStatus::Ok => (style.mark(Glyph::Ok), style.paint(Tone::Ok, "OK")),
        CheckStatus::Warn => (style.mark(Glyph::Ask), style.paint(Tone::Warn, "WARN")),
        CheckStatus::Failed => (style.mark(Glyph::Bad), style.paint(Tone::Bad, "FAILED")),
    };

    println!(
        "{} [{}] {}",
        mark,
        badge,
        style.paint(Tone::Strong, check.label)
    );
    println!("    {}", style.paint(Tone::Dim, &check.detail));
    if let Some(suggestion) = &check.suggestion {
        println!(
            "    {} {}",
            style.paint(Tone::Warn, "Suggestion:"),
            suggestion
        );
    }
}

fn check_configuration(config: &AppConfig) -> HealthCheck {
    ok(
        "Configuration",
        format!(
            "loaded environment '{}' with database {}",
            config.environment,
            config.database_path.display()
        ),
    )
}

fn check_database(config: &AppConfig) -> HealthCheck {
    match Database::new(config) {
        Ok(_) => ok(
            "Database",
            format!("initialized {}", config.database_path.display()),
        ),
        Err(error) => failed(
            "Database",
            format!(
                "could not initialize {}: {error}",
                config.database_path.display()
            ),
            "check that the database parent directory exists, is a directory, and is writable",
        ),
    }
}

fn check_ssh_client() -> HealthCheck {
    match find_command("ssh") {
        Some(path) => ok("SSH client", format!("found {}", path.display())),
        None => warn(
            "SSH client",
            "ssh was not found in PATH",
            "install OpenSSH client or add ssh to PATH before connecting",
        ),
    }
}

fn check_ssh_config(config: &AppConfig) -> HealthCheck {
    match &config.ssh_config_path {
        Some(path) if path.exists() => ok("SSH config", format!("found {}", path.display())),
        Some(path) => warn(
            "SSH config",
            format!("{} does not exist", path.display()),
            "create the file or run imports with bssh import --file <path>",
        ),
        None => warn(
            "SSH config",
            "no SSH config path is configured",
            "set ssh_config_path in the environment config or pass --file to import",
        ),
    }
}

fn check_ssh_agent(config: &AppConfig) -> HealthCheck {
    if !config.auth.use_agent {
        return ok("SSH agent", "disabled by config");
    }

    let socket = config
        .auth
        .agent_socket
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| env::var_os("SSH_AUTH_SOCK").map(PathBuf::from));

    match socket {
        Some(path) if path.exists() => ok("SSH agent", format!("socket found {}", path.display())),
        Some(path) => warn(
            "SSH agent",
            format!("configured socket {} does not exist", path.display()),
            "start ssh-agent or update auth.agent_socket",
        ),
        None => warn(
            "SSH agent",
            "SSH_AUTH_SOCK is not set",
            "start ssh-agent or disable auth.use_agent if you only use identity files",
        ),
    }
}

fn check_kerberos(config: &AppConfig) -> HealthCheck {
    if !config.use_kerberos_by_default {
        return ok("Kerberos", "disabled by default");
    }

    let has_klist = find_command("klist").is_some();
    let has_kinit = find_command("kinit").is_some();

    if has_klist && has_kinit {
        ok("Kerberos", "klist and kinit are available")
    } else {
        warn(
            "Kerberos",
            "Kerberos is enabled but klist or kinit is missing",
            "install Kerberos client tools or disable use_kerberos_by_default",
        )
    }
}

fn find_command(command: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
}
