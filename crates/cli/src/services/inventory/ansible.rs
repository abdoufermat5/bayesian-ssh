use super::{config_str, run_command, InventoryProvider};
use crate::models::{DiscoveredHost, Source};
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};
use std::collections::HashSet;

pub struct AnsibleProvider;

impl InventoryProvider for AnsibleProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>> {
        let inventory = config_str(&source.config, "inventory").ok_or_else(|| {
            anyhow!(
                "ansible source '{}' has no inventory file configured",
                source.name
            )
        })?;

        let output = run_command(
            "ansible-inventory",
            &["-i".to_string(), inventory, "--list".to_string()],
            "install ansible",
        )?;
        let value: Value =
            serde_json::from_str(&output).context("ansible-inventory returned invalid JSON")?;
        Ok(parse_inventory(&value))
    }
}

/// Turn `ansible-inventory --list` output into hosts. Group names become tags,
/// following `children` so a host inherits the tags of its parent groups.
fn parse_inventory(root: &Value) -> Vec<DiscoveredHost> {
    let hostvars = root
        .get("_meta")
        .and_then(|meta| meta.get("hostvars"))
        .and_then(|vars| vars.as_object());

    let groups: Map<String, Value> = root
        .as_object()
        .map(|object| {
            object
                .iter()
                .filter(|(name, value)| name.as_str() != "_meta" && value.is_object())
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect()
        })
        .unwrap_or_default();

    let mut names: Vec<String> = hostvars
        .map(|vars| vars.keys().cloned().collect())
        .unwrap_or_default();
    for group in groups.values() {
        if let Some(hosts) = group.get("hosts").and_then(|hosts| hosts.as_array()) {
            for host in hosts.iter().filter_map(|host| host.as_str()) {
                if !names.iter().any(|name| name == host) {
                    names.push(host.to_string());
                }
            }
        }
    }

    let empty = Map::new();
    names
        .into_iter()
        .map(|name| {
            let vars = hostvars
                .and_then(|vars| vars.get(&name))
                .and_then(|vars| vars.as_object())
                .unwrap_or(&empty);

            let host = vars
                .get("ansible_host")
                .and_then(|host| host.as_str())
                .unwrap_or(&name)
                .to_string();
            let port = vars
                .get("ansible_port")
                .and_then(|port| {
                    port.as_u64()
                        .map(|port| port as u16)
                        .or_else(|| port.as_str().and_then(|port| port.parse().ok()))
                })
                .filter(|&port| port != 0);

            let mut tags: Vec<String> = groups
                .keys()
                .filter(|group| group.as_str() != "all" && group.as_str() != "ungrouped")
                .filter(|group| group_contains(&groups, group, &name, &mut HashSet::new()))
                .cloned()
                .collect();
            tags.sort();
            tags.push("ansible".to_string());

            DiscoveredHost {
                external_id: name.clone(),
                name,
                host,
                user: vars
                    .get("ansible_user")
                    .and_then(|user| user.as_str())
                    .map(|user| user.to_string()),
                port,
                key_path: vars
                    .get("ansible_ssh_private_key_file")
                    .and_then(|key| key.as_str())
                    .map(|key| key.to_string()),
                tags,
            }
        })
        .collect()
}

/// Whether `host` belongs to `group`, directly or through `children`.
fn group_contains(
    groups: &Map<String, Value>,
    group: &str,
    host: &str,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(group.to_string()) {
        return false;
    }
    let Some(value) = groups.get(group) else {
        return false;
    };
    if value
        .get("hosts")
        .and_then(|hosts| hosts.as_array())
        .map(|hosts| hosts.iter().any(|entry| entry.as_str() == Some(host)))
        .unwrap_or(false)
    {
        return true;
    }
    value
        .get("children")
        .and_then(|children| children.as_array())
        .map(|children| {
            children
                .iter()
                .filter_map(|child| child.as_str())
                .any(|child| group_contains(groups, child, host, visited))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_hostvars_and_nested_group_tags() {
        let value = json!({
            "_meta": {
                "hostvars": {
                    "web01": {
                        "ansible_host": "10.0.0.5",
                        "ansible_user": "deploy",
                        "ansible_port": 2222,
                        "ansible_ssh_private_key_file": "~/.ssh/id_ed25519"
                    }
                }
            },
            "all": { "children": ["prod"] },
            "prod": { "children": ["web"] },
            "web": { "hosts": ["web01", "db01"] },
            "ungrouped": {}
        });

        let hosts = parse_inventory(&value);
        assert_eq!(hosts.len(), 2);

        let web = hosts.iter().find(|host| host.name == "web01").unwrap();
        assert_eq!(web.external_id, "web01");
        assert_eq!(web.host, "10.0.0.5");
        assert_eq!(web.user.as_deref(), Some("deploy"));
        assert_eq!(web.port, Some(2222));
        assert_eq!(web.key_path.as_deref(), Some("~/.ssh/id_ed25519"));
        assert_eq!(web.tags, ["prod", "web", "ansible"]);

        let db = hosts.iter().find(|host| host.name == "db01").unwrap();
        assert_eq!(db.host, "db01");
        assert_eq!(db.tags, ["prod", "web", "ansible"]);
    }
}
