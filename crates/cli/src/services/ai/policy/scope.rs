//! Which hosts a policy covers and whether a host's tags may relax it.

use super::glob::{glob_match, GlobMode};
use crate::models::Connection;
use crate::services::ai::types::{Enforcement, HostFacts, Policy, Scope};

/// Scope tag meaning "every host". Used by the built-in default policy.
pub const ALL_HOSTS: &str = "*";

/// Is this connection visible to the agent at all?
///
/// `tags_trusted` says whether the connection's tags may widen the scope; see
/// [`tags_trusted`] for how to decide it.
pub fn host_in_scope(policy: &Policy, conn: &Connection, tags_trusted: bool) -> bool {
    in_scope(
        &policy.scope,
        &conn.name,
        &conn.tags,
        TagTrust::from_flag(tags_trusted),
    )
}

/// Whether a host's tags may widen the scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TagTrust {
    Trusted,
    Untrusted,
}

impl TagTrust {
    pub(super) fn from_flag(trusted: bool) -> Self {
        if trusted {
            TagTrust::Trusted
        } else {
            TagTrust::Untrusted
        }
    }
}

pub(super) fn in_scope(scope: &Scope, name: &str, tags: &[String], trust: TagTrust) -> bool {
    scope.tags.iter().any(|t| t == ALL_HOSTS)
        || (trust == TagTrust::Trusted && scope.tags.iter().any(|t| tags.iter().any(|h| h == t)))
        || scope
            .hosts
            .iter()
            .any(|h| glob_match(h, name, GlobMode::Text))
}

/// May this connection's tags relax the policy?
///
/// Connections added by hand have no source, so bssh owns their tags. A source-managed
/// connection is trusted only when its source name is listed in `ai.trusted_tag_sources`.
pub fn tags_trusted(
    trusted_sources: &[String],
    conn: &Connection,
    source_name: Option<&str>,
) -> bool {
    conn.source_id.is_none()
        || source_name.is_some_and(|name| trusted_sources.iter().any(|t| t == name))
}

/// [`HostFacts`] for a connection, with the trust of its tags.
pub fn connection_facts(
    conn: &Connection,
    source_name: Option<&str>,
    trusted_sources: &[String],
    enforcement: Enforcement,
) -> HostFacts {
    HostFacts {
        name: conn.name.clone(),
        tags: conn.tags.clone(),
        tags_trusted: tags_trusted(trusted_sources, conn, source_name),
        tag_source: source_name.map(str::to_string),
        enforcement,
        ..HostFacts::default()
    }
}

/// Tags that would have relaxed the decision but were ignored because their source is not
/// trusted, plus how to trust it. `rules` is `scope.tags` or `approval.in_client`.
pub(super) fn ignored_tag_note(rules: &[String], host: &HostFacts) -> Option<String> {
    if host.tags_trusted {
        return None;
    }
    let matched: Vec<&str> = rules
        .iter()
        .filter(|t| host.tags.iter().any(|h| h == *t))
        .map(String::as_str)
        .collect();
    if matched.is_empty() {
        return None;
    }
    let from = match &host.tag_source {
        Some(source) => format!("inventory source '{source}'"),
        None => "a source not listed in ai.trusted_tag_sources".to_string(),
    };
    Some(format!(
        "tags [{}] from {from} were ignored; add the source name to ai.trusted_tag_sources to let its tags relax the policy",
        matched.join(", ")
    ))
}
