//! Reading the audit log page by page.

use super::error::{ClientResult, TeamsError};
use bssh_teams_proto::api::{AuditListQuery, AuditOrder, AuditRecord};

/// Page size of an audit query that sets none: the server's default.
const DEFAULT_AUDIT_PAGE: u32 = 100;

/// The query string of an audit request. Serialized by the protocol type
/// itself, so the field and enum names can't drift from the server's.
pub(super) fn audit_query_pairs(query: &AuditListQuery) -> ClientResult<Vec<(String, String)>> {
    let serde_json::Value::Object(fields) =
        serde_json::to_value(query).map_err(|e| TeamsError::Decode(e.to_string()))?
    else {
        return Err(TeamsError::Decode("audit query is not an object".into()));
    };
    Ok(fields
        .into_iter()
        .map(|(k, v)| match v {
            serde_json::Value::String(s) => (k, s),
            other => (k, other.to_string()),
        })
        .collect())
}

/// Whether `last` lies strictly past the cursor of `query`, in the order the
/// query asks for. A query with no cursor (the first page) is always passed.
/// A server older than paging ignores the cursor and answers the first page
/// again, which fails this test.
fn passes_cursor(query: &AuditListQuery, last: &AuditRecord) -> bool {
    let event = &last.event;
    match query.order.unwrap_or_default() {
        AuditOrder::Newest => match (
            query.before_received_at,
            query.before_device_id,
            query.before_seq,
        ) {
            (Some(at), Some(device), Some(seq)) => {
                (last.received_at, event.device_id, event.seq) < (at, device, seq)
            }
            _ => true,
        },
        AuditOrder::Chain => match (query.after_device_id, query.after_seq) {
            (Some(device), Some(seq)) => (event.device_id, event.seq) > (device, seq),
            _ => true,
        },
    }
}

/// How [`walk_audit_pages`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditWalk {
    /// A page shorter than the page size came back: the log is read to its end.
    Finished,
    /// A page did not advance past its cursor: the server doesn't page (it
    /// predates paging) and the log was not read to its end.
    Stalled,
    /// `each` asked to stop.
    Stopped,
}

/// Read the audit log page by page, starting from `first`, and hand every
/// page to `each` as it arrives, so a caller that only checks events in
/// passing never holds the whole log. Each next page is
/// [`AuditListQuery::after`] the last record of the previous one. Stops on a
/// page shorter than the page size, on an empty page, when a page does not
/// advance past its cursor, or when `each` returns `false`.
pub async fn walk_audit_pages<F, Fut, E>(
    first: AuditListQuery,
    mut fetch: F,
    mut each: E,
) -> ClientResult<AuditWalk>
where
    F: FnMut(AuditListQuery) -> Fut,
    Fut: std::future::Future<Output = ClientResult<Vec<AuditRecord>>>,
    E: FnMut(Vec<AuditRecord>) -> bool,
{
    let page_size = first.limit.unwrap_or(DEFAULT_AUDIT_PAGE).max(1) as usize;
    let mut query = first;
    loop {
        let page = fetch(query.clone()).await?;
        let Some(last) = page.last().cloned() else {
            return Ok(AuditWalk::Finished);
        };
        if !passes_cursor(&query, &last) {
            return Ok(AuditWalk::Stalled);
        }
        let full = page.len() >= page_size;
        if !each(page) {
            return Ok(AuditWalk::Stopped);
        }
        if !full {
            return Ok(AuditWalk::Finished);
        }
        query = query.after(&last);
    }
}
