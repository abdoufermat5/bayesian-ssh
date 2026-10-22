use super::{config_str, config_strings, run_command, InventoryProvider};
use crate::models::{DiscoveredHost, Source};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tracing::warn;

pub struct AwsProvider;

impl InventoryProvider for AwsProvider {
    fn fetch(&self, source: &Source) -> Result<Vec<DiscoveredHost>> {
        let regions = config_strings(&source.config, "regions");
        if regions.is_empty() {
            return Err(anyhow!(
                "aws source '{}' has no regions configured",
                source.name
            ));
        }
        let address = config_str(&source.config, "address").unwrap_or_else(|| "private".into());
        if !matches!(address.as_str(), "private" | "public" | "dns") {
            return Err(anyhow!(
                "aws source '{}' has invalid address '{}' (expected private, public or dns)",
                source.name,
                address
            ));
        }
        let profile = config_str(&source.config, "profile");
        let user = config_str(&source.config, "user");

        let mut hosts = Vec::new();
        let mut skipped = 0;
        for region in regions {
            let mut args = vec![
                "ec2".to_string(),
                "describe-instances".to_string(),
                "--region".to_string(),
                region.clone(),
                "--output".to_string(),
                "json".to_string(),
                "--filters".to_string(),
                "Name=instance-state-name,Values=running".to_string(),
            ];
            if let Some(profile) = &profile {
                args.push("--profile".to_string());
                args.push(profile.clone());
            }

            let output = run_command("aws", &args, "install the AWS CLI")?;
            let value: Value = serde_json::from_str(&output)
                .context("aws describe-instances returned invalid JSON")?;
            let (mut region_hosts, region_skipped) =
                parse_instances(&value, &address, user.clone(), &region);
            hosts.append(&mut region_hosts);
            skipped += region_skipped;
        }

        if skipped > 0 {
            warn!(
                "Skipped {} AWS instance(s) without a usable address",
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
    region: &str,
) -> (Vec<DiscoveredHost>, usize) {
    let mut hosts = Vec::new();
    let mut skipped = 0;
    let reservations = value
        .get("Reservations")
        .and_then(|reservations| reservations.as_array())
        .map(|reservations| reservations.as_slice())
        .unwrap_or(&[]);

    for instance in reservations.iter().flat_map(|reservation| {
        reservation
            .get("Instances")
            .and_then(|instances| instances.as_array())
            .map(|instances| instances.as_slice())
            .unwrap_or(&[])
    }) {
        let Some(instance_id) = instance.get("InstanceId").and_then(|id| id.as_str()) else {
            continue;
        };

        let address_value = match address {
            "public" => instance.get("PublicIpAddress"),
            "dns" => instance.get("PublicDnsName"),
            _ => instance.get("PrivateIpAddress"),
        }
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty());
        let Some(host) = address_value else {
            skipped += 1;
            continue;
        };

        let mut name = None;
        let mut tags = Vec::new();
        if let Some(instance_tags) = instance.get("Tags").and_then(|tags| tags.as_array()) {
            for tag in instance_tags {
                let key = tag
                    .get("Key")
                    .and_then(|key| key.as_str())
                    .unwrap_or_default();
                let value = tag
                    .get("Value")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                if key == "Name" {
                    name = Some(value.to_string());
                } else if !key.is_empty() {
                    tags.push(format!("{}={}", key.to_lowercase(), value.to_lowercase()));
                }
            }
        }
        tags.push("aws".to_string());
        tags.push(region.to_string());

        hosts.push(DiscoveredHost {
            external_id: instance_id.to_string(),
            name: name
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| instance_id.to_string()),
            host: host.to_string(),
            user: user.clone(),
            port: None,
            key_path: None,
            tags,
            ..Default::default()
        });
    }

    (hosts, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_name_tag_and_skips_instances_without_private_ip() {
        let value = json!({
            "Reservations": [{
                "Instances": [
                    {
                        "InstanceId": "i-123",
                        "PrivateIpAddress": "10.0.0.7",
                        "Tags": [
                            { "Key": "Name", "Value": "web" },
                            { "Key": "Env", "Value": "Prod" }
                        ]
                    },
                    { "InstanceId": "i-456", "Tags": [] }
                ]
            }]
        });

        let (hosts, skipped) =
            parse_instances(&value, "private", Some("ec2-user".into()), "eu-west-1");
        assert_eq!(skipped, 1);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].external_id, "i-123");
        assert_eq!(hosts[0].name, "web");
        assert_eq!(hosts[0].host, "10.0.0.7");
        assert_eq!(hosts[0].user.as_deref(), Some("ec2-user"));
        assert_eq!(hosts[0].tags, ["env=prod", "aws", "eu-west-1"]);
    }
}
