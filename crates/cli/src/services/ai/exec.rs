//! Executor for AI calls: timeouts, output caps, non-interactive auth, jailed and certified modes.
//!
//! Every call goes through `SshTransport::exec_limited` with
//! `non_interactive = true`, so a missing key or agent fails with a clear error instead of
//! prompting on a TTY an MCP server does not have.

use super::cert;
use super::jail;
use super::types::{ExecLimits, ExecMode, ExecResult, Executor};
use crate::config::AppConfig;
use crate::models::Connection;
use crate::services::transport::{
    execute_with_fallback, CertAuth, ExecOptions, LimitedOutput, TransportError,
};
use async_trait::async_trait;
use std::time::{Duration, Instant};

/// Longest wait for the TCP connect and the authentication of one call.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Lifetime of the per-command certificate (design 7.3: single use, 2 minutes).
const CERT_TTL: Duration = Duration::from_secs(120);

/// The executor every AI tool uses. Non-interactive: never prompts on a TTY.
pub struct AiExecutor {
    config: AppConfig,
    /// Paths the read-only jail may read (`Policy::read_paths`).
    read_paths: Vec<String>,
}

impl AiExecutor {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            config: config.clone(),
            read_paths: Vec::new(),
        }
    }

    /// Paths a jailed command may read; passed to `jail::wrap`.
    pub fn with_read_paths(mut self, read_paths: Vec<String>) -> Self {
        self.read_paths = read_paths;
        self
    }

    /// Turn a call into the connection, command and options the transport needs.
    fn prepare(
        &self,
        conn: &Connection,
        command: &str,
        mode: &ExecMode,
        limits: &ExecLimits,
    ) -> anyhow::Result<(Connection, String, ExecOptions)> {
        let mut opts = ExecOptions {
            timeout: Some(limits.timeout),
            max_output_bytes: Some(limits.max_output_bytes),
            connect_timeout: Some(CONNECT_TIMEOUT.min(limits.timeout.max(Duration::from_secs(1)))),
            non_interactive: true,
            cert_auth: None,
        };
        match mode {
            ExecMode::Plain => Ok((conn.clone(), command.to_string(), opts)),
            ExecMode::Jailed(kind) => {
                let wrapped = jail::wrap(*kind, &self.read_paths, command)?;
                Ok((conn.clone(), wrapped, opts))
            }
            ExecMode::Certified => {
                if !cert::ca_available(&self.config) {
                    anyhow::bail!(
                        "certified exec is not supported yet: no local CA \
                         (initialise one with `bssh ai ca init`)"
                    );
                }
                let issued = cert::issue(&self.config, command, CERT_TTL, None)
                    .map_err(|e| anyhow::anyhow!("certified exec is not supported yet: {e}"))?;
                opts.cert_auth = Some(CertAuth {
                    private_key_openssh: issued.private_key_openssh,
                    certificate_openssh: issued.certificate_openssh,
                });
                // Only the certificate may authenticate, as the agent principal.
                let mut agent_conn = conn.clone();
                agent_conn.user = cert::AGENT_PRINCIPAL.to_string();
                agent_conn.key_path = None;
                Ok((agent_conn, command.to_string(), opts))
            }
        }
    }
}

