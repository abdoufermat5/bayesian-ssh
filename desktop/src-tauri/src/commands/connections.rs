use super::get_db_and_config;
use bayesian_ssh::models::Connection;
use uuid::Uuid;

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
                .map_err(|e| e.to_string());
        }
    }

    db.list_connections(tag_filter.as_deref(), false)
        .map_err(|e| e.to_string())
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

    let mut connection = Connection::new(
        name,
        host,
        user.unwrap_or(default_u),
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
