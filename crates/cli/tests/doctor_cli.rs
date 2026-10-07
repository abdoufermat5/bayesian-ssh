use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run_bssh(config_home: &Path, env_name: &str, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bayesian-ssh"));
    command
        .env("XDG_CONFIG_HOME", config_home)
        .env("XDG_DATA_HOME", config_home.join("data"))
        .arg("--env")
        .arg(env_name)
        .args(args);

    command.output().expect("bssh command should run")
}

fn config_file(config_home: &Path, env_name: &str) -> PathBuf {
    config_home
        .join("bayesian-ssh")
        .join("environments")
        .join(env_name)
        .join("config.json")
}

#[test]
fn doctor_reports_actionable_health_when_environment_is_new() {
    // Given: a brand-new isolated bssh configuration directory.
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");

    // When: the user runs the real CLI doctor command.
    let output = run_bssh(temp_dir.path(), "fresh", &["doctor"]);

    // Then: the command succeeds and reports the key local health checks.
    assert!(
        output.status.success(),
        "doctor should succeed for a new environment\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bssh doctor"));
    assert!(stdout.contains("Configuration"));
    assert!(stdout.contains("Database"));
    assert!(stdout.contains("SSH client"));
}

#[test]
fn doctor_fails_with_actionable_database_error_when_database_parent_is_file() {
    // Given: a valid config whose database parent path is occupied by a file.
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let env_name = "broken-db";
    let config_path = config_file(temp_dir.path(), env_name);
    let env_dir = config_path
        .parent()
        .expect("config path should have parent");
    fs::create_dir_all(env_dir).expect("env dir should be created");

    let blocked_parent = env_dir.join("blocked");
    fs::write(&blocked_parent, "not a directory").expect("blocked file should be created");
    let database_path = blocked_parent.join("history.db");
    let config_json = format!(
        r#"{{
  "database_path": "{}",
  "default_user": "tester",
  "default_bastion": null,
  "default_bastion_user": null,
  "default_port": 22,
  "use_kerberos_by_default": false,
  "ssh_config_path": null,
  "log_level": "off",
  "auto_save_history": true,
  "max_history_size": 1000,
  "search_mode": "bayesian",
  "transport": {{
    "force_subprocess": false,
    "strict_host_key_checking": "accept-new"
  }},
  "auth": {{
    "identity_files": [],
    "use_agent": true,
    "agent_socket": null
  }}
}}"#,
        database_path.display()
    );
    fs::write(&config_path, config_json).expect("config should be written");

    // When: the user runs the real CLI doctor command.
    let output = run_bssh(temp_dir.path(), env_name, &["doctor"]);

    // Then: doctor fails with an actionable database diagnostic.
    assert!(
        !output.status.success(),
        "doctor should fail when the database cannot be initialized"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Database"));
    assert!(stdout.contains("FAILED"));
    assert!(stdout.contains("Suggestion"));
}
