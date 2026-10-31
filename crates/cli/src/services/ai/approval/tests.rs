use super::broker::{short_subject, truncate, MAX_GRANT_HOURS};
use super::*;
use crate::services::ai::store::AiStore;
use crate::services::ai::types::{
    ApprovalBroker, ApprovalOutcome, ApprovalRequest, ApprovalSubject, Grant, Tier,
};
use crate::services::ai::types::{Scope, ToolCall, ToolName};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

pub(crate) fn run_info() -> RunInfo {
    RunInfo {
        on_behalf_of: "ana".into(),
        hosts: vec!["web-1".into()],
        run_id: "run1".into(),
        client: "test-client".into(),
        policy: "default".into(),
        started_at: chrono::Utc::now().to_rfc3339(),
    }
}

pub(crate) fn request(id: &str, tier: Tier) -> ApprovalRequest {
    ApprovalRequest {
        request_id: id.into(),
        run_id: "run1".into(),
        client: "test-client".into(),
        tier,
        subject: ApprovalSubject::Access {
            scope: Scope::default(),
            reason: "need logs".into(),
            duration: "30m".into(),
        },
        created_at: chrono::Utc::now().to_rfc3339(),
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
        reason: String::new(),
    }
}

pub(crate) fn broker(
    timeout: u64,
    el: Option<Arc<dyn Elicitor>>,
) -> (tempfile::TempDir, Arc<LocalBroker>) {
    let tmp = tempfile::tempdir().unwrap();
    let store = Arc::new(AiStore::open_in(tmp.path()).unwrap());
    let b = LocalBroker::new(store, run_info(), el, timeout).without_notifications();
    (tmp, Arc::new(b))
}

fn approve(by: &str) -> ApprovalOutcome {
    ApprovalOutcome::Approved {
        by: by.into(),
        channel: "cli".into(),
        grant: None,
    }
}

/// A single-call request: the only subject a grant may cover.
fn call_request(id: &str, tool: ToolName, host: Option<&str>) -> ApprovalRequest {
    let mut r = request(id, Tier::OutOfBand);
    r.subject = ApprovalSubject::Call {
        call: ToolCall {
            run_id: "run1".into(),
            call_id: "c1".into(),
            tool,
            host: host.map(String::from),
            command: Some("uptime".into()),
            path: None,
            args: serde_json::Value::Null,
        },
        analysis: None,
    };
    r
}

/// A grant that `decide` accepts as-is for a `Service` call on `web-1`.
fn a_grant() -> Grant {
    Grant {
        grant_id: "g1".into(),
        run_id: Some("run1".into()),
        tool: ToolName::Service,
        hosts: "web-1".into(),
        target: None,
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
    }
}

