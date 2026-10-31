//! The words an approval is described in: what kind of request it is, which host, what the
//! call does, and how long a duration lasts.

use crate::services::ai::effects::Verb;
use crate::services::ai::types::{
    ApprovalRequest, ApprovalSubject, ServiceAction, Tier, ToolCall, ToolName,
};
use crate::services::ai::ui;
use chrono::{DateTime, Utc};

/// `4:41` as "expires in 4:41"; "expired" once past.
pub(super) fn expiry_text(r: &ApprovalRequest, now: DateTime<Utc>) -> String {
    match ui::expires_in(&r.expires_at, now).as_str() {
        "expired" => "expired".into(),
        left => format!("expires in {left}"),
    }
}

pub(super) fn kind_word(r: &ApprovalRequest) -> &'static str {
    match &r.subject {
        ApprovalSubject::Plan { .. } => "plan",
        ApprovalSubject::Access { .. } => "access",
        ApprovalSubject::Call { call, .. } => match call.tool {
            ToolName::Run => "run",
            ToolName::Service => "service",
            ToolName::EditFile => "edit",
            ToolName::ApplyChange => "apply",
            ToolName::Undo => "undo",
            _ => "call",
        },
    }
}

pub(super) fn request_host(r: &ApprovalRequest) -> String {
    match &r.subject {
        ApprovalSubject::Call { call, .. } => call.host.clone().unwrap_or_else(|| "-".into()),
        ApprovalSubject::Plan { plan, .. } => {
            let mut hosts: Vec<&str> = plan
                .steps
                .iter()
                .filter_map(|s| s.host.as_deref())
                .collect();
            hosts.dedup();
            match hosts.as_slice() {
                [] => "-".into(),
                [one] => (*one).to_string(),
                many => format!("{} hosts", many.len()),
            }
        }
        ApprovalSubject::Access { scope, .. } => match scope.hosts.as_slice() {
            [] => "-".into(),
            [one] => one.clone(),
            many => format!("{} hosts", many.len()),
        },
    }
}

/// What the request is, for one-line listings.
pub(super) fn request_title(r: &ApprovalRequest) -> String {
    match &r.subject {
        ApprovalSubject::Plan { plan, .. } => plan.title.clone(),
        ApprovalSubject::Access { duration, .. } => {
            format!("Wider access for {}", human_duration(duration))
        }
        ApprovalSubject::Call { call, .. } => call
            .command
            .as_deref()
            .map(|c| c.split_whitespace().collect::<Vec<_>>().join(" "))
            .or_else(|| call.path.clone())
            .unwrap_or_else(|| call_title(call)),
    }
}
/// `30m` becomes `30 min`, `2h` becomes `2 h`; anything else stays as written.
pub(super) fn human_duration(d: &str) -> String {
    let digits: String = d.chars().take_while(char::is_ascii_digit).collect();
    let unit = &d[digits.len()..];
    let word = match unit {
        "s" => "s",
        "m" => "min",
        "h" => "h",
        "d" => "days",
        _ => return d.to_string(),
    };
    if digits.is_empty() {
        d.to_string()
    } else {
        format!("{digits} {word}")
    }
}

pub(super) fn service_verb(action: &str) -> Option<Verb> {
    match action {
        "start" => Some(Verb::Starts),
        "stop" => Some(Verb::Stops),
        "restart" => Some(Verb::Restarts),
        "reload" => Some(Verb::Reloads),
        _ => None,
    }
}

pub(super) fn action_name(a: ServiceAction) -> &'static str {
    match a {
        ServiceAction::Start => "start",
        ServiceAction::Stop => "stop",
        ServiceAction::Restart => "restart",
        ServiceAction::Reload => "reload",
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// A single call in words: `Restart nginx on db-1`.
pub(super) fn call_title(call: &ToolCall) -> String {
    let on = call
        .host
        .as_deref()
        .map(|h| format!(" on {h}"))
        .unwrap_or_default();
    match call.tool {
        ToolName::Service => {
            let unit = call.args.get("unit").and_then(|v| v.as_str());
            let action = call.args.get("action").and_then(|v| v.as_str());
            match (action, unit) {
                (Some(a), Some(u)) => format!("{} {u}{on}", capitalize(a)),
                _ => format!("Change a service{on}"),
            }
        }
        ToolName::EditFile => match &call.path {
            Some(p) => format!("Edit {p}{on}"),
            None => format!("Edit a file{on}"),
        },
        ToolName::Run => format!("Run a command{on}"),
        ToolName::RunReadonly | ToolName::RunMany => format!("Run a read-only command{on}"),
        ToolName::ReadFile => match &call.path {
            Some(p) => format!("Read {p}{on}"),
            None => format!("Read a file{on}"),
        },
        ToolName::SearchLogs => format!("Search logs{on}"),
        ToolName::Observe => format!("Look at the host{on}"),
        ToolName::Undo => format!("Undo a change{on}"),
        ToolName::ApplyChange => format!("Apply a change{on}"),
        _ => format!("Run a tool{on}"),
    }
}

pub(super) fn tier_words(t: Tier) -> &'static str {
    match t {
        Tier::InClient => "confirm in the agent's client",
        Tier::OutOfBand => "one approver, outside the agent",
        Tier::TwoPerson => "two different people must approve",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_are_human() {
        assert_eq!(human_duration("30m"), "30 min");
        assert_eq!(human_duration("2h"), "2 h");
        assert_eq!(human_duration("until friday"), "until friday");
    }
}
