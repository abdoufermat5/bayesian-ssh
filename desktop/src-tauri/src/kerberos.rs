use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Serialize, Clone, Debug, Default)]
pub struct KerberosStatus {
    pub tools_available: bool,
    /// True when a readable krb5.conf defines a default realm (client is configured).
    pub client_configured: bool,
    pub has_ticket: bool,
    pub valid: bool,
    pub principal: Option<String>,
    /// Principal inferred from krb5.conf when no ticket is present.
    pub suggested_principal: Option<String>,
    pub default_realm: Option<String>,
    pub config_path: Option<String>,
    pub cache_path: Option<String>,
    pub expires_at: Option<i64>,
    pub renew_until: Option<i64>,
    pub renewable: bool,
    pub seconds_remaining: Option<i64>,
}

#[derive(Clone, Debug, Default)]
struct Krb5Config {
    config_path: Option<String>,
    default_realm: Option<String>,
    default_principal: Option<String>,
    default_ccache_name: Option<String>,
}

impl Krb5Config {
    fn is_configured(&self) -> bool {
        self.default_realm.as_ref().is_some_and(|r| !r.is_empty())
    }
}

fn resolve_command(name: &str) -> Option<PathBuf> {
    if let Ok(output) = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                return Some(PathBuf::from(path_str));
            }
        }
    }

    for path in [
        "/usr/bin",
        "/usr/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/lib/mit/bin",
        "/usr/lib/mit/sbin",
        "/usr/libexec/krb5/bin",
        "/usr/heimdal/bin",
        "/usr/heimdal/sbin",
        "/opt/local/bin",
        "/opt/local/sbin",
    ] {
        let full_path = PathBuf::from(path).join(name);
        if full_path.exists() {
            return Some(full_path);
        }
    }

    None
}

fn command_exists(name: &str) -> bool {
    resolve_command(name).is_some()
}

fn resolve_krb5_config_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("KRB5_CONFIG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    for candidate in ["/etc/krb5.conf", "/etc/krb5/krb5.conf"] {
        let path = PathBuf::from(candidate);
        if path.is_file() {
            return Some(path);
        }
    }

    None
}

fn parse_krb5_libdefaults(content: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut in_libdefaults = false;
    let mut default_realm = None;
    let mut default_principal = None;
    let mut default_ccache = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_libdefaults = trimmed.eq_ignore_ascii_case("[libdefaults]");
            continue;
        }
        if !in_libdefaults || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';')
        {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value
            .split('#')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        if value.is_empty() {
            continue;
        }

        match key.as_str() {
            "default_realm" => default_realm = Some(value),
            "default_principal" => default_principal = Some(value),
            "default_ccache_name" => default_ccache = Some(value),
            _ => {}
        }
    }

    (default_realm, default_principal, default_ccache)
}

fn load_krb5_config() -> Krb5Config {
    let Some(path) = resolve_krb5_config_path() else {
        return Krb5Config::default();
    };

    let Ok(content) = fs::read_to_string(&path) else {
        return Krb5Config {
            config_path: Some(path.display().to_string()),
            ..Default::default()
        };
    };

    let (default_realm, default_principal, default_ccache_name) = parse_krb5_libdefaults(&content);
    Krb5Config {
        config_path: Some(path.display().to_string()),
        default_realm,
        default_principal,
        default_ccache_name,
    }
}

