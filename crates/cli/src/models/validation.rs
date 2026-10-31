//! Field validation shared by every path that stores a connection (CLI
//! add/edit/import and the desktop GUI). Values end up in an ssh argv, so a
//! leading '-' or shell metacharacters must never get through.

const INVALID_HOST_CHARS: &[char] = &[
    ' ', '\t', '\n', '\r', '\'', '"', '`', '$', ';', '&', '|', '<', '>', '(', ')', '{', '}', '*',
    '?', '!', '#', '~', '\\', '/', ',', '=', '%', '@',
];
const INVALID_USER_CHARS: &[char] = &[
    ' ', '\t', '\n', '\r', '\'', '"', '`', '$', ';', '&', '|', '<', '>', '(', ')', '{', '}', '*',
    '?', '!', '#', '~', '\\', '/', '@',
];

/// Why a connection field failed validation. The [`std::fmt::Display`]
/// messages are shown to users as they are.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("{label} cannot be empty.")]
    Empty { label: &'static str },
    #[error("{label} is too long (max {max} characters).")]
    TooLong { label: &'static str, max: usize },
    #[error("{label} cannot start with '-'.")]
    StartsWithDash { label: &'static str },
    #[error("{label} has an invalid bracketed IPv6 literal.")]
    InvalidBracketedIpv6 { label: &'static str },
    #[error("{label} contains invalid characters (spaces, quotes, shell metacharacters, or '@' are not allowed).")]
    InvalidChars { label: &'static str },
    #[error("{label} contains invalid control characters.")]
    InvalidControl { label: &'static str },
    #[error("Key path is too long.")]
    KeyPathTooLong,
    #[error("Key path contains invalid characters.")]
    KeyPathInvalidChars,
}

fn validate_host_field(
    field: &str,
    label: &'static str,
    allow_ipv6_brackets: bool,
) -> Result<(), ValidationError> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::Empty { label });
    }
    if trimmed.len() > 253 {
        return Err(ValidationError::TooLong { label, max: 253 });
    }
    // A leading '-' would be parsed by ssh as an option (e.g. "-oProxyCommand=…").
    if trimmed.starts_with('-') {
        return Err(ValidationError::StartsWithDash { label });
    }
    let mut chars = trimmed.chars().peekable();
    // Allow a leading '[' for bracketed IPv6 literals, e.g. [::1] or [fe80::1%eth0].
    if allow_ipv6_brackets && chars.peek() == Some(&'[') {
        let inner = trimmed
            .strip_suffix(']')
            .map(|s| &s[1..])
            .filter(|s| !s.is_empty())
            .ok_or(ValidationError::InvalidBracketedIpv6 { label })?;
        if !inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '%' | '_' | '-'))
        {
            return Err(ValidationError::InvalidBracketedIpv6 { label });
        }
        return Ok(());
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || INVALID_HOST_CHARS.contains(&c))
    {
        return Err(ValidationError::InvalidChars { label });
    }
    Ok(())
}

fn validate_user_field(field: &str, label: &'static str) -> Result<(), ValidationError> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Err(ValidationError::Empty { label });
    }
    if trimmed.len() > 64 {
        return Err(ValidationError::TooLong { label, max: 64 });
    }
    if trimmed.starts_with('-') {
        return Err(ValidationError::StartsWithDash { label });
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || INVALID_USER_CHARS.contains(&c))
    {
        return Err(ValidationError::InvalidChars { label });
    }
    Ok(())
}

fn validate_key_path_field(field: Option<&str>) -> Result<(), ValidationError> {
    let Some(path) = field else {
        return Ok(());
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.len() > 4096 {
        return Err(ValidationError::KeyPathTooLong);
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || c == '\'' || c == '"' || c == '`' || c == '$')
    {
        return Err(ValidationError::KeyPathInvalidChars);
    }
    Ok(())
}

/// Validate a connection's fields before they are stored or reach an ssh argv.
///
/// # Errors
///
/// The first field that fails, as a [`ValidationError`].
pub fn validate_connection_fields(
    name: &str,
    host: &str,
    user: &str,
    bastion: Option<&str>,
    bastion_user: Option<&str>,
    key_path: Option<&str>,
) -> Result<(), ValidationError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ValidationError::Empty { label: "Name" });
    }
    if name.len() > 128 {
        return Err(ValidationError::TooLong {
            label: "Name",
            max: 128,
        });
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(ValidationError::InvalidControl { label: "Name" });
    }

    validate_host_field(host, "Host", true)?;
    validate_user_field(user, "User")?;

    if let Some(b) = bastion {
        validate_host_field(b, "Bastion host", true)?;
    }
    if let Some(bu) = bastion_user {
        validate_user_field(bu, "Bastion user")?;
    }
    validate_key_path_field(key_path)
}

impl super::Connection {
    /// Validate this connection's fields; see [`validate_connection_fields`].
    ///
    /// # Errors
    ///
    /// The first field that fails, as a [`ValidationError`].
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_connection_fields(
            &self.name,
            &self.host,
            &self.user,
            self.bastion.as_deref(),
            self.bastion_user.as_deref(),
            self.key_path.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_option_like_user_and_host() {
        assert!(validate_user_field("-oProxyCommand=reboot", "User").is_err());
        assert!(validate_host_field("-oProxyCommand", "Host", true).is_err());
        assert!(validate_user_field("deploy", "User").is_ok());
        assert!(validate_host_field("db-1.example.com", "Host", true).is_ok());
    }

    #[test]
    fn bracketed_ipv6_content_is_checked() {
        assert!(validate_host_field("[::1]", "Host", true).is_ok());
        assert!(validate_host_field("[fe80::1%eth0]", "Host", true).is_ok());
        assert!(validate_host_field("[]", "Host", true).is_err());
        assert!(validate_host_field("[::1;touch /tmp/x]", "Host", true).is_err());
        assert!(validate_host_field("[::1", "Host", true).is_err());
    }
}
