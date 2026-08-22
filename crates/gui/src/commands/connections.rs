use super::get_db_and_config;
use bayesian_ssh::models::Connection;
use uuid::Uuid;

fn friendly_db_error(e: impl std::fmt::Display) -> String {
    let msg = e.to_string();
    if msg.contains("UNIQUE constraint failed") {
        "A connection with this name already exists.".to_string()
    } else {
        msg
    }
}

const INVALID_HOST_CHARS: &[char] = &[
    ' ', '\t', '\n', '\r', '\'', '"', '`', '$', ';', '&', '|', '<', '>', '(', ')', '{', '}', '*',
    '?', '!', '#', '~', '\\', '/', ',', '=', '%', '@',
];
const INVALID_USER_CHARS: &[char] = &[
    ' ', '\t', '\n', '\r', '\'', '"', '`', '$', ';', '&', '|', '<', '>', '(', ')', '{', '}', '*',
    '?', '!', '#', '~', '\\', '/', '@',
];

fn validate_host_field(field: &str, label: &str, allow_ipv6_brackets: bool) -> Result<(), String> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be empty."));
    }
    if trimmed.len() > 253 {
        return Err(format!("{label} is too long (max 253 characters)."));
    }
    let mut chars = trimmed.chars().peekable();
    // Allow a leading '[' for bracketed IPv6 literals, e.g. [::1].
    if allow_ipv6_brackets && chars.peek() == Some(&'[') {
        if !trimmed.ends_with(']') {
            return Err(format!("{label} has an invalid bracketed IPv6 literal."));
        }
        if trimmed[1..trimmed.len() - 1].contains('[') || trimmed[1..trimmed.len() - 1].contains(']')
        {
            return Err(format!("{label} has an invalid bracketed IPv6 literal."));
        }
        return Ok(());
    }
    if trimmed.chars().any(|c| {
        c.is_control() || INVALID_HOST_CHARS.contains(&c)
    }) {
        return Err(format!(
            "{label} contains invalid characters (spaces, quotes, shell metacharacters, or '@' are not allowed)."
        ));
    }
    Ok(())
}

fn validate_user_field(field: &str, label: &str) -> Result<(), String> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be empty."));
    }
    if trimmed.len() > 64 {
        return Err(format!("{label} is too long (max 64 characters)."));
    }
    if trimmed.chars().any(|c| c.is_control() || INVALID_USER_CHARS.contains(&c)) {
        return Err(format!(
            "{label} contains invalid characters (spaces, quotes, shell metacharacters, or '@' are not allowed)."
        ));
    }
    Ok(())
}

fn validate_key_path_field(field: &Option<String>) -> Result<(), String> {
    let Some(path) = field else {
        return Ok(());
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.len() > 4096 {
        return Err("Key path is too long.".to_string());
    }
    if trimmed.chars().any(|c| c.is_control() || c == '\'' || c == '"' || c == '`' || c == '$') {
        return Err("Key path contains invalid characters.".to_string());
    }
    Ok(())
}

fn validate_connection_fields(
    name: &str,
    host: &str,
    user: &str,
    bastion: &Option<String>,
    bastion_user: &Option<String>,
    key_path: &Option<String>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty.".to_string());
    }
    if name.len() > 128 {
        return Err("Name is too long (max 128 characters).".to_string());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("Name contains invalid control characters.".to_string());
    }

    validate_host_field(host, "Host", true)?;
    validate_user_field(user, "User")?;

    if let Some(b) = bastion {
        validate_host_field(b, "Bastion host", true)?;
    }
    if let Some(bu) = bastion_user {
        validate_user_field(bu, "Bastion user")?;
    }
    validate_key_path_field(key_path)
}

