use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
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
}

impl Connection {
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

    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
    }

    pub fn to_ssh_command(&self) -> String {
        let mut cmd = String::new();
        
        if self.use_kerberos {
            cmd.push_str("ssh -t -A -K ");
        } else {
            cmd.push_str("ssh ");
        }
        
        if let Some(key) = &self.key_path {
            cmd.push_str(&format!("-i {} ", key));
        }
        
        if let Some(bastion) = &self.bastion {
            let bastion_user = self.bastion_user.as_deref().unwrap_or(&self.user);
            cmd.push_str(&format!("-p 22 {}@{}", bastion_user, bastion));
            cmd.push_str(&format!(" {}@{}", self.user, self.host));
        } else {
            cmd.push_str(&format!("-p {} {}@{}", self.port, self.user, self.host));
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
