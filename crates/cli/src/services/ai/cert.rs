//! Local user CA and approval-bound short-lived certificates (force-command).
//!
//! Owner: W5. Design: ai-agent-access.md section 7.3, ssh-certificates.md section 3.
//!
//! The CA is one Ed25519 key under `<ai_dir>/ca/`. Hosts trust its public half (`bssh ai ca
//! trust`). For each run or approved step bssh mints a fresh Ed25519 key and a certificate
//! for the `bssh-agent` principal whose `force-command` is the exact command, with no
//! extensions: no PTY, no forwarding, no agent. Both live in memory only.
//!
//! The CA private key is sealed with `services/crypto.rs` (AES-256-GCM, Argon2id) when
//! `BSSH_AI_CA_PASSPHRASE` is set at `ca init`. The MCP server and the gateway have no
//! terminal, so a prompted passphrase can't work for them; the environment variable is the
//! only non-interactive input of the local scheme. Without it the key is stored as an
//! OpenSSH key file with mode 0600 in a 0700 directory, and `issue` refuses to read it if
//! its mode ever becomes wider.

use crate::config::AppConfig;
use crate::services::crypto;
use anyhow::{anyhow, bail, Context, Result};
use rand::RngCore;
use russh::keys::ssh_key::certificate::{Builder, CertType};
use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::ssh_key::{Certificate, HashAlg, LineEnding, PrivateKey, PublicKey};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroizing;

pub const AGENT_PRINCIPAL: &str = "bssh-agent";

/// Environment variable holding the passphrase that seals the CA key.
pub const CA_PASSPHRASE_ENV: &str = "BSSH_AI_CA_PASSPHRASE";

/// Longest certificate lifetime `issue` accepts (design 7.3: reads for a run, at most 1 h).
pub const MAX_CERT_TTL: Duration = Duration::from_secs(3600);

/// Certificates start this long before now, so host clock skew doesn't reject them.
const BACKDATE: Duration = Duration::from_secs(5 * 60);

const CA_KEY_FILE: &str = "ca_ed25519";
const CA_SEALED_FILE: &str = "ca_ed25519.sealed";
const CA_PUB_FILE: &str = "ca_ed25519.pub";

/// An ephemeral key plus a certificate signed by the local CA.
pub struct IssuedCert {
    pub private_key_openssh: Zeroizing<String>,
    pub certificate_openssh: String,
    pub key_id: String,
}

