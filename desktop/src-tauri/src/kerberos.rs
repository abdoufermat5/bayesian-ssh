use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::Serialize;
use std::io::Write;
use std::process::{Command, Stdio};

#[derive(Serialize, Clone, Debug)]
pub struct KerberosStatus {
    pub tools_available: bool,
    pub has_ticket: bool,
    pub valid: bool,
    pub principal: Option<String>,
    pub cache_path: Option<String>,
    pub expires_at: Option<i64>,
    pub renew_until: Option<i64>,
    pub renewable: bool,
    pub seconds_remaining: Option<i64>,
}

fn command_exists(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn parse_klist_datetime(value: &str) -> Option<DateTime<Local>> {
    let trimmed = value.trim();
    for fmt in ["%m/%d/%Y %H:%M:%S", "%m/%d/%y %H:%M:%S"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            if let Some(local) = Local.from_local_datetime(&naive).single() {
                return Some(local);
            }
        }
    }
    None
}

fn parse_klist_output(output: &str, valid: bool) -> KerberosStatus {
    let mut principal = None;
    let mut cache_path = None;
    let mut expires_at = None;
    let mut renew_until = None;
    let mut renewable = false;
    let mut has_ticket = false;

    for line in output.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix("Ticket cache:") {
            cache_path = Some(rest.trim().to_string());
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("Default principal:") {
            principal = Some(rest.trim().to_string());
            has_ticket = true;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("renew until") {
            if let Some(dt) = parse_klist_datetime(rest) {
                renew_until = Some(dt.timestamp());
                renewable = true;
            }
            continue;
        }

        if trimmed.starts_with("Valid starting") || trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() >= 5 {
            let valid_from = format!("{} {}", parts[0], parts[1]);
            let expires = format!("{} {}", parts[2], parts[3]);
            let service = parts[4..].join(" ");

            if let Some(expiry) = parse_klist_datetime(&expires) {
                has_ticket = true;
                if service.starts_with("krbtgt/") {
                    expires_at = Some(expiry.timestamp());
                }
            }

            let _ = parse_klist_datetime(&valid_from);
        }
    }

    let seconds_remaining = expires_at.map(|exp| exp - Local::now().timestamp());

    KerberosStatus {
        tools_available: true,
        has_ticket,
        valid,
        principal,
        cache_path,
        expires_at,
        renew_until,
        renewable,
        seconds_remaining,
    }
}

fn run_kinit(args: &[&str], password: Option<&str>) -> Result<(), String> {
    let mut command = Command::new("kinit");
    command.args(args);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to run kinit: {e}"))?;

    if let Some(password) = password {
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(format!("{password}\n").as_bytes())
                .map_err(|e| format!("Failed to send password to kinit: {e}"))?;
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to wait for kinit: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Err(if stderr.is_empty() { stdout } else { stderr })
    }
}

#[tauri::command]
pub fn get_kerberos_status() -> Result<KerberosStatus, String> {
    if !command_exists("klist") || !command_exists("kinit") {
        return Ok(KerberosStatus {
            tools_available: false,
            has_ticket: false,
            valid: false,
            principal: None,
            cache_path: std::env::var("KRB5CCNAME").ok(),
            expires_at: None,
            renew_until: None,
            renewable: false,
            seconds_remaining: None,
        });
    }

    let valid = Command::new("klist")
        .arg("-s")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    let list_output = Command::new("klist")
        .output()
        .map_err(|e| format!("Failed to run klist: {e}"))?;

    let stdout = String::from_utf8_lossy(&list_output.stdout);
    let stderr = String::from_utf8_lossy(&list_output.stderr);
    let combined = format!("{stdout}{stderr}");

    if combined.contains("No credentials cache")
        || combined.contains("Credentials cache")
        || (!list_output.status.success() && !stdout.contains("Default principal:"))
    {
        return Ok(KerberosStatus {
            tools_available: true,
            has_ticket: false,
            valid: false,
            principal: None,
            cache_path: std::env::var("KRB5CCNAME").ok(),
            expires_at: None,
            renew_until: None,
            renewable: false,
            seconds_remaining: None,
        });
    }

    Ok(parse_klist_output(&stdout, valid))
}

#[tauri::command]
pub fn renew_kerberos_ticket(password: Option<String>) -> Result<KerberosStatus, String> {
    if !command_exists("kinit") {
        return Err("kinit is not installed".to_string());
    }

    let renew = run_kinit(&["-R"], None);
    if renew.is_ok() {
        return get_kerberos_status();
    }

    if let Some(password) = password.filter(|value| !value.is_empty()) {
        run_kinit(&["-f"], Some(&password))?;
        return get_kerberos_status();
    }

    Err(renew.unwrap_err())
}

#[tauri::command]
pub fn acquire_kerberos_ticket(
    principal: Option<String>,
    password: String,
    forwardable: Option<bool>,
) -> Result<KerberosStatus, String> {
    if !command_exists("kinit") {
        return Err("kinit is not installed".to_string());
    }

    if password.trim().is_empty() {
        return Err("Password is required".to_string());
    }

    let forwardable = forwardable.unwrap_or(true);
    let principal_value = principal
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string());

    let mut args: Vec<String> = Vec::new();
    if forwardable {
        args.push("-f".to_string());
    }
    if let Some(principal) = principal_value {
        args.push(principal);
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_kinit(&arg_refs, Some(password.trim()))?;
    get_kerberos_status()
}