/// Decode as UTF-8, replacing invalid sequences. When `truncated`, a multi-byte character
/// cut in half by the cap is dropped instead of becoming U+FFFD.
fn decode(bytes: &[u8], truncated: bool) -> String {
    let bytes = if truncated {
        match std::str::from_utf8(bytes) {
            Err(e) if e.error_len().is_none() => &bytes[..e.valid_up_to()],
            _ => bytes,
        }
    } else {
        bytes
    };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Map the transport's result to the contract type. `exit_code` is `None` only for a
/// timeout; a command that ended without any exit status is reported as `-1`.
pub(crate) fn to_exec_result(out: LimitedOutput, elapsed: Duration) -> ExecResult {
    let LimitedOutput {
        stdout,
        stderr,
        exit_code,
        truncated,
        timed_out,
    } = out;
    ExecResult {
        stdout: decode(&stdout, truncated),
        stderr: decode(&stderr, truncated),
        exit_code: if timed_out {
            None
        } else {
            Some(exit_code.unwrap_or(-1))
        },
        truncated,
        timed_out,
        duration_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
    }
}

#[async_trait]
impl Executor for AiExecutor {
    async fn exec(
        &self,
        conn: &Connection,
        command: &str,
        mode: ExecMode,
        limits: ExecLimits,
    ) -> anyhow::Result<ExecResult> {
        let (conn, command, opts) = self.prepare(conn, command, &mode, &limits)?;
        let started = Instant::now();
        let out = execute_with_fallback(&conn, &self.config, |transport| {
            let conn = conn.clone();
            let command = command.clone();
            let opts = opts.clone();
            Box::pin(async move { transport.exec_limited(&conn, &command, &opts).await })
        })
        .await
        .map_err(|e| match e {
            TransportError::Fallback(e) | TransportError::Permanent(e) => e,
        })?;
        Ok(to_exec_result(out, started.elapsed()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ai::types::JailKind;

    fn conn() -> Connection {
        Connection::new(
            "web-1".into(),
            "web-1.example".into(),
            "alice".into(),
            22,
            None,
            None,
            false,
            Some("/home/alice/.ssh/id_ed25519".into()),
        )
    }

    fn limits() -> ExecLimits {
        ExecLimits {
            timeout: Duration::from_secs(5),
            max_output_bytes: 1024,
        }
    }

    #[test]
    fn plain_mode_keeps_command_and_sets_limits() {
        let exec = AiExecutor::new(&AppConfig::default());
        let (c, cmd, opts) = exec
            .prepare(&conn(), "uptime", &ExecMode::Plain, &limits())
            .unwrap();
        assert_eq!(c.user, "alice");
        assert_eq!(cmd, "uptime");
        assert_eq!(opts.timeout, Some(Duration::from_secs(5)));
        assert_eq!(opts.max_output_bytes, Some(1024));
        assert!(opts.non_interactive, "AI exec must never prompt");
        assert!(opts.cert_auth.is_none());
        // The connect phase is bounded, and never longer than the command's own limit.
        assert_eq!(opts.connect_timeout, Some(Duration::from_secs(5)));
    }

    #[test]
    fn connect_timeout_is_capped() {
        let exec = AiExecutor::new(&AppConfig::default());
        let l = ExecLimits {
            timeout: Duration::from_secs(600),
            ..limits()
        };
        let (_, _, opts) = exec.prepare(&conn(), "x", &ExecMode::Plain, &l).unwrap();
        assert_eq!(opts.connect_timeout, Some(CONNECT_TIMEOUT));
    }

    #[test]
    fn jailed_mode_goes_through_jail_wrap() {
        // A jail that can't be built must fail the call, never fall back to plain.
        let exec = AiExecutor::new(&AppConfig::default()).with_read_paths(vec!["/var/log".into()]);
        let res = exec.prepare(&conn(), "ls", &ExecMode::Jailed(JailKind::None), &limits());
        assert!(res.is_err(), "JailKind::None cannot run jailed");
    }

    #[test]
    fn certified_mode_errors_clearly_without_a_ca() {
        let exec = AiExecutor::new(&AppConfig::default());
        let err = exec
            .prepare(
                &conn(),
                "systemctl reload nginx",
                &ExecMode::Certified,
                &limits(),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("not supported yet"), "{err}");
    }

    #[test]
    fn result_mapping_plain_exit() {
        let r = to_exec_result(
            LimitedOutput {
                stdout: b"ok\n".to_vec(),
                stderr: b"warn\n".to_vec(),
                exit_code: Some(3),
                truncated: false,
                timed_out: false,
            },
            Duration::from_millis(42),
        );
        assert_eq!(r.stdout, "ok\n");
        assert_eq!(r.stderr, "warn\n");
        assert_eq!(r.exit_code, Some(3));
        assert!(!r.truncated && !r.timed_out);
        assert_eq!(r.duration_ms, 42);
    }

    #[test]
    fn result_mapping_timeout_has_no_exit_code() {
        let r = to_exec_result(
            LimitedOutput {
                stdout: b"partial".to_vec(),
                exit_code: None,
                timed_out: true,
                ..LimitedOutput::default()
            },
            Duration::from_secs(5),
        );
        assert_eq!(r.exit_code, None);
        assert!(r.timed_out);
        assert_eq!(r.stdout, "partial");
    }

    #[test]
    fn result_mapping_missing_status_is_minus_one() {
        let r = to_exec_result(LimitedOutput::default(), Duration::ZERO);
        assert_eq!(r.exit_code, Some(-1));
    }

    #[test]
    fn invalid_utf8_is_replaced() {
        let r = to_exec_result(
            LimitedOutput {
                stdout: vec![b'a', 0xff, b'b'],
                exit_code: Some(0),
                ..LimitedOutput::default()
            },
            Duration::ZERO,
        );
        assert_eq!(r.stdout, "a\u{fffd}b");
    }

    #[test]
    fn truncation_does_not_leave_half_a_character() {
        // "é" is 0xC3 0xA9; the cap cut after the first byte.
        let r = to_exec_result(
            LimitedOutput {
                stdout: vec![b'a', 0xC3],
                exit_code: Some(0),
                truncated: true,
                ..LimitedOutput::default()
            },
            Duration::ZERO,
        );
        assert_eq!(r.stdout, "a");
        assert!(r.truncated);
    }
}