impl std::fmt::Debug for IssuedCert {
    // The private key must never reach a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IssuedCert")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

/// `<ai_dir>/ca/`.
pub fn ca_dir(config: &AppConfig) -> PathBuf {
    super::ai_dir(config).join("ca")
}

fn sealed_path(config: &AppConfig) -> PathBuf {
    ca_dir(config).join(CA_SEALED_FILE)
}

fn plain_path(config: &AppConfig) -> PathBuf {
    ca_dir(config).join(CA_KEY_FILE)
}

fn pub_path(config: &AppConfig) -> PathBuf {
    ca_dir(config).join(CA_PUB_FILE)
}

/// Is a local CA initialised (`bssh ai ca init`)?
pub fn ca_available(config: &AppConfig) -> bool {
    pub_path(config).is_file() && (plain_path(config).is_file() || sealed_path(config).is_file())
}

/// Is the CA key sealed with a passphrase?
pub fn ca_sealed(config: &AppConfig) -> bool {
    sealed_path(config).is_file()
}

/// Create the local CA. Returns its public key as an OpenSSH line.
///
/// `passphrase` seals the private key; `None` stores it 0600 (see the module docs).
pub fn init_ca(config: &AppConfig, passphrase: Option<&str>) -> Result<String> {
    if ca_available(config) {
        bail!(
            "a CA already exists in {}; remove that directory first if you want a new one \
             (hosts that trust the old CA will need `bssh ai ca trust` again)",
            ca_dir(config).display()
        );
    }
    let dir = ca_dir(config);
    create_private_dir(&dir)?;

    let mut key = ephemeral_ed25519()?;
    key.set_comment(format!(
        "bssh-ai-ca@{}",
        whoami::fallible::hostname().unwrap_or_default()
    ));
    let pem = key
        .to_openssh(LineEnding::LF)
        .map_err(|e| anyhow!("encode CA key: {e}"))?;

    match passphrase.filter(|p| !p.is_empty()) {
        Some(pass) => {
            let sealed = crypto::encrypt_data(pem.as_bytes(), pass)?;
            write_private_file(&sealed_path(config), &sealed)?;
        }
        None => write_private_file(&plain_path(config), pem.as_bytes())?,
    }
    let pub_line = key
        .public_key()
        .to_openssh()
        .map_err(|e| anyhow!("encode CA public key: {e}"))?;
    std::fs::write(pub_path(config), format!("{pub_line}\n"))
        .with_context(|| format!("write {}", pub_path(config).display()))?;
    Ok(pub_line)
}

/// The CA public key as one OpenSSH line (no trailing newline).
pub fn ca_public_key(config: &AppConfig) -> Result<String> {
    if !ca_available(config) {
        bail!("no local CA yet: run `bssh ai ca init`");
    }
    let line = std::fs::read_to_string(pub_path(config))
        .with_context(|| format!("read {}", pub_path(config).display()))?;
    let line = line.trim().to_string();
    PublicKey::from_openssh(&line).map_err(|e| anyhow!("CA public key is corrupt: {e}"))?;
    Ok(line)
}

/// `SHA256:...` fingerprint of the CA public key.
pub fn ca_fingerprint(config: &AppConfig) -> Result<String> {
    let key = PublicKey::from_openssh(&ca_public_key(config)?)
        .map_err(|e| anyhow!("CA public key is corrupt: {e}"))?;
    Ok(key.fingerprint(HashAlg::Sha256).to_string())
}

/// The base64 body of an OpenSSH public key line (what `authorized_keys`-style greps match).
pub fn key_body(openssh_line: &str) -> Option<&str> {
    openssh_line.split_whitespace().nth(1)
}

fn load_ca(config: &AppConfig) -> Result<PrivateKey> {
    let sealed = sealed_path(config);
    let key = if sealed.is_file() {
        let pass = std::env::var(CA_PASSPHRASE_ENV)
            .ok()
            .filter(|p| !p.is_empty())
            .map(Zeroizing::new)
            .ok_or_else(|| {
                anyhow!("the CA key is sealed: set {CA_PASSPHRASE_ENV} so bssh can open it")
            })?;
        let blob = std::fs::read(&sealed).with_context(|| format!("read {}", sealed.display()))?;
        let pem = Zeroizing::new(
            crypto::decrypt_data(&blob, &pass)
                .context("could not open the CA key (wrong passphrase?)")?,
        );
        PrivateKey::from_openssh(&*pem).map_err(|e| anyhow!("CA key is corrupt: {e}"))?
    } else {
        let path = plain_path(config);
        if !path.is_file() {
            bail!("no local CA yet: run `bssh ai ca init`");
        }
        check_private_mode(&path)?;
        let pem = Zeroizing::new(
            std::fs::read(&path).with_context(|| format!("read {}", path.display()))?,
        );
        PrivateKey::from_openssh(&*pem).map_err(|e| anyhow!("CA key is corrupt: {e}"))?
    };
    if key.is_encrypted() {
        bail!("the CA key file is passphrase-encrypted; bssh expects its own sealed format");
    }
    Ok(key)
}

/// Issue a certificate for `bssh-agent` with `force-command`, no PTY/forwarding.
///
/// `ttl` is capped at [`MAX_CERT_TTL`]. `source_address` is a comma-separated list of
/// IPs or CIDRs the server accepts the certificate from. What comes out is verified
/// against the CA before it is returned.
pub fn issue(
    config: &AppConfig,
    force_command: &str,
    ttl: Duration,
    source_address: Option<&str>,
) -> Result<IssuedCert> {
    let ca = load_ca(config)?;
    issue_with_ca(&ca, force_command, ttl, source_address, SystemTime::now())
}

/// `issue` with an explicit CA key and clock (tests, and the signer for Teams later).
pub fn issue_with_ca(
    ca: &PrivateKey,
    force_command: &str,
    ttl: Duration,
    source_address: Option<&str>,
    now: SystemTime,
) -> Result<IssuedCert> {
    if force_command.is_empty() || force_command.contains('\0') {
        bail!("force-command must be a non-empty string without NUL");
    }
    if ttl.is_zero() || ttl > MAX_CERT_TTL {
        bail!(
            "certificate lifetime must be between 1s and {}s",
            MAX_CERT_TTL.as_secs()
        );
    }
    if let Some(sa) = source_address {
        validate_source_address(sa)?;
    }

    let now_secs = unix_secs(now)?;
    let valid_after = now_secs.saturating_sub(BACKDATE.as_secs());
    let valid_before = now_secs + ttl.as_secs();

    let eph = ephemeral_ed25519()?;
    let mut nonce = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut serial = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut serial);
    let key_id = format!("bssh:ai:{}", hex(&nonce[..16]));

