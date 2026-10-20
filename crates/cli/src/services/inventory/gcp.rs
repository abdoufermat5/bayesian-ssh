use super::{config_str, run_command, InventoryProvider};
use crate::models::{DiscoveredHost, Source};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tracing::warn;

pub struct GcpProvider;

impl InventoryProvider for GcpProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>> {
        let project = config_str(&source.config, "project")
            .ok_or_else(|| anyhow!("gcp source '{}' has no project configured", source.name))?;
        let address = config_str(&source.config, "address").unwrap_or_else(|| "internal".into());
        if !matches!(address.as_str(), "internal" | "external") {
            return Err(anyhow!(
                "gcp source '{}' has invalid address '{}' (expected internal or external)",
                source.name,
                address
            ));
        }
        let user = config_str(&source.config, "user");

        let output = run_command(
            "gcloud",
            &[
                "compute".to_string(),
                "instances".to_string(),
                "list".to_string(),
                "--project".to_string(),
                project,
                "--format=json".to_string(),
                "--filter=status=RUNNING".to_string(),
            ],
            "install the Google Cloud CLI",
        )?;
        let value: Value =
            serde_json::from_str(&output).context("gcloud instances list returned invalid JSON")?;
        let (hosts, skipped) = parse_instances(&value, &address, user);
        if skipped > 0 {
            warn!(
                "Skipped {} GCP instance(s) without a usable address",
                skipped
            );
        }
        Ok(hosts)
    }
}

fn parse_instances(
    value: &Value,
    address: &str,
    user: Option<String>,
) -> (Vec<DiscoveredHost>, usize) {
    let mut hosts = Vec::new();
    let mut skipped = 0;
    let instances = value
        .as_array()
        .map(|instances| instances.as_slice())
        .unwrap_or(&[]);

    for instance in instances {
        let Some(id) = instance.get("id").and_then(|id| id.as_str()) else {
            continue;
        };
        let Some(name) = instance.get("name").and_then(|name| name.as_str()) else {
            continue;
        };

        let network = instance
            .get("networkInterfaces")
            .and_then(|interfaces| interfaces.as_array())
            .and_then(|interfaces| interfaces.first());
        let host = match address {
            "external" => network
                .and_then(|network| network.get("accessConfigs"))
                .and_then(|configs| configs.as_array())
                .and_then(|configs| configs.first())
                .and_then(|config| config.get("natIP"))
                .and_then(|ip| ip.as_str()),
            _ => network
                .and_then(|network| network.get("networkIP"))
                .and_then(|ip| ip.as_str()),
        }
        .filter(|ip| !ip.is_empty());
        let Some(host) = host else {
            skipped += 1;
            continue;
        };

        let mut tags: Vec<String> = instance
            .get("labels")
            .and_then(|labels| labels.as_object())
            .map(|labels| {
                labels
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}={}",
                            key.to_lowercase(),
                            value.as_str().unwrap_or_default().to_lowercase()
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        tags.sort();
        tags.push("gcp".to_string());
        if let Some(zone) = instance.get("zone").and_then(|zone| zone.as_str()) {
            if let Some(segment) = zone
                .rsplit('/')
                .next()
                .filter(|segment| !segment.is_empty())
            {
                tags.push(segment.to_string());
            }
        }

        hosts.push(DiscoveredHost {
            external_id: id.to_string(),
            name: name.to_string(),
            host: host.to_string(),
            user: user.clone(),
            port: None,
            key_path: None,
            tags,
        });
    }

    (hosts, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_internal_and_external_addresses() {
        let value = json!([{
            "id": "123",
            "name": "web",
            "zone": "projects/demo/zones/europe-west1-b",
            "labels": { "Env": "Prod" },
            "networkInterfaces": [{
                "networkIP": "10.0.0.9",
                "accessConfigs": [{ "natIP": "34.1.2.3" }]
            }]
        }]);

        let (internal, skipped) = parse_instances(&value, "internal", None);
        assert_eq!(skipped, 0);
        assert_eq!(internal[0].host, "10.0.0.9");
        assert_eq!(internal[0].external_id, "123");
        assert_eq!(internal[0].name, "web");
        assert_eq!(internal[0].tags, ["env=prod", "gcp", "europe-west1-b"]);

        let (external, _) = parse_instances(&value, "external", None);
        assert_eq!(external[0].host, "34.1.2.3");
    }
}
