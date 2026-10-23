//! Organizations as the CLI shows them: an organization has no name on the
//! server, so it is labelled by its owners' emails, and `<org>` arguments
//! accept an id prefix or the name of one of its teams. Pure on purpose: the
//! selection rules are tested without a server.

use anyhow::{bail, Result};
use bssh_teams_proto::api::OrgInfo;
use uuid::Uuid;

/// First 8 hex digits of an id, the same shorthand sync and joins use.
pub fn short_id(id: &Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

/// The organization's label: the emails of its owners, e.g.
/// `"ana@example.com, bo@example.com"`. Falls back to the short id for an
/// organization that somehow lists no owner.
pub fn label(org: &OrgInfo) -> String {
    if org.owners.is_empty() {
        return format!("organization {}", short_id(&org.org_id));
    }
    org.owners
        .iter()
        .map(|o| o.email.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A team of the organization: its local name when this machine knows it,
/// else its short id.
pub fn team_label(team_id: Uuid, local_teams: &[(Uuid, String)]) -> String {
    local_teams
        .iter()
        .find(|(id, _)| *id == team_id)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| short_id(&team_id))
}

/// The organizations to choose from, one per line: `label (short id)`.
fn choices(orgs: &[OrgInfo]) -> String {
    orgs.iter()
        .map(|o| format!("  {} ({})", label(o), short_id(&o.org_id)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Pick the organization an `<org>` argument names.
///
/// `query` is an organization id prefix or the name of a local team (meaning
/// that team's organization); `None` means "the only one", so it is an error
/// when the account sees several. `local_teams` is this machine's
/// `(team_id, name)` list, used for team names.
pub fn select(
    orgs: &[OrgInfo],
    query: Option<&str>,
    local_teams: &[(Uuid, String)],
) -> Result<Uuid> {
    if orgs.is_empty() {
        bail!("this account has no organizations yet: create one with `bssh org create`");
    }
    let Some(query) = query.map(str::trim).filter(|q| !q.is_empty()) else {
        return match orgs {
            [one] => Ok(one.org_id),
            _ => bail!(
                "you see {} organizations; name one by an id prefix or a team name:\n{}",
                orgs.len(),
                choices(orgs)
            ),
        };
    };
    // A local team name is the friendliest way to name its organization.
    if let Some((team_id, _)) = local_teams.iter().find(|(_, name)| name == query) {
        if let Some(org) = orgs.iter().find(|o| o.team_ids.contains(team_id)) {
            return Ok(org.org_id);
        }
        bail!(
            "'{query}' is not in an organization this account can see:\n{}",
            choices(orgs)
        );
    }
    let prefix = query.to_ascii_lowercase();
    let matches: Vec<_> = orgs
        .iter()
        .filter(|o| o.org_id.to_string().starts_with(&prefix))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.org_id),
        [] => bail!(
            "no organization matches '{query}'; name one by an id prefix or a team name:\n{}",
            choices(orgs)
        ),
        _ => bail!(
            "'{query}' matches several organizations; use more of the id:\n{}",
            choices(orgs)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bssh_teams_proto::api::{OrgOwner, OrgRole};
    use chrono::Utc;

    fn org(id: u128, emails: &[&str], team_ids: Vec<Uuid>) -> OrgInfo {
        OrgInfo {
            org_id: Uuid::from_u128(id),
            my_role: OrgRole::Owner,
            owners: emails
                .iter()
                .enumerate()
                .map(|(i, email)| OrgOwner {
                    account_id: Uuid::from_u128(i as u128 + 1),
                    email: email.to_string(),
                })
                .collect(),
            team_ids,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn label_joins_owner_emails() {
        assert_eq!(
            label(&org(1, &["ana@example.com", "bo@example.com"], vec![])),
            "ana@example.com, bo@example.com"
        );
        assert_eq!(
            label(&org(1, &["ana@example.com"], vec![])),
            "ana@example.com"
        );
        assert_eq!(
            label(&org(0xabcd, &[], vec![])),
            format!("organization {}", short_id(&Uuid::from_u128(0xabcd)))
        );
    }

    #[test]
    fn team_label_prefers_local_name() {
        let id = Uuid::from_u128(0x1234_5678_9abc_def0_0000_0000_0000_0001);
        let local = vec![(id, "platform".to_string())];
        assert_eq!(team_label(id, &local), "platform");
        let unknown = Uuid::from_u128(0x2222_3333_4444_5555_6666_7777_8888_9999);
        assert_eq!(team_label(unknown, &local), "22223333");
    }

    #[test]
    fn a_single_org_needs_no_name() {
        let only = org(1, &["ana@example.com"], vec![]);
        let one = std::slice::from_ref(&only);
        assert_eq!(select(one, None, &[]).unwrap(), only.org_id);
        // An empty string is "not given".
        assert_eq!(select(one, Some("  "), &[]).unwrap(), only.org_id);
    }

    #[test]
    fn no_org_is_an_error() {
        let err = select(&[], None, &[]).unwrap_err().to_string();
        assert!(err.contains("bssh org create"), "{err}");
    }

    #[test]
    fn several_orgs_must_be_named() {
        let orgs = vec![
            org(
                0x1111_0000_0000_0000_0000_0000_0000_0000,
                &["ana@example.com"],
                vec![],
            ),
            org(
                0x2222_0000_0000_0000_0000_0000_0000_0000,
                &["bo@example.com"],
                vec![],
            ),
        ];
        let err = select(&orgs, None, &[]).unwrap_err().to_string();
        assert!(err.contains("you see 2 organizations"), "{err}");
        assert!(err.contains("ana@example.com (11110000)"), "{err}");
        assert!(err.contains("bo@example.com (22220000)"), "{err}");
    }

    #[test]
    fn an_id_prefix_names_an_org() {
        let wide = Uuid::from_u128(0x1234_0000_0000_0000_0000_0000_0000_0001);
        let narrow = Uuid::from_u128(0x1235_0000_0000_0000_0000_0000_0000_0002);
        let orgs = vec![
            OrgInfo {
                org_id: wide,
                ..org(0, &["a@example.com"], vec![])
            },
            OrgInfo {
                org_id: narrow,
                ..org(0, &["b@example.com"], vec![])
            },
        ];
        // Upper case is accepted; uuid formatting is lower case.
        assert_eq!(
            select(&orgs, Some(&wide.to_string()[..6].to_uppercase()), &[]).unwrap(),
            wide
        );
        let err = select(&orgs, Some("123"), &[]).unwrap_err().to_string();
        assert!(err.contains("matches several organizations"), "{err}");
        let err = select(&orgs, Some("9999"), &[]).unwrap_err().to_string();
        assert!(err.contains("no organization matches '9999'"), "{err}");
    }

    #[test]
    fn a_local_team_name_names_its_org() {
        let team_id = Uuid::from_u128(9);
        let orgs = vec![
            org(1, &["ana@example.com"], vec![]),
            org(2, &["bo@example.com"], vec![team_id]),
        ];
        let local = vec![(team_id, "platform".to_string())];
        assert_eq!(
            select(&orgs, Some("platform"), &local).unwrap(),
            orgs[1].org_id
        );

        let hidden = vec![(Uuid::from_u128(10), "secret".to_string())];
        let err = select(&orgs, Some("secret"), &hidden)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not in an organization"), "{err}");
    }
}
