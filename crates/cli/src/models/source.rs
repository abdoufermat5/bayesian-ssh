use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

/// Where a set of connections was discovered from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceKind {
    SshConfig,
    Ansible,
    Tailscale,
    Aws,
    Gcp,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SshConfig => "ssh-config",
            Self::Ansible => "ansible",
            Self::Tailscale => "tailscale",
            Self::Aws => "aws",
            Self::Gcp => "gcp",
        }
    }
}

impl fmt::Display for SourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SourceKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ssh-config" => Ok(Self::SshConfig),
            "ansible" => Ok(Self::Ansible),
            "tailscale" => Ok(Self::Tailscale),
            "aws" => Ok(Self::Aws),
            "gcp" => Ok(Self::Gcp),
            other => Err(format!(
                "unknown source kind '{}' (expected ssh-config, ansible, tailscale, aws or gcp)",
                other
            )),
        }
    }
}

/// A configured inventory that connections can be synced from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: Uuid,
    pub name: String,
    pub kind: SourceKind,
    pub config: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_sync_error: Option<String>,
}

impl Source {
    pub fn new(name: String, kind: SourceKind, config: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            kind,
            config,
            created_at: Utc::now(),
            last_sync_at: None,
            last_sync_error: None,
        }
    }
}

/// One host as reported by an inventory provider, before it is matched
/// against the connections already stored for the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredHost {
    /// Stable identifier inside the source (EC2 instance id, ssh alias, ...).
    pub external_id: String,
    pub name: String,
    pub host: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub key_path: Option<String>,
    pub tags: Vec<String>,
}
