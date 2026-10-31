//! Approval tier for a call that must ask.

use super::glob::{GlobMode, glob_match};
use crate::services::ai::types::{Analysis, HostFacts, Policy, RiskTag, Tier, ToolCall, ToolName};

/// A command is "low risk and reversible" when it was analyzed and carries none of the
/// dangerous tags.
pub(super) fn low_risk(analysis: Option<&Analysis>) -> bool {
    match analysis {
        Some(a) => {
            a.unknown_reason.is_none()
                && !a.tags.iter().any(|t| {
                    matches!(
                        t,
                        RiskTag::Destructive
                            | RiskTag::Privilege
                            | RiskTag::Network
                            | RiskTag::ExfilRisk
                            | RiskTag::Unknown
                    )
                })
        }
        None => false,
    }
}

/// Tier for something that must ask (design 6.3).
///
/// `host_name` is the effective target (the facts' name, else the call's host). Two-person
/// approval reads every tag, trusted or not, and `two_person_hosts` as well: a stricter rule
/// must not depend on who can edit a source. In-client approval reads trusted tags only.
pub(super) fn tier_for(
    policy: &Policy,
    call: &ToolCall,
    analysis: Option<&Analysis>,
    host: &HostFacts,
    host_name: &str,
) -> Tier {
    let two_person = policy
        .approval
        .two_person
        .iter()
        .any(|t| host.tags.contains(t))
        || policy
            .approval
            .two_person_hosts
            .iter()
            .any(|h| glob_match(h, host_name, GlobMode::Text));
    if two_person {
        return Tier::TwoPerson;
    }
    // `edit_file` and `service` have no shell command to analyze; both are undoable.
    let reversible_tool = matches!(call.tool, ToolName::EditFile | ToolName::Service);
    let in_client = host.tags_trusted
        && policy
            .approval
            .in_client
            .iter()
            .any(|t| host.tags.contains(t));
    if in_client && (low_risk(analysis) || reversible_tool) {
        return Tier::InClient;
    }
    policy.approval.default
}

/// The tier a forced ask should use (or any ask): the same rules the engine applies.
pub fn tier_for_call(
    policy: &Policy,
    call: &ToolCall,
    analysis: Option<&Analysis>,
    host: &HostFacts,
) -> Tier {
    let host_name = if host.name.is_empty() {
        call.host.clone().unwrap_or_default()
    } else {
        host.name.clone()
    };
    tier_for(policy, call, analysis, host, &host_name)
}
