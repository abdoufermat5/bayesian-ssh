use super::{config_bool, run_command, InventoryProvider};
use crate::models::{DiscoveredHost, Source};
use anyhow::{Context, Result};
use serde_json::Value;

pub struct TailscaleProvider;

impl InventoryProvider for TailscaleProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>> {
        let output = run_command(
            "tailscale",
            &["status".to_string(), "--json".to_string()],
            "install the Tailscale client",
        )?;
        let value: Value =
            serde_json::from_str(&output).context("tailscale status returned invalid JSON")?;
        Ok(parse_status(
            &value,
            config_bool(&source.config, "include_offline"),
        ))
    }
}

/// Turn `tailscale status --json` output into hosts.
fn parse_status(root: &Value, include_offline: bool) -> Vec<DiscoveredHost> {
    let Some(peers) = root.get("Peer").and_then(|peer| peer.as_object()) else {
        return Vec::new();
    };

    peers
        .values()
        .filter_map(|peer| {
            if !include_offline && peer.get("Online").and_then(|v| v.as_bool()) == Some(false) {
                return None;
            }

            let host = peer
                .get("DNSName")
                .and_then(|dns| dns.as_str())
                .map(|dns| dns.trim_end_matches('.').to_string())
                .filter(|dns| !dns.is_empty())
                .or_else(|| {
                    peer.get("TailscaleIPs")
                        .and_then(|ips| ips.as_array())
                        .and_then(|ips| ips.first())
                        .and_then(|ip| ip.as_str())
                        .map(|ip| ip.to_string())
                })?;

            let mut tags: Vec<String> = peer
                .get("Tags")
                .and_then(|tags| tags.as_array())
                .map(|tags| {
                    tags.iter()
                        .filter_map(|tag| tag.as_str())
                        .map(|tag| tag.strip_prefix("tag:").unwrap_or(tag).to_string())
                        .collect()
                })
                .unwrap_or_default();
            tags.push("tailscale".to_string());
            if let Some(os) = peer.get("OS").and_then(|os| os.as_str()) {
                if !os.is_empty() {
                    tags.push(os.to_lowercase());
                }
            }

            Some(DiscoveredHost {
                external_id: peer.get("ID").and_then(|id| id.as_str())?.to_string(),
                name: peer
                    .get("HostName")
                    .and_then(|name| name.as_str())?
                    .to_string(),
                host,
                user: None,
                port: None,
                key_path: None,
                tags,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_peers_skipping_offline_and_cleaning_tags() {
        let value = json!({
            "Self": { "ID": "self", "HostName": "laptop", "DNSName": "laptop.tail.ts.net." },
            "Peer": {
                "a": {
                    "ID": "peer-a",
                    "HostName": "web",
                    "DNSName": "web.tail.ts.net.",
                    "Online": true,
                    "OS": "linux",
                    "Tags": ["tag:prod", "tag:web"]
                },
                "b": {
                    "ID": "peer-b",
                    "HostName": "db",
                    "DNSName": "db.tail.ts.net.",
                    "Online": false
                }
            }
        });

        let hosts = parse_status(&value, false);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].external_id, "peer-a");
        assert_eq!(hosts[0].host, "web.tail.ts.net");
        assert_eq!(hosts[0].tags, ["prod", "web", "tailscale", "linux"]);

        let hosts = parse_status(&value, true);
        assert_eq!(hosts.len(), 2);
    }
}