    let mut builder = Builder::new(
        nonce.to_vec(),
        eph.public_key().key_data().clone(),
        valid_after,
        valid_before,
    )
    .map_err(|e| anyhow!("certificate builder: {e}"))?;
    builder
        .serial(u64::from_be_bytes(serial))
        .and_then(|b| b.cert_type(CertType::User))
        .and_then(|b| b.key_id(key_id.clone()))
        .and_then(|b| b.valid_principal(AGENT_PRINCIPAL))
        .and_then(|b| b.critical_option("force-command", force_command))
        .map_err(|e| anyhow!("certificate fields: {e}"))?;
    if let Some(sa) = source_address {
        builder
            .critical_option("source-address", sa)
            .map_err(|e| anyhow!("certificate fields: {e}"))?;
    }
    let cert = builder
        .sign(ca)
        .map_err(|e| anyhow!("sign certificate: {e}"))?;

    let issued = IssuedCert {
        private_key_openssh: eph
            .to_openssh(LineEnding::LF)
            .map_err(|e| anyhow!("encode ephemeral key: {e}"))?,
        certificate_openssh: cert
            .to_openssh()
            .map_err(|e| anyhow!("encode certificate: {e}"))?,
        key_id,
    };
    verify_issued(
        &issued,
        ca.public_key(),
        force_command,
        source_address,
        ttl,
        now,
    )
    .context("freshly issued certificate failed verification")?;
    Ok(issued)
}

/// Check a certificate the way a host would, plus our own invariants: user certificate,
/// signed by `ca`, principal `bssh-agent` only, exactly the expected critical options,
/// no extensions, made for the ephemeral key, valid now and no longer than `ttl`.
pub fn verify_issued(
    issued: &IssuedCert,
    ca: &PublicKey,
    force_command: &str,
    source_address: Option<&str>,
    ttl: Duration,
    now: SystemTime,
) -> Result<()> {
    let cert = Certificate::from_openssh(&issued.certificate_openssh)
        .map_err(|e| anyhow!("certificate does not parse: {e}"))?;
    let key = PrivateKey::from_openssh(issued.private_key_openssh.as_bytes())
        .map_err(|e| anyhow!("ephemeral key does not parse: {e}"))?;
    let now_secs = unix_secs(now)?;

    cert.verify_signature()
        .map_err(|e| anyhow!("bad signature: {e}"))?;
    cert.validate_at(now_secs, [&ca.fingerprint(HashAlg::Sha256)])
        .map_err(|e| anyhow!("not valid for this CA now: {e}"))?;
    if cert.cert_type() != CertType::User {
        bail!("not a user certificate");
    }
    if cert.key_id() != issued.key_id || !cert.key_id().starts_with("bssh:ai:") {
        bail!("unexpected key id {:?}", cert.key_id());
    }
    if cert.valid_principals() != [AGENT_PRINCIPAL.to_string()] {
        bail!("unexpected principals {:?}", cert.valid_principals());
    }
    let expected_opts = 1 + usize::from(source_address.is_some());
    let opts = cert.critical_options();
    if opts.len() != expected_opts
        || opts.get("force-command").map(String::as_str) != Some(force_command)
        || opts.get("source-address").map(String::as_str) != source_address
    {
        bail!("unexpected critical options");
    }
    if !cert.extensions().is_empty() {
        bail!("certificate carries extensions (pty/forwarding must stay off)");
    }
    if cert.public_key() != key.public_key().key_data() {
        bail!("certificate is not for the ephemeral key");
    }
    if cert.valid_before() > now_secs + ttl.as_secs() {
        bail!("certificate outlives the requested lifetime");
    }
    Ok(())
}

