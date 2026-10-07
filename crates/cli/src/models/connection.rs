use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    pub bastion: Option<String>,
    pub bastion_user: Option<String>,
    pub use_kerberos: bool,
    pub key_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_used: Option<DateTime<Utc>>,
    pub tags: Vec<String>,
    /// Aliases for this connection (not stored in main table, loaded separately)
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl Connection {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        host: String,
        user: String,
        port: u16,
        bastion: Option<String>,
        bastion_user: Option<String>,
        use_kerberos: bool,
        key_path: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            host,
            user,
            port,
            bastion,
            bastion_user,
            use_kerberos,
            key_path,
            created_at: Utc::now(),
            last_used: None,
            tags: Vec::new(),
            aliases: Vec::new(),
        }
    }

    pub fn update_last_used(&mut self) {
        self.last_used = Some(Utc::now());
    }

    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
        }
    }

    /// Remove a tag, matching on the normalized form so the stored
    /// (normalized) tag is found even if the argument has stray whitespace
    /// or quotes.
    pub fn remove_tag(&mut self, tag: &str) {
        let target = Self::normalize_tag(tag);
        self.tags.retain(|t| Self::normalize_tag(t) != target);
    }

    /// Normalize a tag: trim whitespace, reject empty values, escape
    /// surrounding quotes. Used everywhere user-supplied tag strings cross
    /// the SQL boundary.
    pub fn normalize_tag(tag: &str) -> Option<String> {
        let t = tag.trim().trim_matches('"').trim_matches('\'').trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }

    pub fn to_ssh_command(&self) -> String {
        let mut cmd = String::new();

        if self.use_kerberos {
            cmd.push_str("ssh -t -A -K ");
        } else {
            cmd.push_str("ssh ");
        }

        if let Some(key) = &self.key_path {
            // Validation rejects `'` in key paths, so single quotes are safe.
            if key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/._~-".contains(c))
            {
                cmd.push_str(&format!("-i {} ", key));
            } else {
                cmd.push_str(&format!("-i '{}' ", key));
            }
        }

        match &self.bastion {
            // Kerberos + bastion is an interactive bastion: log into the
            // bastion and hand it the target (see `build_shell_argv`).
            Some(bastion) if self.use_kerberos => {
                let bastion_user = self.bastion_user.as_deref().unwrap_or(&self.user);
                cmd.push_str(&format!("-p 22 {}@{}", bastion_user, bastion));
                cmd.push_str(&format!(" {}@{}", self.user, self.host));
            }
            // Otherwise the bastion is a plain jump host. The previous
            // `ssh bastion user@host` form would run `user@host` as a
            // remote command on the bastion and ignored the target port.
            Some(bastion) => {
                let bastion_user = self.bastion_user.as_deref().unwrap_or(&self.user);
                cmd.push_str(&format!(
                    "-J {}@{} -p {} {}@{}",
                    bastion_user, bastion, self.port, self.user, self.host
                ));
            }
            None => {
                cmd.push_str(&format!("-p {} {}@{}", self.port, self.user, self.host));
            }
        }

        cmd
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStats {
    pub total_connections: usize,
    pub most_used: Option<Connection>,
    pub recently_used: Vec<Connection>,
    pub by_tag: std::collections::HashMap<String, usize>,
}

#[cfg(test)]
mod tests {
    use super::Connection;

    fn conn(bastion: Option<&str>, kerberos: bool) -> Connection {
        Connection::new(
            "web".into(),
            "web.internal".into(),
            "alice".into(),
            2222,
            bastion.map(Into::into),
            Some("jump".into()),
            kerberos,
            None,
        )
    }

    #[test]
    fn ssh_command_uses_jump_host_for_plain_bastion() {
        assert_eq!(
            conn(Some("bastion.example"), false).to_ssh_command(),
            "ssh -J jump@bastion.example -p 2222 alice@web.internal"
        );
    }

    #[test]
    fn ssh_command_keeps_interactive_kerberos_bastion() {
        assert_eq!(
            conn(Some("bastion.example"), true).to_ssh_command(),
            "ssh -t -A -K -p 22 jump@bastion.example alice@web.internal"
        );
    }

    #[test]
    fn remove_tag_matches_normalized_form() {
        let mut c = conn(None, false);
        c.add_tag("prod".into());
        c.remove_tag(" prod ");
        assert!(c.tags.is_empty());
    }
}