fn login_name() -> Option<String> {
    for key in ["USER", "LOGNAME", "USERNAME"] {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn current_uid_string() -> Option<String> {
    if let Ok(uid) = std::env::var("UID") {
        let trimmed = uid.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    #[cfg(unix)]
    {
        let status_output = Command::new("id").arg("-u").output().ok()?;
        if status_output.status.success() {
            let uid = String::from_utf8_lossy(&status_output.stdout).trim().to_string();
            if !uid.is_empty() {
                return Some(uid);
            }
        }
    }

    None
}

fn expand_ccache_template(template: &str) -> String {
    let uid = current_uid_string().unwrap_or_else(|| "0".to_string());
    template
        .replace("%{uid}", &uid)
        .replace("%{euid}", &uid)
}

fn resolve_cache_path(config: &Krb5Config, klist_output: Option<&str>) -> Option<String> {
    if let Ok(path) = std::env::var("KRB5CCNAME") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Some(output) = klist_output {
        if let Some(path) = parse_cache_path_from_klist(output) {
            return Some(path);
        }
    }

    if let Some(template) = config.default_ccache_name.as_ref() {
        return Some(expand_ccache_template(template));
    }

    current_uid_string().map(|uid| format!("FILE:/tmp/krb5cc_{uid}"))
}

fn parse_cache_path_from_klist(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        for prefix in ["Ticket cache:", "Credentials cache:", "Using cache:"] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let path = rest.trim();
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }

    if let Some(start) = output.find("(filename:") {
        let rest = &output[start + "(filename:".len()..];
        let end = rest.find(')')?;
        let path = rest[..end].trim();
        if !path.is_empty() {
            return Some(path.to_string());
        }
    }

    if let Some(start) = output.find("(ticket cache ") {
        let rest = &output[start + "(ticket cache ".len()..];
        let end = rest.find(')')?;
        let path = rest[..end].trim();
        if !path.is_empty() {
            return Some(path.to_string());
        }
    }

    None
}

fn infer_suggested_principal(config: &Krb5Config) -> Option<String> {
    if let Some(principal) = config.default_principal.as_ref() {
        let trimmed = principal.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let realm = config.default_realm.as_ref()?.trim();
    if realm.is_empty() {
        return None;
    }

    let user = login_name()?;
    Some(format!("{user}@{realm}"))
}

fn enrich_status(mut status: KerberosStatus, config: &Krb5Config, klist_output: Option<&str>) -> KerberosStatus {
    status.client_configured = config.is_configured();
    status.config_path = config.config_path.clone();
    status.default_realm = config.default_realm.clone();
    status.suggested_principal = infer_suggested_principal(config);
    if status.cache_path.is_none() {
        status.cache_path = resolve_cache_path(config, klist_output);
    }
    if !status.has_ticket && status.principal.is_none() {
        status.principal = status.suggested_principal.clone();
    }
    status
}

fn base_status(tools_available: bool, config: &Krb5Config, klist_output: Option<&str>) -> KerberosStatus {
    enrich_status(
        KerberosStatus {
            tools_available,
            has_ticket: false,
            valid: false,
            principal: None,
            expires_at: None,
            renew_until: None,
            renewable: false,
            seconds_remaining: None,
            ..Default::default()
        },
        config,
        klist_output,
    )
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

fn has_ticket_in_output(output: &str) -> bool {
    output.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("Default principal:")
            || trimmed.starts_with("Principal:")
            || (trimmed.starts_with("Ticket(s):") && !trimmed.ends_with("Not available"))
    })
}

fn is_no_ticket_output(combined: &str, stdout: &str) -> bool {
    if has_ticket_in_output(stdout) {
        return false;
    }

    let lower = combined.to_ascii_lowercase();
    lower.contains("no credentials cache")
        || lower.contains("credentials cache file")
        || lower.contains("cannot find")
        || lower.contains("ticket cache file")
        || lower.contains("did not find")
}

fn parse_klist_output(output: &str, valid: bool, config: &Krb5Config) -> KerberosStatus {
    let mut principal = None;
    let mut cache_path = None;
    let mut expires_at = None;
    let mut renew_until = None;
    let mut renewable = false;
    let mut has_ticket = false;

    for line in output.lines() {
        let trimmed = line.trim();

        if let Some(rest) = trimmed
            .strip_prefix("Ticket cache:")
            .or_else(|| trimmed.strip_prefix("Credentials cache:"))
            .or_else(|| trimmed.strip_prefix("Using cache:"))
        {
            cache_path = Some(rest.trim().to_string());
            continue;
        }

        if let Some(rest) = trimmed
            .strip_prefix("Default principal:")
            .or_else(|| trimmed.strip_prefix("Principal:"))
        {
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

    enrich_status(
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
            ..Default::default()
        },
        config,
        Some(output),
    )
}

fn run_kinit(args: &[&str], password: Option<&str>) -> Result<(), String> {
    let kinit_path = resolve_command("kinit").ok_or_else(|| "kinit command not found".to_string())?;
    let mut command = Command::new(kinit_path);
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
    let config = load_krb5_config();

    let klist_path = match resolve_command("klist") {
        Some(path) => path,
        None => return Ok(base_status(false, &config, None)),
    };

    if !command_exists("kinit") {
        return Ok(base_status(false, &config, None));
    }

    let valid = Command::new(&klist_path)
        .arg("-s")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    let list_output = Command::new(&klist_path)
        .output()
        .map_err(|e| format!("Failed to run klist: {e}"))?;

    let stdout = String::from_utf8_lossy(&list_output.stdout);
    let stderr = String::from_utf8_lossy(&list_output.stderr);
    let combined = format!("{stdout}{stderr}");

    if is_no_ticket_output(&combined, &stdout) {
        return Ok(base_status(true, &config, Some(&combined)));
    }

    Ok(parse_klist_output(&stdout, valid, &config))
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
    proxiable: Option<bool>,
    lifetime: Option<String>,
    renew_lifetime: Option<String>,
) -> Result<KerberosStatus, String> {
    if !command_exists("kinit") {
        return Err("kinit is not installed".to_string());
    }

    if password.trim().is_empty() {
        return Err("Password is required".to_string());
    }

    let forwardable = forwardable.unwrap_or(true);
    let proxiable = proxiable.unwrap_or(false);
    let principal_value = principal
        .filter(|value| !value.trim().is_empty())
        .or_else(|| infer_suggested_principal(&load_krb5_config()))
        .map(|value| value.trim().to_string());

    let mut args: Vec<String> = Vec::new();
    if forwardable {
        args.push("-f".to_string());
    } else {
        args.push("-F".to_string());
    }
    if proxiable {
        args.push("-p".to_string());
    }
    if let Some(lt) = lifetime.filter(|l| !l.trim().is_empty()) {
        args.push("-l".to_string());
        args.push(lt);
    }
    if let Some(rt) = renew_lifetime.filter(|r| !r.trim().is_empty()) {
        args.push("-r".to_string());
        args.push(rt);
    }
    if let Some(principal) = principal_value {
        args.push(principal);
    }

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_kinit(&arg_refs, Some(password.trim()))?;
    get_kerberos_status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_krb5_libdefaults() {
        let content = r#"
[libdefaults]
    default_realm = CLOUDSANTE.PRIV
    default_ccache_name = FILE:/tmp/krb5cc_%{uid}
"#;
        let (realm, principal, ccache) = parse_krb5_libdefaults(content);
        assert_eq!(realm.as_deref(), Some("CLOUDSANTE.PRIV"));
        assert!(principal.is_none());
        assert_eq!(ccache.as_deref(), Some("FILE:/tmp/krb5cc_%{uid}"));
    }

    #[test]
    fn detects_no_ticket_message_without_matching_valid_cache_header() {
        let stdout = "";
        let combined = "klist: No credentials cache found (filename: /tmp/krb5cc_1000)\n";
        assert!(is_no_ticket_output(combined, stdout));
    }

    #[test]
    fn does_not_treat_valid_heimdal_cache_as_missing_ticket() {
        let stdout = "Credentials cache: FILE:/tmp/krb5cc_1000\n\tPrincipal: alice@REALM\n";
        let combined = stdout;
        assert!(!is_no_ticket_output(combined, stdout));
        assert!(has_ticket_in_output(stdout));
    }

    #[test]
    fn parses_cache_path_from_klist_error() {
        let output = "klist: No credentials cache found (filename: /tmp/krb5cc_1000)";
        assert_eq!(
            parse_cache_path_from_klist(&output).as_deref(),
            Some("/tmp/krb5cc_1000")
        );
    }
}