/// `source-address` takes a comma-separated list of IPs and CIDRs.
fn validate_source_address(list: &str) -> Result<()> {
    if list.is_empty() {
        bail!("source-address is empty");
    }
    for item in list.split(',') {
        let (ip, prefix) = match item.split_once('/') {
            Some((ip, p)) => (ip, Some(p)),
            None => (item, None),
        };
        let addr: std::net::IpAddr = ip
            .parse()
            .map_err(|_| anyhow!("source-address entry {item:?} is not an IP or CIDR"))?;
        if let Some(p) = prefix {
            let max = if addr.is_ipv4() { 32 } else { 128 };
            let n: u8 = p
                .parse()
                .map_err(|_| anyhow!("source-address entry {item:?} has a bad prefix"))?;
            if n > max {
                bail!("source-address entry {item:?} has a bad prefix");
            }
        }
    }
    Ok(())
}

/// A fresh Ed25519 key. The seed comes from the OS RNG and never touches the disk.
fn ephemeral_ed25519() -> Result<PrivateKey> {
    let mut seed = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut *seed);
    Ok(PrivateKey::from(Ed25519Keypair::from_seed(&seed)))
}

fn unix_secs(t: SystemTime) -> Result<u64> {
    Ok(t.duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow!("system clock is before 1970"))?
        .as_secs())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("create {}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("chmod {}", dir.display()))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))
}

