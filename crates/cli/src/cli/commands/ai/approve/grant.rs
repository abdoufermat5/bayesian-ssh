//! A grant for the same kind of call, valid for a few minutes, and the window the `g` key
//! offers it for.

use crate::services::ai::types::{ApprovalRequest, ApprovalSubject, Grant, ToolName};

/// Minutes a `g` keypress allows the same kind of call.
pub(super) const QUICK_GRANT_MINUTES: u32 = 15;

/// A grant for the same kind of call, valid `minutes` from now. Only single calls qualify.
pub(super) fn build_grant(r: &ApprovalRequest, minutes: u32) -> Option<Grant> {
    let ApprovalSubject::Call { call, .. } = &r.subject else {
        return None;
    };
    // A grant must name the host it covers: the broker refuses a grant without one, so
    // offering `*` here would only promise something that is then rejected.
    let hosts = call.host.clone()?;
    let target = match call.tool {
        ToolName::EditFile => call.path.clone(),
        ToolName::Service => call
            .args
            .get("unit")
            .and_then(|v| v.as_str())
            .map(String::from),
        _ => call.command.clone(),
    };
    Some(Grant {
        grant_id: uuid::Uuid::new_v4().to_string(),
        run_id: Some(r.run_id.clone()),
        tool: call.tool,
        hosts,
        target,
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(i64::from(minutes)))
            .to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{base, call, demo_plan_request, service_request};
    use super::*;
    use crate::services::ai::types::Tier;

    #[test]
    fn grant_for_a_call_targets_the_command_path_or_unit() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run, Some("systemctl restart nginx")),
            analysis: None,
        });
        let g = build_grant(&r, 30).unwrap();
        assert_eq!(g.tool, ToolName::Run);
        assert_eq!(g.hosts, "db-1");
        assert_eq!(g.target.as_deref(), Some("systemctl restart nginx"));
        assert_eq!(
            g.run_id.as_deref(),
            Some("dddbee19-f151-4ee0-8473-93924f35e1d4")
        );

        let mut c = call(ToolName::EditFile, None);
        c.path = Some("/etc/nginx/a.conf".into());
        let g = build_grant(
            &base(ApprovalSubject::Call {
                call: c,
                analysis: None,
            }),
            5,
        )
        .unwrap();
        assert_eq!(g.target.as_deref(), Some("/etc/nginx/a.conf"));

        let g = build_grant(&service_request(Tier::OutOfBand), 5).unwrap();
        assert_eq!(g.target.as_deref(), Some("nginx"));

        // A call without a host can't be granted: the broker needs a host to scope it to.
        let mut c = call(ToolName::Run, Some("uptime"));
        c.host = None;
        assert!(
            build_grant(
                &base(ApprovalSubject::Call {
                    call: c,
                    analysis: None,
                }),
                5
            )
            .is_none()
        );
    }

    #[test]
    fn grant_expires_in_the_future_and_is_refused_for_plans() {
        let r = base(ApprovalSubject::Call {
            call: call(ToolName::Run, Some("ls")),
            analysis: None,
        });
        let g = build_grant(&r, 30).unwrap();
        let exp = chrono::DateTime::parse_from_rfc3339(&g.expires_at).unwrap();
        let delta = exp.signed_duration_since(chrono::Utc::now());
        assert!(delta.num_minutes() >= 29 && delta.num_minutes() <= 30);

        assert!(build_grant(&demo_plan_request(), 30).is_none());
    }
}
