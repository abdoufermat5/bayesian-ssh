use crate::config::AppConfig;
use crate::database::Database;
use crate::models::{Connection, Session};
use anyhow::Result;
use std::process::Stdio;
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
        record: Option<bool>,
    ) -> Result<()> {
        // Try to find connection in database first (also checks aliases)
        if let Some(mut connection) = self.database.get_connection_or_alias(target)? {
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

            // Update last used timestamp (best-effort — see `touch_connection`).
            connection.update_last_used();
            self.database.touch_connection(&connection)?;

            return self.execute_ssh(&connection, record).await;
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

        self.execute_ssh(&connection, record).await
    }

    async fn execute_ssh(&self, connection: &Connection, record: Option<bool>) -> Result<()> {
        info!("Executing SSH connection to {}", connection.host);

        // Check and create Kerberos ticket if needed
        if connection.use_kerberos {
            self.ensure_kerberos_ticket().await?;
        }

        // Choose transport based on connection properties.
        let kind = crate::services::transport::pick_kind(connection, &self.config);
        info!("Using transport: {:?}", kind);

        // Create session record before running. Mark it active and record
        // the pid in one INSERT (previously this was two DB writes: an
        // insert of the Starting state followed by an update to Active).
        let mut session = Session::new(connection.clone());
        session.transport = Some(format!("{kind:?}").to_lowercase());
        session.context = crate::services::context::capture(&self.database, &self.config);
        session.mark_active(std::process::id());
        // A direct connection to a host that isn't saved has no row for the
        // session to point at, so its session goes unrecorded.
        let stored = self
            .database
            .get_connection(&connection.id.to_string())?
            .is_some();

        let recorder = if crate::services::recording::should_record(
            connection,
            &self.config.recording,
            record,
        ) {
            let path = crate::services::recording::recording_path(&self.config, session.id);
            let (cols, rows) = crossterm::terminal::size().unwrap_or((
                crate::services::pty::DEFAULT_COLS,
                crate::services::pty::DEFAULT_ROWS,
            ));
            match crate::services::recording::Recorder::create(
                &path,
                cols,
                rows,
                &connection.name,
                self.config.recording.max_bytes_per_session,
            ) {
                Ok(rec) => {
                    session.recording_path = Some(path.to_string_lossy().to_string());
                    Some(std::sync::Arc::new(std::sync::Mutex::new(rec)))
                }
                Err(e) => {
                    warn!(
                        "Failed to start session recorder at '{}': {e}",
                        path.display()
                    );
                    session.recording_path = None;
                    None
                }
            }
        } else {
            None
        };

        if stored {
            self.database.add_session(&session)?;
        }

        let conn = connection.clone();
        let rec_for_transport = recorder.clone();
        let result = crate::services::transport::execute_with_fallback(
            connection,
            &self.config,
            |transport| {
                let conn_clone = conn.clone();
                let rec_clone = rec_for_transport.clone();
                Box::pin(async move { transport.run_interactive(&conn_clone, rec_clone).await })
            },
        )
        .await;
        drop(rec_for_transport);

        if let Some(rec_arc) = recorder {
            let text = match std::sync::Arc::try_unwrap(rec_arc) {
                Ok(mutex) => mutex
                    .into_inner()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .finish(),
                Err(arc) => arc
                    .lock()
                    .map(|mut g| g.finish_in_place())
                    .unwrap_or_default(),
            };
            if stored {
                if let Err(e) = self.database.index_session_text(session.id, &text) {
                    warn!("Failed to index recorded session text: {e}");
                }
            }
        }

        let outcome = match result {
            Ok(0) => {
                info!("SSH session completed successfully");
                session.mark_terminated(0);
                Ok(())
            }
            Ok(code) => {
                warn!("SSH session exited with code {code}");
                session.mark_terminated(code);
                Ok(())
            }
            Err(e) => {
                error!("SSH transport error: {e}");
                session.mark_error(format!("{e}"));
                Err(anyhow::anyhow!("{}", e))
            }
        };

        if stored {
            self.database.update_session(&session)?;
        }

        if self.config.recording.retention_days > 0 {
            match self
                .database
                .prune_recordings(self.config.recording.retention_days)
            {
                Ok(paths) => {
                    for p in paths {
                        if let Err(e) = std::fs::remove_file(&p) {
                            if e.kind() != std::io::ErrorKind::NotFound {
                                warn!("Failed to remove expired recording '{p}': {e}");
                            }
                        }
                    }
                }
                Err(e) => warn!("Failed to prune old recordings: {e}"),
            }
        }

        outcome
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
    #[allow(dead_code)]
    pub async fn fuzzy_search(&self, query: &str, limit: usize) -> Result<Vec<Connection>> {
        self.database.fuzzy_search_connections(query, limit)
    }

    // Search with configurable mode (bayesian or fuzzy)
    pub async fn search(&self, query: &str, limit: usize, mode: &str) -> Result<Vec<Connection>> {
        let ctx = if self.config.prediction.enabled {
            Some(crate::database::RankContext::current(
                &self.database,
                &self.config,
            ))
        } else {
            None
        };
        self.database
            .search_connections_ctx(query, limit, mode, ctx.as_ref())
    }

    pub async fn get_recent_connections(&self, limit: usize) -> Result<Vec<Connection>> {
        self.database
            .list_connections(None, true)
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
        record: Option<bool>,
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

        // Update last used timestamp. We use `touch_connection` (a
        // best-effort UPDATE on `last_used` only) rather than the full
        // `update_connection` so a connect that races with a concurrent
        // `bssh remove` doesn't fail. Storing the session happens in
        // `execute_ssh` regardless.
        conn.update_last_used();
        self.database.touch_connection(&conn)?;

        // Execute the connection
        self.execute_ssh(&conn, record).await
    }
}
