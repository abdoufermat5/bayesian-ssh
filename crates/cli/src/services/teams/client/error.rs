//! Errors of the Teams HTTP client.

use bssh_teams_proto::api::{ApiError, ErrorCode};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TeamsError {
    #[error("refusing insecure server URL {0}: use https (http is only allowed for localhost)")]
    InsecureUrl(String),
    #[error("invalid server URL: {0}")]
    InvalidUrl(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("{}", api_text(*.status, .error))]
    Api { status: u16, error: ApiError },
    #[error("unexpected response {status}: {body}")]
    Http { status: u16, body: String },
    #[error("could not decode server response: {0}")]
    Decode(String),
    #[error("this server doesn't support organizations: it may need an update")]
    OrgUnsupported,
}

/// How an API error reads to the user. For billing and provider failures the
/// server's message already says what to do (seats, plan, "try again in a
/// minute"), so it is shown as is, without a status and code prefix.
fn api_text(status: u16, error: &ApiError) -> String {
    match error.code {
        ErrorCode::PaymentRequired | ErrorCode::SeatLimit | ErrorCode::Internal => {
            error.message.clone()
        }
        code => format!("server error {status}: {code:?}: {}", error.message),
    }
}

impl TeamsError {
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            TeamsError::Api { error, .. } => Some(error.code),
            _ => None,
        }
    }

    pub fn is_rotation_required(&self) -> bool {
        self.code() == Some(ErrorCode::RotationRequired)
    }

    /// The server wants proof that the account owns its email: request a code
    /// with [`TeamsClient::request_email_code`](super::TeamsClient::request_email_code) and retry with it in
    /// [`CreateAccountRequest::email_code`](bssh_teams_proto::api::CreateAccountRequest::email_code).
    pub fn is_email_verification_required(&self) -> bool {
        self.code() == Some(ErrorCode::EmailVerificationRequired)
    }

    /// A mailed code was wrong, expired, already used or tried too often.
    pub fn is_invalid_email_code(&self) -> bool {
        self.code() == Some(ErrorCode::InvalidEmailCode)
    }

    /// The team's audit storage is full. Not transient: retrying changes
    /// nothing until the server's operator raises the team's audit storage limit.
    pub fn is_audit_quota_exceeded(&self) -> bool {
        self.code() == Some(ErrorCode::AuditQuotaExceeded)
    }

    /// True for failures worth retrying later (network, 5xx, rate limit).
    pub fn is_transient(&self) -> bool {
        match self {
            TeamsError::Network(_) => true,
            TeamsError::Http { status, .. } => *status >= 500,
            TeamsError::Api { error, .. } => {
                matches!(error.code, ErrorCode::RateLimited | ErrorCode::Internal)
            }
            _ => false,
        }
    }
}

pub type ClientResult<T> = std::result::Result<T, TeamsError>;

/// Servers older than organizations answer `/orgs` with 404 (no route) or 405:
/// say so instead of showing the response body. The server's 404 may or may
/// not carry a JSON error, so both shapes map.
pub(super) fn org_route(err: TeamsError) -> TeamsError {
    let missing = match &err {
        TeamsError::Http { status, .. } | TeamsError::Api { status, .. } => {
            matches!(*status, 404 | 405)
        }
        _ => false,
    };
    if missing {
        TeamsError::OrgUnsupported
    } else {
        err
    }
}
