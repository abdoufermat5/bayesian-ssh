/// Emit a human-friendly error plus (when possible) a remediation hint to
/// stderr. Hints are derived from well-known messages anywhere in the
/// error chain.
pub fn report_cli_error(error: &anyhow::Error) {
    eprintln!("Error: {error}");

    if let Some(suggestion) = suggestion_for(error) {
        eprintln!("Suggestion: {suggestion}");
    }
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

    None
}

fn error_contains(error: &anyhow::Error, needle: &str) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().to_lowercase().contains(needle))
}

#[cfg(test)]
mod tests {
    use super::suggestion_for;

    #[test]
    fn duplicate_name_error_gets_hint() {
        let err = anyhow::anyhow!("Duplicate connection name 'web' (existing id: 1)");
        assert!(suggestion_for(&err).unwrap().contains("bssh list"));
    }
}