#[tauri::command]
pub fn get_connections(
    query: Option<String>,
    tag_filter: Option<String>,
) -> Result<Vec<Connection>, String> {
    let (db, config) = get_db_and_config()?;

    if let Some(q) = query {
        if !q.trim().is_empty() {
            return db
                .search_connections(&q, 100, &config.search_mode)
                .map_err(friendly_db_error);
        }
    }

    db.list_connections(tag_filter.as_deref(), false)
        .map_err(friendly_db_error)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn add_connection(
    name: String,
    host: String,
    user: Option<String>,
    port: Option<u16>,
    kerberos: Option<bool>,
    bastion: Option<String>,
    bastion_user: Option<String>,
    key_path: Option<String>,
    tags: Vec<String>,
) -> Result<(), String> {
    let (db, config) = get_db_and_config()?;

    let default_u = config.default_user.clone();
    let default_p = config.default_port;
    let default_k = config.use_kerberos_by_default;

    let user = user.unwrap_or(default_u);
    validate_connection_fields(&name, &host, &user, &bastion, &bastion_user, &key_path)?;

    let mut connection = Connection::new(
        name,
        host,
        user,
        port.unwrap_or(default_p),
        bastion,
        bastion_user,
        kerberos.unwrap_or(default_k),
        key_path,
    );

    for tag in tags {
        connection.add_tag(tag);
    }

    db.add_connection(&connection).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn edit_connection(
    id: String,
    name: String,
    host: String,
    user: String,
    port: u16,
    kerberos: bool,
    bastion: Option<String>,
    bastion_user: Option<String>,
    key_path: Option<String>,
    tags: Vec<String>,
) -> Result<(), String> {
    let (db, _config) = get_db_and_config()?;

    let uuid = Uuid::parse_str(&id).map_err(|e: uuid::Error| e.to_string())?;

    let existing = db
        .get_connection(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Connection '{}' not found", id))?;

    if existing.id != uuid {
        return Err("Connection id mismatch.".to_string());
    }

    validate_connection_fields(&name, &host, &user, &bastion, &bastion_user, &key_path)?;

    let mut connection = Connection {
        id: uuid,
        name,
        host,
        user,
        port,
        bastion,
        bastion_user,
        use_kerberos: kerberos,
        key_path,
        created_at: existing.created_at,
        last_used: existing.last_used,
        tags: Vec::new(),
        aliases: existing.aliases,
    };

    for tag in tags {
        connection.add_tag(tag);
    }

    db.update_connection(&connection).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_connection(id_or_name: String) -> Result<(), String> {
    let (db, _config) = get_db_and_config()?;

    db.remove_connection(&id_or_name)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct BatchExecHostResult {
    pub connection_id: String,
    pub name: String,
    pub host: String,
    pub user: String,
    pub is_production: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub success: bool,
    pub duration_ms: u64,
}

#[tauri::command]
pub async fn run_batch_command(
    connection_ids: Vec<String>,
    command: String,
    dry_run: bool,
    timeout_secs: Option<u64>,
) -> Result<Vec<BatchExecHostResult>, String> {
    let (db, config) = get_db_and_config()?;
    let all_conns = db.list_connections(None, false).map_err(|e| e.to_string())?;

    let targets: Vec<Connection> = all_conns
        .into_iter()
        .filter(|c| connection_ids.contains(&c.id.to_string()) || connection_ids.contains(&c.name))
        .collect();

    if targets.is_empty() {
        return Err("No valid target connections selected".to_string());
    }

    if command.trim().is_empty() {
        return Err("Command cannot be empty".to_string());
    }

    let timeout_duration = std::time::Duration::from_secs(timeout_secs.unwrap_or(10));

    if dry_run {
        let results = targets
            .into_iter()
            .map(|conn| {
                let is_prod = conn.name.to_lowercase().contains("prod")
                    || conn.tags.iter().any(|t| t.to_lowercase().contains("prod"));
                let stdout_msg = format!("[DRY RUN PREVIEW] Would execute '{}' on {}@{}", command, conn.user, conn.host);
                BatchExecHostResult {
                    connection_id: conn.id.to_string(),
                    name: conn.name,
                    host: conn.host,
                    user: conn.user,
                    is_production: is_prod,
                    stdout: stdout_msg,
                    stderr: String::new(),
                    exit_code: 0,
                    success: true,
                    duration_ms: 0,
                }
            })
            .collect();
        return Ok(results);
    }

    let mut tasks = Vec::new();
    for conn in targets {
        let cfg_clone = config.clone();
        let cmd_clone = command.clone();
        let timeout_dur = timeout_duration;

        tasks.push(tokio::spawn(async move {
            let is_prod = conn.name.to_lowercase().contains("prod")
                || conn.tags.iter().any(|t| t.to_lowercase().contains("prod"));

            let start = std::time::Instant::now();
            let conn_for_ref = conn.clone();
            let conn_for_closure = conn.clone();
            let cmd_for_exec = cmd_clone.clone();

            let exec_future = bayesian_ssh::services::transport::execute_with_fallback(&conn_for_ref, &cfg_clone, move |transport| {
                let c = conn_for_closure.clone();
                let cm = cmd_for_exec.clone();
                Box::pin(async move { transport.exec(&c, &cm).await })
            });

            match tokio::time::timeout(timeout_dur, exec_future).await {
                Ok(Ok(output)) => {
                    let duration = start.elapsed().as_millis() as u64;
                    BatchExecHostResult {
                        connection_id: conn.id.to_string(),
                        name: conn.name,
                        host: conn.host,
                        user: conn.user,
                        is_production: is_prod,
                        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        exit_code: output.exit_code,
                        success: output.exit_code == 0,
                        duration_ms: duration,
                    }
                }
                Ok(Err(e)) => {
                    let duration = start.elapsed().as_millis() as u64;
                    BatchExecHostResult {
                        connection_id: conn.id.to_string(),
                        name: conn.name,
                        host: conn.host,
                        user: conn.user,
                        is_production: is_prod,
                        stdout: String::new(),
                        stderr: format!("Execution failed: {}", e),
                        exit_code: -1,
                        success: false,
                        duration_ms: duration,
                    }
                }
                Err(_) => {
                    let duration = start.elapsed().as_millis() as u64;
                    BatchExecHostResult {
                        connection_id: conn.id.to_string(),
                        name: conn.name,
                        host: conn.host,
                        user: conn.user,
                        is_production: is_prod,
                        stdout: String::new(),
                        stderr: format!("Execution timed out after {}s", timeout_dur.as_secs()),
                        exit_code: -1,
                        success: false,
                        duration_ms: duration,
                    }
                }
            }
        }));
    }

    let mut results = Vec::new();
    for task in tasks {
        if let Ok(res) = task.await {
            results.push(res);
        }
    }

    Ok(results)
}
