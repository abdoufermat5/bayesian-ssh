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

fn validate_host_field(field: &str, label: &str, allow_ipv6_brackets: bool) -> Result<(), String> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be empty."));
    }
    if trimmed.len() > 253 {
        return Err(format!("{label} is too long (max 253 characters)."));
    }
    // A leading '-' would be parsed by ssh as an option (e.g. "-oProxyCommand=…").
    if trimmed.starts_with('-') {
        return Err(format!("{label} cannot start with '-'."));
    }
    let mut chars = trimmed.chars().peekable();
    // Allow a leading '[' for bracketed IPv6 literals, e.g. [::1] or [fe80::1%eth0].
    if allow_ipv6_brackets && chars.peek() == Some(&'[') {
        let inner = trimmed
            .strip_suffix(']')
            .map(|s| &s[1..])
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("{label} has an invalid bracketed IPv6 literal."))?;
        if !inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '%' | '_' | '-'))
        {
            return Err(format!("{label} has an invalid bracketed IPv6 literal."));
        }
        return Ok(());
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || INVALID_HOST_CHARS.contains(&c))
    {
        return Err(format!(
            "{label} contains invalid characters (spaces, quotes, shell metacharacters, or '@' are not allowed)."
        ));
    }
    Ok(())
}

fn validate_user_field(field: &str, label: &str) -> Result<(), String> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} cannot be empty."));
    }
    if trimmed.len() > 64 {
        return Err(format!("{label} is too long (max 64 characters)."));
    }
    if trimmed.starts_with('-') {
        return Err(format!("{label} cannot start with '-'."));
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || INVALID_USER_CHARS.contains(&c))
    {
        return Err(format!(
            "{label} contains invalid characters (spaces, quotes, shell metacharacters, or '@' are not allowed)."
        ));
    }
    Ok(())
}

fn validate_key_path_field(field: &Option<String>) -> Result<(), String> {
    let Some(path) = field else {
        return Ok(());
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.len() > 4096 {
        return Err("Key path is too long.".to_string());
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || c == '\'' || c == '"' || c == '`' || c == '$')
    {
        return Err("Key path contains invalid characters.".to_string());
    }
    Ok(())
}

pub fn validate_connection_fields(
    name: &str,
    host: &str,
    user: &str,
    bastion: &Option<String>,
    bastion_user: &Option<String>,
    key_path: &Option<String>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name cannot be empty.".to_string());
    }
    if name.len() > 128 {
        return Err("Name is too long (max 128 characters).".to_string());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("Name contains invalid control characters.".to_string());
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
    pub fn validate(&self) -> Result<(), String> {
        validate_connection_fields(
            &self.name,
            &self.host,
            &self.user,
            &self.bastion,
            &self.bastion_user,
            &self.key_path,
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
