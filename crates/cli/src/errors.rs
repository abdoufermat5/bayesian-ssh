use crate::ui::{Style, Tone};

/// Marker error for commands that have already rendered their own structured
/// failure output and only need `main` to exit with status code 1 without
/// printing a redundant `error:` line.
#[derive(Debug)]
pub struct SilentExit;

impl std::fmt::Display for SilentExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("command exited with failure")
    }
}

impl std::error::Error for SilentExit {}

/// Emit a styled, human-friendly error plus any wrapped causes and (when
/// possible) a remediation hint to stderr. Hints are derived from well-known
/// messages anywhere in the error chain.
pub fn report_cli_error(error: &anyhow::Error) {
    let style = Style::stderr();
    eprintln!("{}", format_cli_error(error, &style));
}

fn format_cli_error(error: &anyhow::Error, style: &Style) -> String {
    let error_label = if style.color {
        "\x1b[1;31merror:\x1b[0m"
    } else {
        "error:"
    };

    let mut lines = vec![format!("{error_label} {error}")];

    for cause in error.chain().skip(1) {
        let caused_by = style.paint(Tone::Dim, "caused by:");
        lines.push(format!("  {caused_by} {cause}"));
    }

    if let Some(suggestion) = suggestion_for(error) {
        let hint_label = style.paint(Tone::Info, "hint:");
        lines.push(format!("  {hint_label} {suggestion}"));
    }

    lines.join("\n")
}

fn suggestion_for(error: &anyhow::Error) -> Option<&'static str> {
    if error_contains(error, "database file does not exist") {
        return Some("run `bssh doctor` to initialize and inspect the active environment");
    }

    if error_contains(error, "backup file does not exist") {
        return Some("check the backup path or run `bssh backup` to create a new backup first");
    }

    // Needles must be lower-case: `error_contains` lower-cases the haystack.
    if error_contains(error, "duplicate connection") {
        return Some(
            "run `bssh list` to see existing names or `bssh edit <name> --name <new>` to rename",
        );
    }

    if error_contains(error, "connection") && error_contains(error, "not found") {
        return Some(
            "run `bssh list` to see saved connections or `bssh add <name> <host>` to create one",
        );
    }

    if error_contains(error, "permission denied") {
        return Some("check file permissions and ownership for the path shown above");
    }

    if error_contains(error, "no command supplied") {
        return Some("pass a remote command after `--`, e.g. `bssh exec web-prod -- uptime`");
    }

    if error_contains(error, "invalid encrypted payload")
        || error_contains(error, "wrong passphrase")
        || error_contains(error, "corrupted file")
    {
        return Some("check the passphrase or re-export with `bssh export --passphrase <secret>`");
    }

    if error_contains(error, "interactive terminal required")
        || error_contains(error, "stdin is not a tty")
    {
        return Some("pass an exact connection name or `--force` when running non-interactively");
    }

    if error_contains(error, "connection refused") {
        return Some(
            "verify the host and port are reachable and sshd is running (`bssh ping <name>`)",
        );
    }

    if error_contains(error, "no route to host")
        || error_contains(error, "failed to lookup address")
    {
        return Some(
            "check your network or VPN connection and inspect the host with `bssh show <name>`",
        );
    }

    if error_contains(error, "host key")
        && (error_contains(error, "mismatch")
            || error_contains(error, "changed")
            || error_contains(error, "verification failed"))
    {
        return Some(
            "inspect the host key change carefully; check `~/.ssh/known_hosts` or run `bssh audit`",
        );
    }

    if error_contains(error, "kerberos")
        && (error_contains(error, "expired") || error_contains(error, "no credentials"))
    {
        return Some("run `kinit` to obtain or renew your Kerberos ticket");
    }

    None
}

fn error_contains(error: &anyhow::Error, needle: &str) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().to_lowercase().contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{SilentExit, format_cli_error, suggestion_for};
    use crate::ui::{Style, strip_ansi};

    #[test]
    fn duplicate_name_error_gets_hint() {
        let err = anyhow::anyhow!("Duplicate connection name 'web' (existing id: 1)");
        assert!(suggestion_for(&err).unwrap().contains("bssh list"));
    }

    #[test]
    fn formats_error_causes_and_hint() {
        let inner = anyhow::anyhow!("Database file does not exist at /tmp/db");
        let wrapped = inner.context("Failed to create backup");
        let rendered = format_cli_error(&wrapped, &Style::plain());
        assert_eq!(
            rendered,
            "error: Failed to create backup\n  \
             caused by: Database file does not exist at /tmp/db\n  \
             hint: run `bssh doctor` to initialize and inspect the active environment"
        );

        let colored = format_cli_error(
            &wrapped,
            &Style {
                color: true,
                unicode: true,
            },
        );
        assert!(colored.starts_with("\x1b[1;31merror:\x1b[0m "));
        assert_eq!(strip_ansi(&colored), rendered);
    }

    #[test]
    fn silent_exit_is_downcastable_from_anyhow() {
        let err: anyhow::Error = SilentExit.into();
        assert!(err.downcast_ref::<SilentExit>().is_some());
    }
}
