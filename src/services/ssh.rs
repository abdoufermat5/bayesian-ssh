use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Connection, Session};
use anyhow::Result;
use std::process::{Command, Stdio};
use tokio::process::Command as TokioCommand;
use tracing::{error, info, warn};

pub struct SshService {
    config: AppConfig,
    database: Database,
}

impl SshService {
    pub fn new(config: AppConfig) -> Result<Self> {
        let database = Database::new(&config)?;
        Ok(SshService { config, database })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn connect(
        &self,
        target: &str,
        user: Option<String>,
        port: Option<u16>,
        kerberos: Option<bool>,
        bastion: Option<String>,
        no_bastion: bool,
        bastion_user: Option<String>,
        key: Option<String>,
    ) -> Result<()> {
        // Try to find connection in database first
        if let Some(mut connection) = self.database.get_connection(target)? {
            info!("Found existing connection: {}", connection.name);

            // Override with command line arguments if provided
            if let Some(user) = user {
                connection.user = user;
            }
            if let Some(port) = port {
                connection.port = port;
            }
            if let Some(kerberos) = kerberos {
                connection.use_kerberos = kerberos;
            }
            if let Some(bastion) = bastion {
                connection.bastion = Some(bastion);
            }
            if no_bastion {
                connection.bastion = None;
                connection.bastion_user = None;
            }
            if let Some(bastion_user) = bastion_user {
                connection.bastion_user = Some(bastion_user);
            }
            if let Some(key) = key {
                connection.key_path = Some(key);
            }

            // Update last used timestamp
            connection.update_last_used();
            self.database.update_connection(&connection)?;

            return self.execute_ssh(&connection).await;
        }

        // If not found, try to connect directly
        info!(
            "Connection '{}' not found, attempting direct connection",
            target
        );

        let connection = Connection::new(
            target.to_string(),
            target.to_string(),
            user.unwrap_or_else(|| self.config.default_user.clone()),
            port.unwrap_or(self.config.default_port),
            bastion,      // Only use bastion if explicitly specified
            bastion_user, // Only use bastion_user if explicitly specified
            kerberos.unwrap_or(self.config.use_kerberos_by_default),
            key,
        );

        self.execute_ssh(&connection).await
    }

    async fn execute_ssh(&self, connection: &Connection) -> Result<()> {
        info!("Executing SSH connection to {}", connection.host);

        // Check and create Kerberos ticket if needed
        if connection.use_kerberos {
            self.ensure_kerberos_ticket().await?;
        }

        // Create session record
        let mut session = Session::new(connection.clone());
        self.database.add_session(&session)?;

        // Build SSH command
        let mut cmd = Command::new("ssh");

        // Add Kerberos flags if enabled
        if connection.use_kerberos {
            cmd.arg("-t").arg("-A").arg("-K");
        }

        // Add SSH key if specified
        if let Some(key_path) = &connection.key_path {
            cmd.arg("-i").arg(key_path);
        }

        // Add port and target
        if let Some(bastion) = &connection.bastion {
            // If bastion is specified, connect to bastion and specify target
            let bastion_user = connection
                .bastion_user
                .as_deref()
                .unwrap_or(&connection.user);
            cmd.arg("-p").arg("22"); // Default bastion port
            cmd.arg(format!("{}@{}", bastion_user, bastion));
            // Add the target host as additional argument
            cmd.arg(format!("{}@{}", connection.user, connection.host));

            info!(
                "Connecting via bastion {} to target {}",
                bastion, connection.host
            );
        } else {
            // Direct connection
            cmd.arg("-p").arg(connection.port.to_string());
            cmd.arg(format!("{}@{}", connection.user, connection.host));
        }

        // Set up terminal
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        info!("Executing command: {:?}", cmd);

        // Execute SSH command
        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                session.mark_active(pid);
                self.database.update_session(&session)?;

                info!("SSH session started with PID: {}", pid);

                // Wait for completion
                match child.wait() {
                    Ok(status) => {
                        if status.success() {
                            info!("SSH session completed successfully");
                            session.mark_terminated(0);
                        } else {
                            warn!("SSH session exited with code: {:?}", status.code());
                            session.mark_terminated(status.code().unwrap_or(-1));
                        }
                    }
                    Err(e) => {
                        error!("Error waiting for SSH process: {}", e);
                        session.mark_error(format!("Process error: {}", e));
                    }
                }

                self.database.update_session(&session)?;
            }
            Err(e) => {
                error!("Failed to spawn SSH process: {}", e);
                session.mark_error(format!("Spawn error: {}", e));
                self.database.update_session(&session)?;
                return Err(e.into());
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_connection(
        &self,
        name: String,
        host: String,
        user: Option<String>,
        port: Option<u16>,
        kerberos: Option<bool>,
        bastion: Option<String>,
        no_bastion: bool,
        bastion_user: Option<String>,
        key: Option<String>,
        tags: Vec<String>,
    ) -> Result<()> {
        // Determine bastion configuration
        let final_bastion = if no_bastion {
            None // Force no bastion
        } else {
            bastion.or_else(|| self.config.default_bastion.clone())
        };

        let final_bastion_user = if no_bastion {
            None // Force no bastion user
        } else {
            bastion_user.or_else(|| self.config.default_bastion_user.clone())
        };

        // Determine Kerberos setting: disable by default for direct connections
        let final_kerberos = if let Some(k) = kerberos {
            k // Use explicitly provided value
        } else if final_bastion.is_some() {
            self.config.use_kerberos_by_default // Use default for bastion connections
        } else {
            false // Disable for direct connections
        };

        let connection = Connection::new(
            name.clone(),
            host,
            user.unwrap_or_else(|| self.config.default_user.clone()),
            port.unwrap_or(self.config.default_port),
            final_bastion,
            final_bastion_user,
            final_kerberos,
            key,
        );

        // Add tags
        let mut conn = connection;
        for tag in tags {
            conn.add_tag(tag);
        }

        self.database.add_connection(&conn)?;
        info!("Connection '{}' added successfully", name);

        Ok(())
    }

    pub async fn list_connections(
        &self,
        tag_filter: Option<&str>,
        recent_only: bool,
    ) -> Result<Vec<Connection>> {
        self.database.list_connections(tag_filter, recent_only)
    }

    pub async fn remove_connection(&self, target: &str) -> Result<bool> {
        self.database.remove_connection(target)
    }

    pub async fn get_connection(&self, target: &str) -> Result<Option<Connection>> {
        self.database.get_connection(target)
    }

    pub async fn update_connection(&self, mut connection: Connection) -> Result<()> {
        connection.update_last_used();
        self.database.update_connection(&connection)
    }

    pub async fn get_stats(&self) -> Result<crate::models::ConnectionStats> {
        self.database.get_stats()
    }

    /// Ensure a valid Kerberos ticket exists, creating one if necessary
    async fn ensure_kerberos_ticket(&self) -> Result<()> {
        info!("Checking Kerberos ticket status...");

        // Check if we have a valid ticket
        let ticket_status = TokioCommand::new("klist").arg("-s").output().await?;

        if ticket_status.status.success() {
            info!("Valid Kerberos ticket found");
            return Ok(());
        }

        info!("No valid Kerberos ticket found, creating new forwardable ticket...");

        // Create a new forwardable ticket
        let kinit_result = TokioCommand::new("kinit")
            .arg("-f") // Forwardable ticket
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;

        let kinit_status = kinit_result.wait_with_output().await?;

        if kinit_status.status.success() {
            info!("Kerberos ticket created successfully");
            Ok(())
        } else {
            let error_output = String::from_utf8_lossy(&kinit_status.stderr);
            error!("Failed to create Kerberos ticket: {}", error_output);
            Err(anyhow::anyhow!(
                "Failed to create Kerberos ticket: {}",
                error_output
            ))
        }
    }

    // Fuzzy search methods for enhanced connection discovery
    pub async fn fuzzy_search(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        self.database.fuzzy_search_connections(query, limit)
    }

    pub async fn get_recent_connections(&self, limit: usize) -> Result<Vec<Connection>> {
        self.database.list_connections(None, true)
            .map(|mut connections| {
                connections.truncate(limit);
                connections
            })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn connect_to_connection(
        &self,
        connection: &Connection,
        user: Option<String>,
        port: Option<u16>,
        kerberos: Option<bool>,
        bastion: Option<String>,
        no_bastion: bool,
        bastion_user: Option<String>,
        key: Option<String>,
    ) -> Result<()> {
        info!("Connecting to connection: {}", connection.name);

        // Create a mutable copy to apply overrides
        let mut conn = connection.clone();

        // Apply command-line overrides
        if let Some(user) = user {
            conn.user = user;
        }
        if let Some(port) = port {
            conn.port = port;
        }
        if let Some(kerberos) = kerberos {
            conn.use_kerberos = kerberos;
        }
        if let Some(bastion) = bastion {
            conn.bastion = Some(bastion);
        }
        if no_bastion {
            conn.bastion = None;
            conn.bastion_user = None;
        }
        if let Some(bastion_user) = bastion_user {
            conn.bastion_user = Some(bastion_user);
        }
        if let Some(key) = key {
            conn.key_path = Some(key);
        }

        // Update last used timestamp
        conn.update_last_used();
        self.database.update_connection(&conn)?;

        // Execute the connection
        self.execute_ssh(&conn).await
    }
}