/// Wait until the broker has `n` pending requests.
pub(crate) async fn wait_pending(b: &LocalBroker, n: usize) {
    for _ in 0..200 {
        if b.pending_requests().len() >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("request never became pending");
}

struct FixedElicitor(Option<bool>);
#[async_trait]
impl Elicitor for FixedElicitor {
    async fn confirm(&self, _: &ApprovalRequest) -> anyhow::Result<Option<bool>> {
        Ok(self.0)
    }
}
struct FailingElicitor;
#[async_trait]
impl Elicitor for FailingElicitor {
    async fn confirm(&self, _: &ApprovalRequest) -> anyhow::Result<Option<bool>> {
        anyhow::bail!("client went away")
    }
}

#[tokio::test]
async fn timeout_expires_the_request() {
    let (_t, b) = broker(0, None);
    let out = b.request(request("q1", Tier::OutOfBand)).await.unwrap();
    assert_eq!(out, ApprovalOutcome::Expired);
    assert_eq!(
        b.store().request_status("q1").unwrap().as_deref(),
        Some("expired")
    );
    assert!(b.pending_requests().is_empty());
    // a late decision is refused
    assert!(b.decide("q1", approve("alice")).is_err());
}

#[tokio::test]
async fn out_of_band_approval() {
    let (_t, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
    wait_pending(&b, 1).await;
    assert_eq!(
        b.store().request_status("q1").unwrap().as_deref(),
        Some("pending")
    );
    b.decide("q1", approve("alice")).unwrap();
    let out = h.await.unwrap().unwrap();
    assert_eq!(out, approve("alice"));
    assert_eq!(
        b.store().request_status("q1").unwrap().as_deref(),
        Some("approved")
    );
    assert!(b.pending_requests().is_empty());
}

#[tokio::test]
async fn out_of_band_denial_keeps_reason() {
    let (_t, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
    wait_pending(&b, 1).await;
    let denied = ApprovalOutcome::Denied {
        by: "alice".into(),
        reason: Some("not now".into()),
    };
    b.decide("q1", denied.clone()).unwrap();
    assert_eq!(h.await.unwrap().unwrap(), denied);
    assert_eq!(b.store().outcome("q1").unwrap(), Some(denied));
}

#[tokio::test]
async fn approval_with_grant_stores_the_grant() {
    let (_t, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move {
        b2.request(call_request("q1", ToolName::Service, Some("web-1")))
            .await
    });
    wait_pending(&b, 1).await;
    let grant = Grant {
        grant_id: String::new(),
        run_id: Some("run1".into()),
        tool: ToolName::Service,
        hosts: "web-*".into(),
        target: None,
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339(),
    };
    b.decide(
        "q1",
        ApprovalOutcome::Approved {
            by: "alice".into(),
            channel: "cli".into(),
            grant: Some(grant),
        },
    )
    .unwrap();
    match h.await.unwrap().unwrap() {
        ApprovalOutcome::Approved { grant: Some(g), .. } => assert!(!g.grant_id.is_empty()),
        other => panic!("{other:?}"),
    }
    let stored = b.store().active_grants(Some("run1")).unwrap();
    assert_eq!(stored.len(), 1);
    // The client asked for a glob; the broker keeps the request's exact host.
    assert_eq!(stored[0].hosts, "web-1");
    assert_eq!(stored[0].run_id.as_deref(), Some("run1"));
}

#[tokio::test]
async fn a_grant_is_narrowed_to_the_request() {
    let (_t, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move {
        b2.request(call_request("q1", ToolName::Service, Some("web-1")))
            .await
    });
    wait_pending(&b, 1).await;
    let wide = Grant {
        grant_id: "client-chosen".into(),
        run_id: Some("other-run".into()),
        tool: ToolName::Service,
        hosts: "*".into(),
        target: None,
        expires_at: (chrono::Utc::now() + chrono::Duration::days(365)).to_rfc3339(),
    };
    b.decide(
        "q1",
        ApprovalOutcome::Approved {
            by: "alice".into(),
            channel: "cli".into(),
            grant: Some(wide),
        },
    )
    .unwrap();
    let out = h.await.unwrap().unwrap();
    let ApprovalOutcome::Approved { grant: Some(g), .. } = out else {
        panic!("{out:?}");
    };
    assert_eq!(g.hosts, "web-1");
    assert_eq!(g.run_id.as_deref(), Some("run1"));
    assert_ne!(g.grant_id, "client-chosen");
    let expires = chrono::DateTime::parse_from_rfc3339(&g.expires_at).unwrap();
    let cap = chrono::Utc::now() + chrono::Duration::hours(MAX_GRANT_HOURS);
    assert!(expires <= cap, "{expires} > {cap}");
}

#[tokio::test]
async fn grants_that_make_no_sense_are_refused() {
    let (_t, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move {
        b2.request(call_request("q1", ToolName::Service, Some("web-1")))
            .await
    });
    wait_pending(&b, 1).await;
    let past = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    for g in [
        Grant {
            tool: ToolName::Run,
            ..a_grant()
        },
        Grant {
            expires_at: "soon".into(),
            ..a_grant()
        },
        Grant {
            expires_at: past,
            ..a_grant()
        },
    ] {
        let err = b
            .decide(
                "q1",
                ApprovalOutcome::Approved {
                    by: "alice".into(),
                    channel: "cli".into(),
                    grant: Some(g),
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains("grant"), "{err}");
        assert_eq!(b.pending_requests().len(), 1, "still pending");
    }
    // Without a grant the same approval goes through.
    b.decide("q1", approve("alice")).unwrap();
    h.await.unwrap().unwrap();
}

#[tokio::test]
async fn grants_only_cover_a_single_call_with_a_host() {
    // An access request (not a call) can't carry a grant.
    let (_t, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
    wait_pending(&b, 1).await;
    assert!(b
        .decide(
            "q1",
            ApprovalOutcome::Approved {
                by: "alice".into(),
                channel: "cli".into(),
                grant: Some(a_grant()),
            },
        )
        .is_err());
    assert_eq!(b.pending_requests().len(), 1);
    b.decide("q1", approve("alice")).unwrap();
    h.await.unwrap().unwrap();

    // A call without a host has nothing to scope a grant to.
    let (_t2, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(call_request("q1", ToolName::Run, None)).await });
    wait_pending(&b, 1).await;
    let mut g = a_grant();
    g.tool = ToolName::Run;
    assert!(b
        .decide(
            "q1",
            ApprovalOutcome::Approved {
                by: "alice".into(),
                channel: "cli".into(),
                grant: Some(g),
            },
        )
        .is_err());
    b.decide("q1", approve("alice")).unwrap();
    h.await.unwrap().unwrap();
}

#[tokio::test]
async fn decide_unknown_or_expired_outcome_errors() {
    let (_t, b) = broker(30, None);
    assert!(b.decide("nope", approve("a")).is_err());
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
    wait_pending(&b, 1).await;
    assert!(b.decide("q1", ApprovalOutcome::Expired).is_err());
    b.decide("q1", approve("a")).unwrap();
    h.await.unwrap().unwrap();
}

#[tokio::test]
async fn two_person_needs_two_distinct_approvers() {
    // Locally every approval comes from the same machine, so a second local approval is
    // refused even under another name; the request ends denied needing the Teams relay.
    let (_t, b) = broker(1, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(request("q1", Tier::TwoPerson)).await });
    wait_pending(&b, 1).await;
    b.decide("q1", approve("alice")).unwrap();
    assert_eq!(
        b.pending_requests().len(),
        1,
        "still waiting for a second person"
    );
    assert!(
        b.decide("q1", approve("alice")).is_err(),
        "same person twice"
    );
    assert!(
        b.decide("q1", approve("bob")).is_err(),
        "a local approval can't be the second person"
    );
    match h.await.unwrap().unwrap() {
        ApprovalOutcome::Denied {
            reason: Some(r), ..
        } => {
            assert!(r.contains("needs Teams (two approvers)"), "{r}");
            assert!(r.contains("1 of 2"), "{r}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn two_person_denial_by_one_is_final() {
    let (_t, b) = broker(30, None);
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(request("q1", Tier::TwoPerson)).await });
    wait_pending(&b, 1).await;
    b.decide("q1", approve("alice")).unwrap();
    b.decide(
        "q1",
        ApprovalOutcome::Denied {
            by: "bob".into(),
            reason: None,
        },
    )
    .unwrap();
    assert!(matches!(
        h.await.unwrap().unwrap(),
        ApprovalOutcome::Denied { .. }
    ));
}

#[tokio::test]
async fn two_person_alone_is_denied_needs_teams() {
    let (_t, b) = broker(0, None);
    let out = b.request(request("q1", Tier::TwoPerson)).await.unwrap();
    match out {
        ApprovalOutcome::Denied {
            reason: Some(r), ..
        } => {
            assert!(r.contains("needs Teams (two approvers)"), "{r}")
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn in_client_uses_the_elicitor() {
    let (_t, b) = broker(30, Some(Arc::new(FixedElicitor(Some(true)))));
    let out = b.request(request("q1", Tier::InClient)).await.unwrap();
    match out {
        ApprovalOutcome::Approved { channel, .. } => {
            assert_eq!(channel, "confirmed-in-client")
        }
        other => panic!("{other:?}"),
    }
    let (_t, b) = broker(30, Some(Arc::new(FixedElicitor(Some(false)))));
    let out = b.request(request("q2", Tier::InClient)).await.unwrap();
    assert!(matches!(out, ApprovalOutcome::Denied { .. }));
}

#[tokio::test]
async fn in_client_falls_back_to_out_of_band() {
    for el in [
        None,
        Some(Arc::new(FixedElicitor(None)) as Arc<dyn Elicitor>),
        Some(Arc::new(FailingElicitor) as Arc<dyn Elicitor>),
    ] {
        let (_t, b) = broker(0, el);
        let out = b.request(request("q1", Tier::InClient)).await.unwrap();
        // Nobody answers the out-of-band request within 0s.
        assert_eq!(out, ApprovalOutcome::Expired);
        assert!(
            b.store().request_status("q1").unwrap().is_some(),
            "was persisted"
        );
    }
}

#[tokio::test]
async fn new_requests_are_broadcast() {
    let (_t, b) = broker(30, None);
    let mut rx = b.subscribe_requests();
    let b2 = b.clone();
    let h = tokio::spawn(async move { b2.request(request("q1", Tier::OutOfBand)).await });
    let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.request_id, "q1");
    b.decide("q1", approve("a")).unwrap();
    h.await.unwrap().unwrap();
}

#[test]
fn short_subject_is_one_line() {
    let r = request("q", Tier::OutOfBand);
    assert!(short_subject(&r).contains("need logs"));
    assert_eq!(truncate("abcdef", 3), "abc...");
    assert_eq!(truncate("ab", 3), "ab");
}