#[cfg(unix)]
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    f.write_all(bytes)
        .with_context(|| format!("write {}", path.display()))
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(unix)]
fn check_private_mode(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        bail!(
            "{} is readable by others (mode {:o}); run chmod 600 on it",
            path.display(),
            mode & 0o777
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private_mode(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_in(dir: &Path) -> AppConfig {
        AppConfig {
            database_path: dir.join("env").join("bssh.db"),
            ..AppConfig::default()
        }
    }

    fn test_ca() -> PrivateKey {
        ephemeral_ed25519().expect("ca key")
    }

    #[test]
    fn issue_and_verify_round_trip() {
        let ca = test_ca();
        let now = SystemTime::now();
        let ttl = Duration::from_secs(120);
        let issued = issue_with_ca(
            &ca,
            "ls -l /var/log",
            ttl,
            Some("10.0.0.0/8,2001:db8::1"),
            now,
        )
        .expect("issue");
        verify_issued(
            &issued,
            ca.public_key(),
            "ls -l /var/log",
            Some("10.0.0.0/8,2001:db8::1"),
            ttl,
            now,
        )
        .expect("verify");

        let cert = Certificate::from_openssh(&issued.certificate_openssh).expect("parse");
        assert_eq!(cert.valid_principals(), [AGENT_PRINCIPAL.to_string()]);
        assert!(cert.key_id().starts_with("bssh:ai:"));
        assert_eq!(cert.valid_after(), unix_secs(now).unwrap() - 300);
        assert_eq!(cert.valid_before(), unix_secs(now).unwrap() + 120);
        assert!(cert.extensions().is_empty());
        assert_eq!(cert.critical_options().len(), 2);
    }

    #[test]
    fn force_command_is_stored_verbatim() {
        let ca = test_ca();
        let cmd = "sh -c 'echo \"a b\" && true'; $(x) `y`\nnext";
        let issued = issue_with_ca(&ca, cmd, Duration::from_secs(60), None, SystemTime::now())
            .expect("issue");
        let cert = Certificate::from_openssh(&issued.certificate_openssh).expect("parse");
        assert_eq!(
            cert.critical_options()
                .get("force-command")
                .map(String::as_str),
            Some(cmd)
        );
        assert!(cert.critical_options().get("source-address").is_none());
    }

    #[test]
    fn every_certificate_is_unique() {
        let ca = test_ca();
        let a = issue_with_ca(&ca, "id", Duration::from_secs(60), None, SystemTime::now()).unwrap();
        let b = issue_with_ca(&ca, "id", Duration::from_secs(60), None, SystemTime::now()).unwrap();
        assert_ne!(a.key_id, b.key_id);
        assert_ne!(a.certificate_openssh, b.certificate_openssh);
        assert_ne!(*a.private_key_openssh, *b.private_key_openssh);
    }

    #[test]
    fn wrong_ca_and_expiry_are_rejected() {
        let ca = test_ca();
        let other = test_ca();
        let now = SystemTime::now();
        let ttl = Duration::from_secs(120);
        let issued = issue_with_ca(&ca, "id", ttl, None, now).unwrap();
        assert!(verify_issued(&issued, other.public_key(), "id", None, ttl, now).is_err());
        let later = now + Duration::from_secs(121);
        assert!(verify_issued(&issued, ca.public_key(), "id", None, ttl, later).is_err());
        assert!(verify_issued(&issued, ca.public_key(), "other", None, ttl, now).is_err());
        assert!(verify_issued(&issued, ca.public_key(), "id", Some("10.0.0.1"), ttl, now).is_err());
    }

    #[test]
    fn bad_requests_are_refused() {
        let ca = test_ca();
        let now = SystemTime::now();
        let ok = Duration::from_secs(60);
        assert!(issue_with_ca(&ca, "", ok, None, now).is_err());
        assert!(issue_with_ca(&ca, "a\0b", ok, None, now).is_err());
        assert!(issue_with_ca(&ca, "id", Duration::ZERO, None, now).is_err());
        assert!(
            issue_with_ca(&ca, "id", MAX_CERT_TTL + Duration::from_secs(1), None, now).is_err()
        );
        assert!(issue_with_ca(&ca, "id", MAX_CERT_TTL, None, now).is_ok());
        for bad in [
            "",
            "not-an-ip",
            "10.0.0.1/33",
            "::1/129",
            "10.0.0.1,",
            "10.0.0.1/x",
            "1.2.3.4;x",
        ] {
            assert!(
                issue_with_ca(&ca, "id", ok, Some(bad), now).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    #[test]
    fn init_ca_plain_then_issue() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config_in(tmp.path());
        assert!(!ca_available(&config));
        assert!(ca_public_key(&config).is_err());

        let pub_line = init_ca(&config, None).expect("init");
        assert!(ca_available(&config));
        assert!(!ca_sealed(&config));
        assert_eq!(ca_public_key(&config).unwrap(), pub_line);
        assert!(ca_fingerprint(&config).unwrap().starts_with("SHA256:"));
        assert!(init_ca(&config, None).is_err(), "no silent overwrite");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: PathBuf| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(ca_dir(&config)), 0o700);
            assert_eq!(mode(plain_path(&config)), 0o600);
        }

        let ttl = Duration::from_secs(120);
        let issued = issue(&config, "uptime", ttl, None).expect("issue");
        let ca = PublicKey::from_openssh(&pub_line).unwrap();
        verify_issued(&issued, &ca, "uptime", None, ttl, SystemTime::now()).expect("verify");
    }

    #[cfg(unix)]
    #[test]
    fn issue_refuses_a_world_readable_ca_key() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let config = config_in(tmp.path());
        init_ca(&config, None).unwrap();
        let key = plain_path(&config);
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        let err = issue(&config, "id", Duration::from_secs(60), None).unwrap_err();
        assert!(err.to_string().contains("readable by others"), "{err}");
    }

    #[test]
    fn sealed_ca_needs_its_passphrase() {
        let tmp = tempfile::tempdir().unwrap();
        let config = config_in(tmp.path());
        let pub_line = init_ca(&config, Some("correct horse battery")).expect("init");
        assert!(ca_sealed(&config));
        assert!(
            !plain_path(&config).exists(),
            "no plaintext copy next to the sealed key"
        );
        let sealed = std::fs::read(sealed_path(&config)).unwrap();
        assert!(!String::from_utf8_lossy(&sealed).contains("OPENSSH PRIVATE KEY"));

        // `load_ca` reads the process environment; this is the only test that sets it.
        std::env::remove_var(CA_PASSPHRASE_ENV);
        let err = issue(&config, "id", Duration::from_secs(60), None).unwrap_err();
        assert!(err.to_string().contains(CA_PASSPHRASE_ENV), "{err}");

        std::env::set_var(CA_PASSPHRASE_ENV, "wrong passphrase");
        assert!(issue(&config, "id", Duration::from_secs(60), None).is_err());

        std::env::set_var(CA_PASSPHRASE_ENV, "correct horse battery");
        let issued = issue(&config, "id", Duration::from_secs(60), None);
        std::env::remove_var(CA_PASSPHRASE_ENV);
        let issued = issued.expect("issue with the right passphrase");
        let ca = PublicKey::from_openssh(&pub_line).unwrap();
        verify_issued(
            &issued,
            &ca,
            "id",
            None,
            Duration::from_secs(60),
            SystemTime::now(),
        )
        .expect("verify");
    }

    #[test]
    fn key_body_extracts_base64() {
        assert_eq!(key_body("ssh-ed25519 AAAAC3 comment"), Some("AAAAC3"));
        assert_eq!(key_body("ssh-ed25519"), None);
    }
}

/// The certificates we mint, offered to a real SSH server over the wire: the server checks
/// them the way sshd does (CA signature, validity window, principal, login account) and
/// runs the forced command instead of the one the client asks for.
#[cfg(test)]
mod wire_tests {
    use super::*;
    use russh::keys::ssh_key::private::Ed25519Keypair;
    use russh::keys::PublicKeyOrCertificate;
    use russh::server::{self, Auth, Msg, Server as _, Session};
    use russh::{client, Channel, ChannelId};
    use std::sync::Arc;
    use tokio::net::TcpListener;

    fn key(seed: u8) -> PrivateKey {
        PrivateKey::from(Ed25519Keypair::from_seed(&[seed; 32]))
    }

    #[derive(Clone)]
    struct TestServer {
        ca: PublicKey,
    }

    struct TestHandler {
        ca: PublicKey,
        force_command: Option<String>,
        source_address: Option<String>,
    }

    impl server::Server for TestServer {
        type Handler = TestHandler;
        fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> TestHandler {
            TestHandler {
                ca: self.ca.clone(),
                force_command: None,
                source_address: None,
            }
        }
    }

    impl server::Handler for TestHandler {
        type Error = russh::Error;

        async fn auth_openssh_certificate(
            &mut self,
            user: &str,
            cert: &Certificate,
        ) -> Result<Auth, Self::Error> {
            let now = unix_secs(SystemTime::now()).unwrap_or(0);
            // AuthorizedPrincipalsFile for `bssh-agent`: principal bssh-agent, account bssh-agent.
            let ok = cert
                .validate_at(now, [&self.ca.fingerprint(HashAlg::Sha256)])
                .is_ok()
                && user == AGENT_PRINCIPAL
                && cert.valid_principals().iter().any(|p| p == AGENT_PRINCIPAL);
            if ok {
                self.force_command = cert.critical_options().get("force-command").cloned();
                self.source_address = cert.critical_options().get("source-address").cloned();
                Ok(Auth::Accept)
            } else {
                Ok(Auth::reject())
            }
        }

        async fn channel_open_session(
            &mut self,
            _channel: Channel<Msg>,
            reply: server::ChannelOpenHandle,
            _session: &mut Session,
        ) -> Result<(), Self::Error> {
            reply.accept().await;
            Ok(())
        }

        async fn pty_request(
            &mut self,
            channel: ChannelId,
            _: &str,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: &[(russh::Pty, u32)],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            // No `permit-pty` extension on our certificates: sshd refuses the request.
            session.channel_failure(channel)?;
            Ok(())
        }

        async fn exec_request(
            &mut self,
            channel: ChannelId,
            data: &[u8],
            session: &mut Session,
        ) -> Result<(), Self::Error> {
            let ran = self
                .force_command
                .clone()
                .unwrap_or_else(|| String::from_utf8_lossy(data).into_owned());
            session.channel_success(channel)?;
            let src = self.source_address.clone().unwrap_or_default();
            session.data(channel, format!("ran: {ran}|src: {src}").into_bytes())?;
            session.exit_status_request(channel, 0)?;
            session.eof(channel)?;
            session.close(channel)?;
            Ok(())
        }
    }

    struct AcceptAll;
    impl client::Handler for AcceptAll {
        type Error = russh::Error;
        async fn check_server_key(
            &mut self,
            _: &PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            Ok(true)
        }
    }

    async fn start(ca: &PrivateKey) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let config = Arc::new(server::Config {
            keys: vec![key(99)],
            auth_rejection_time: Duration::from_millis(1),
            auth_rejection_time_initial: Some(Duration::from_millis(1)),
            ..Default::default()
        });
        let mut srv = TestServer {
            ca: ca.public_key().clone(),
        };
        tokio::spawn(async move {
            let _ = srv.run_on_socket(config, &listener).await;
        });
        addr
    }

    /// Log in with `issued` as `user`. `(accepted, output, pty_granted)`.
    async fn login(
        addr: std::net::SocketAddr,
        user: &str,
        issued: &IssuedCert,
    ) -> Result<(bool, String, bool)> {
        let key = PrivateKey::from_openssh(issued.private_key_openssh.as_bytes())
            .map_err(|e| anyhow!("{e}"))?;
        let cert =
            Certificate::from_openssh(&issued.certificate_openssh).map_err(|e| anyhow!("{e}"))?;
        let mut handle =
            client::connect(Arc::new(client::Config::default()), addr, AcceptAll).await?;
        let ok = handle
            .authenticate_openssh_cert(user, Arc::new(key), cert)
            .await?
            .success();
        let (mut out, mut pty) = (String::new(), false);
        if ok {
            let mut ch = handle.channel_open_session().await?;
            pty = ch
                .request_pty(true, "xterm", 80, 24, 0, 0, &[])
                .await
                .is_ok()
                && matches!(ch.wait().await, Some(russh::ChannelMsg::Success));
            ch.exec(true, "the client's own command".as_bytes()).await?;
            while let Some(msg) = ch.wait().await {
                if let russh::ChannelMsg::Data { data } = msg {
                    out.push_str(&String::from_utf8_lossy(&data));
                }
            }
        }
        Ok((ok, out, pty))
    }

    #[tokio::test]
    async fn issued_certificate_logs_in_and_the_forced_command_wins() {
        let ca = key(1);
        let addr = start(&ca).await;
        let issued = issue_with_ca(
            &ca,
            "journalctl -n 5",
            Duration::from_secs(60),
            Some("127.0.0.1"),
            SystemTime::now(),
        )
        .expect("issue");
        let (ok, out, pty) = login(addr, AGENT_PRINCIPAL, &issued).await.expect("login");
        assert!(ok, "the server should accept the certificate");
        assert_eq!(out, "ran: journalctl -n 5|src: 127.0.0.1");
        assert!(!pty, "no permit-pty extension, no terminal");
    }

    #[tokio::test]
    async fn host_refuses_a_certificate_from_another_ca() {
        let addr = start(&key(1)).await;
        let rogue = key(2);
        let issued = issue_with_ca(
            &rogue,
            "id",
            Duration::from_secs(60),
            None,
            SystemTime::now(),
        )
        .unwrap();
        assert!(!login(addr, AGENT_PRINCIPAL, &issued).await.unwrap().0);
    }

    #[tokio::test]
    async fn host_refuses_an_expired_certificate() {
        let ca = key(1);
        let addr = start(&ca).await;
        let past = SystemTime::now() - Duration::from_secs(7200);
        let issued = issue_with_ca(&ca, "id", Duration::from_secs(120), None, past).unwrap();
        assert!(!login(addr, AGENT_PRINCIPAL, &issued).await.unwrap().0);
    }

    #[tokio::test]
    async fn host_refuses_other_accounts() {
        let ca = key(1);
        let addr = start(&ca).await;
        let issued =
            issue_with_ca(&ca, "id", Duration::from_secs(60), None, SystemTime::now()).unwrap();
        assert!(!login(addr, "root", &issued).await.unwrap().0);
    }
}
