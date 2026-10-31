use crate::cli::utils::resolve_connection;
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::SshService;
use anyhow::Result;
use tracing::info;

/// Which connection `bssh edit` targets and what to change on it.
pub struct EditArgs {
    pub target: String,
    pub changes: ConnectionEdit,
}

/// Field changes for one connection; `None`/`false`/empty means "leave as is".
pub struct ConnectionEdit {
    pub name: Option<String>,
    pub host: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub kerberos: Option<bool>,
    pub bastion: Option<String>,
    /// Drop the bastion and its user (applied after `bastion`).
    pub no_bastion: bool,
    pub bastion_user: Option<String>,
    pub key: Option<String>,
    /// `Some(true)` turns agent forwarding on, `Some(false)` turns it off.
    pub forward_agent: Option<bool>,
    pub add_tags: Vec<String>,
    pub remove_tags: Vec<String>,
}

impl ConnectionEdit {
    fn apply(self, connection: &mut Connection) {
        if let Some(name) = self.name {
            connection.name = name;
        }
        if let Some(host) = self.host {
            connection.host = host;
        }
        if let Some(user) = self.user {
            connection.user = user;
        }
        if let Some(port) = self.port {
            connection.port = port;
        }
        if let Some(kerberos) = self.kerberos {
            connection.use_kerberos = kerberos;
        }
        if let Some(bastion) = self.bastion {
            connection.bastion = Some(bastion);
        }
        if self.no_bastion {
            connection.bastion = None;
            connection.bastion_user = None;
        }
        if let Some(bastion_user) = self.bastion_user {
            connection.bastion_user = Some(bastion_user);
        }
        if let Some(key) = self.key {
            connection.key_path = Some(key);
        }
        if let Some(forward_agent) = self.forward_agent {
            connection.forward_agent = forward_agent;
        }

        for tag in self.add_tags {
            connection.add_tag(tag);
        }
        for tag in self.remove_tags {
            connection.remove_tag(&tag);
        }
    }
}

pub async fn execute(args: EditArgs, config: AppConfig) -> Result<()> {
    let EditArgs { target, changes } = args;
    info!("Editing connection: {}", target);

    let ssh_service = SshService::new(config.clone())?;
    let connection = resolve_connection(&ssh_service, &target, "edit", false, &config).await?;
    update_connection(ssh_service, connection, changes).await
}

async fn update_connection(
    ssh_service: SshService,
    mut connection: Connection,
    changes: ConnectionEdit,
) -> Result<()> {
    changes.apply(&mut connection);

    // Update in database
    ssh_service.update_connection(connection.clone()).await?;

    println!("✅ Connection '{}' updated successfully!", connection.name);
    println!("\nUpdated connection details:");
    println!("  Name: {}", connection.name);
    println!("  Host: {}:{}", connection.host, connection.port);
    println!("  User: {}", connection.user);
    if let Some(bastion) = &connection.bastion {
        println!(
            "  Bastion: {}@{}",
            connection
                .bastion_user
                .as_deref()
                .unwrap_or(&connection.user),
            bastion
        );
    }
    println!(
        "  Kerberos: {}",
        if connection.use_kerberos {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    println!(
        "  Agent forwarding: {}",
        if connection.forward_agent {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    if !connection.tags.is_empty() {
        println!("  Tags: {}", connection.tags.join(", "));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection() -> Connection {
        let mut c = Connection::new(
            "web".to_string(),
            "web.example.com".to_string(),
            "deploy".to_string(),
            22,
            Some("jump".to_string()),
            Some("ops".to_string()),
            false,
            None,
        );
        c.add_tag("prod".to_string());
        c
    }

    fn no_changes() -> ConnectionEdit {
        ConnectionEdit {
            name: None,
            host: None,
            user: None,
            port: None,
            kerberos: None,
            bastion: None,
            no_bastion: false,
            bastion_user: None,
            key: None,
            forward_agent: None,
            add_tags: Vec::new(),
            remove_tags: Vec::new(),
        }
    }

    #[test]
    fn empty_edit_leaves_connection_untouched() {
        let mut c = connection();
        no_changes().apply(&mut c);
        assert_eq!(c.host, "web.example.com");
        assert_eq!(c.bastion.as_deref(), Some("jump"));
        assert_eq!(c.tags, vec!["prod"]);
    }

    #[test]
    fn no_bastion_clears_bastion_then_bastion_user_applies() {
        let mut c = connection();
        ConnectionEdit {
            no_bastion: true,
            ..no_changes()
        }
        .apply(&mut c);
        assert_eq!(c.bastion, None);
        assert_eq!(c.bastion_user, None);

        ConnectionEdit {
            no_bastion: true,
            bastion_user: Some("late".to_string()),
            ..no_changes()
        }
        .apply(&mut c);
        assert_eq!(c.bastion_user.as_deref(), Some("late"));
    }

    #[test]
    fn forward_agent_and_tags_apply() {
        let mut c = connection();
        ConnectionEdit {
            forward_agent: Some(true),
            add_tags: vec!["db".to_string(), "prod".to_string()],
            remove_tags: vec![" prod ".to_string()],
            ..no_changes()
        }
        .apply(&mut c);
        assert!(c.forward_agent);
        assert_eq!(c.tags, vec!["db"]);

        ConnectionEdit {
            forward_agent: Some(false),
            ..no_changes()
        }
        .apply(&mut c);
        assert!(!c.forward_agent);
    }
}
