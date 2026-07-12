use std::process::{Command, Output};

fn run_bssh(env_name: &str, args: &[&str]) -> Output {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let mut command = Command::new(env!("CARGO_BIN_EXE_bayesian-ssh"));
    command
        .env("XDG_CONFIG_HOME", temp_dir.path())
        .env("XDG_DATA_HOME", temp_dir.path().join("data"))
        .arg("--env")
        .arg(env_name)
        .args(args);

    command.output().expect("bssh command should run")
}

#[test]
fn command_failure_prints_actionable_error_when_database_file_is_missing() {
    // Given: a fresh environment where no command has initialized the SQLite database yet.
    // When: the user asks bssh to back up that missing database.
    let output = run_bssh("missing-db", &["backup"]);

    // Then: the real CLI exits with a clear error and recovery hint.
    assert!(!output.status.success());

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Error:"));
    assert!(stderr.contains("Database file does not exist"));
    assert!(stderr.contains("Suggestion:"));
}
