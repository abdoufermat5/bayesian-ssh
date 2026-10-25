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
    /// Inventory source that owns this connection (`sources.id`); `None` for
    /// connections added by hand.
    #[serde(default)]
    pub source_id: Option<Uuid>,
    /// Stable id of the host inside its source (e.g. an EC2 instance id).
    #[serde(default)]
    pub external_id: Option<String>,
    /// Set when the last sync of `source_id` no longer listed this host.
    #[serde(default)]
    pub stale_since: Option<DateTime<Utc>>,
    /// Pass `ssh -A` (forward the local ssh-agent) to this host. Local and
    /// deliberate: a sync never turns it on, and a team sync turns it off
    /// when it changes the connection's route.
    #[serde(default)]
    pub forward_agent: bool,
    /// Route changes a team sync made, waiting for the user to confirm them
    /// before the next connect. Never set from local edits.
    #[serde(default)]
    pub pending_change: Option<PendingChange>,
}

/// Route fields a remote sync must not change silently: they decide which
/// host receives the user's credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteValues {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub bastion: Option<String>,
    pub bastion_user: Option<String>,
    pub use_kerberos: bool,
}

impl RouteValues {
    pub fn of(connection: &Connection) -> Self {
        Self {
            host: connection.host.clone(),
            port: connection.port,
            user: connection.user.clone(),
            bastion: connection.bastion.clone(),
            bastion_user: connection.bastion_user.clone(),
            use_kerberos: connection.use_kerberos,
        }
    }
}

/// A route change detected by a team sync, with the values it replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingChange {
    pub detected_at: DateTime<Utc>,
    pub previous: RouteValues,
}

impl PendingChange {
    /// Remember the route `connection` had before a sync replaced it.
    pub fn detect(previous: &Connection, at: DateTime<Utc>) -> Self {
        Self {
            detected_at: at,
            previous: RouteValues::of(previous),
        }
    }

    /// `field: old → new` lines for the fields that differ from `current`.
    pub fn describe(&self, current: &Connection) -> Vec<String> {
        let old = &self.previous;
        let new = RouteValues::of(current);
        let mut lines = Vec::new();
        let mut push = |label: &str, before: String, after: String| {
            if before != after {
                lines.push(format!("{label}: {before} → {after}"));
            }
        };
        push("host", old.host.clone(), new.host);
        push("port", old.port.to_string(), new.port.to_string());
        push("user", old.user.clone(), new.user);
        push(
            "bastion",
            old.bastion.clone().unwrap_or_else(|| "none".into()),
            new.bastion.unwrap_or_else(|| "none".into()),
        );
        push(
            "bastion user",
            old.bastion_user.clone().unwrap_or_else(|| "none".into()),
            new.bastion_user.unwrap_or_else(|| "none".into()),
        );
        push(
            "kerberos",
            old.use_kerberos.to_string(),
            new.use_kerberos.to_string(),
        );
        lines
    }
}

/// Split a stored bastion (`host`, `host:port`, `[v6]` or `[v6]:port`) into
/// the destination ssh accepts and an explicit port, when one is given.
pub fn split_bastion(bastion: &str) -> (&str, Option<u16>) {
    if let Some(rest) = bastion.strip_prefix('[') {
        let Some(inner_len) = rest.find(']') else {
            return (bastion, None);
        };
        let host = &bastion[..inner_len + 2];
        let port = rest[inner_len + 1..].strip_prefix(':').and_then(parse_port);
        return (host, port);
    }
    // An unbracketed IPv6 address never carries a port here.
    if bastion.matches(':').count() > 1 {
        return (bastion, None);
    }
    match bastion.split_once(':') {
        Some((host, port)) => match parse_port(port) {
            Some(port) => (host, Some(port)),
            None => (bastion, None),
        },
        None => (bastion, None),
    }
}

fn parse_port(value: &str) -> Option<u16> {
    value.parse::<u16>().ok().filter(|p| *p != 0)
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
            source_id: None,
            external_id: None,
            stale_since: None,
            forward_agent: false,
            pending_change: None,
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
            cmd.push_str("ssh -t ");
        } else {
            cmd.push_str("ssh ");
        }
        if self.forward_agent {
            cmd.push_str("-A ");
        }
        if self.use_kerberos {
            cmd.push_str("-K ");
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
            // The bastion's port is only pinned when the stored value has
            // one; otherwise ~/.ssh/config decides.
            Some(bastion) if self.use_kerberos => {
                let bastion_user = self.bastion_user.as_deref().unwrap_or(&self.user);
                let (bastion_host, bastion_port) = split_bastion(bastion);
                if let Some(port) = bastion_port {
                    cmd.push_str(&format!("-p {} ", port));
                }
                cmd.push_str(&format!("{}@{}", bastion_user, bastion_host));
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
    use super::{split_bastion, Connection, PendingChange};
    use chrono::Utc;

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
        // Agent forwarding is opt-in: the default Kerberos setup only
        // delegates the TGT (-K), never the agent (-A).
        assert_eq!(
            conn(Some("bastion.example"), true).to_ssh_command(),
            "ssh -t -K jump@bastion.example alice@web.internal"
        );
        let mut forwarded = conn(Some("bastion.example"), true);
        forwarded.forward_agent = true;
        assert_eq!(
            forwarded.to_ssh_command(),
            "ssh -t -A -K jump@bastion.example alice@web.internal"
        );
    }

    #[test]
    fn ssh_command_uses_the_bastion_port_when_stored() {
        assert_eq!(
            conn(Some("bastion.example:2222"), true).to_ssh_command(),
            "ssh -t -K -p 2222 jump@bastion.example alice@web.internal"
        );
        assert_eq!(
            conn(Some("[::1]:2200"), true).to_ssh_command(),
            "ssh -t -K -p 2200 jump@[::1] alice@web.internal"
        );
    }

    #[test]
    fn split_bastion_parses_hosts_and_ports() {
        assert_eq!(split_bastion("bastion.example"), ("bastion.example", None));
        assert_eq!(
            split_bastion("bastion.example:2222"),
            ("bastion.example", Some(2222))
        );
        assert_eq!(split_bastion("[::1]"), ("[::1]", None));
        assert_eq!(split_bastion("[::1]:2200"), ("[::1]", Some(2200)));
        // An unbracketed IPv6 address is all host; a non-port suffix stays host.
        assert_eq!(split_bastion("::1"), ("::1", None));
        assert_eq!(split_bastion("host:ssh"), ("host:ssh", None));
        assert_eq!(split_bastion("host:0"), ("host:0", None));
    }

    #[test]
    fn pending_change_describes_only_the_changed_fields() {
        let mut old = conn(Some("bastion.example"), false);
        old.forward_agent = true;
        let mut new = old.clone();
        new.host = "attacker.example".into();
        new.use_kerberos = true;
        new.forward_agent = false;
        let change = PendingChange::detect(&old, Utc::now());
        assert_eq!(
            change.describe(&new),
            vec![
                "host: web.internal → attacker.example".to_string(),
                "kerberos: false → true".to_string(),
            ]
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
