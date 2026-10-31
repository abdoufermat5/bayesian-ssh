//! Shared Teams state, init errors and the worker-thread plumbing.

//! bssh Teams for the desktop app: the same workflows as `bssh teams`.
//!
//! Device keys are unlocked once (`teams_unlock`) and kept in [`TeamsState`]
//! until `teams_lock` or quit, so the passphrase KDF runs once per session.
//! Each operation runs on its own thread with a single-threaded runtime: the
//! sync engine's futures hold the SQLite connection and aren't `Send`.

use crate::commands::CommandError;
use bayesian_ssh::config::AppConfig;
use bayesian_ssh::services::teams::TeamsStore;
use bayesian_ssh::services::teams::crypto::DeviceKeys;
use bayesian_ssh::services::teams::session::{self, Session};
use serde::Serialize;
use std::future::Future;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct TeamsState {
    pub(super) keys: Mutex<Option<Arc<DeviceKeys>>>,
}

impl TeamsState {
    pub(super) fn get(&self) -> Option<Arc<DeviceKeys>> {
        self.keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn set(&self, keys: Option<Arc<DeviceKeys>>) {
        *self
            .keys
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = keys;
    }

    pub(super) fn unlocked(&self) -> Result<Arc<DeviceKeys>, CommandError> {
        self.get().ok_or_else(|| {
            CommandError::from("Teams is locked: unlock it with your passphrase first.")
        })
    }
}

pub(super) fn message(e: anyhow::Error) -> String {
    match session::explain(&e) {
        Some(hint) => format!("{hint} ({e:#})"),
        None => format!("{e:#}"),
    }
}

pub(super) fn command_error(e: anyhow::Error) -> CommandError {
    message(e).into()
}

pub(super) fn open_store() -> anyhow::Result<(TeamsStore, AppConfig)> {
    let config = AppConfig::load(None)?;
    let store = TeamsStore::open(&config)?;
    Ok((store, config))
}

/// Why `teams_init` didn't finish. A mailed code means the form must ask for
/// it; the UI retries with `emailCode` (and can mail another with
/// `teams_request_email_code`).
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamsInitError {
    /// The server mailed a code: ask for it and retry.
    EmailCodeSent {
        email: String,
        expires_in_secs: u64,
    },
    /// The code given was wrong, expired or used; a new one was mailed.
    /// The code was wrong, expired or used. Nothing new is mailed: the user
    /// checks it or asks for a new one (each code mailed costs relay quota and
    /// replaces the one in their inbox).
    EmailCodeRejected {
        email: String,
    },
    Failed {
        message: String,
    },
}

impl TeamsInitError {
    pub(super) fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for TeamsInitError {
    fn from(e: anyhow::Error) -> Self {
        Self::failed(message(e))
    }
}

/// Run an `init` attempt on its own thread; its failure keeps the shape the UI
/// switches on instead of turning everything into a string.
pub(super) fn on_init_thread<T: Send + 'static>(
    job: impl FnOnce() -> std::result::Result<T, TeamsInitError> + Send + 'static,
) -> std::result::Result<T, TeamsInitError> {
    std::thread::spawn(job)
        .join()
        .map_err(|_| TeamsInitError::failed("the Teams worker crashed"))?
}

/// Run `job` on a dedicated thread and wait for it.
pub(super) fn on_teams_thread<T: Send + 'static>(
    job: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, CommandError> {
    std::thread::spawn(job)
        .join()
        .map_err(|_| CommandError::from("the Teams worker crashed"))?
        .map_err(command_error)
}

pub(super) fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a current-thread tokio runtime always builds")
        .block_on(future)
}

pub(super) type SessionFuture<'s, T> =
    std::pin::Pin<Box<dyn Future<Output = anyhow::Result<T>> + 's>>;

/// Open the store and a session over the unlocked keys, then run `op`.
pub(super) fn with_session<T, F>(keys: Arc<DeviceKeys>, op: F) -> Result<T, CommandError>
where
    T: Send + 'static,
    F: for<'s> FnOnce(&'s Session<'s>) -> SessionFuture<'s, T> + Send + 'static,
{
    on_teams_thread(move || {
        let (store, config) = open_store()?;
        let session = Session::with_keys(&store, &config, keys)?;
        block_on(op(&session))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sync engine's futures aren't `Send`; they must still run.
    #[test]
    fn runs_non_send_futures() {
        let out = on_teams_thread(|| {
            Ok(block_on(async {
                let local = std::rc::Rc::new(41);
                tokio::task::yield_now().await;
                *local + 1
            }))
        });
        assert_eq!(out.unwrap(), 42);
    }

    /// Tauri calls commands from inside its own tokio runtime; starting the
    /// worker's runtime there must not panic ("runtime within a runtime").
    #[tokio::test]
    async fn works_from_inside_a_runtime() {
        let out = on_teams_thread(|| Ok(block_on(async { 7 })));
        assert_eq!(out.unwrap(), 7);
    }

    #[test]
    fn worker_errors_become_messages() {
        let out: Result<(), CommandError> = on_teams_thread(|| Err(anyhow::anyhow!("boom")));
        assert_eq!(out.unwrap_err().to_string(), "boom");
    }

    /// The views switch on `kind`; these are the wire shapes they expect.
    #[test]
    fn init_errors_tell_the_ui_to_ask_for_a_code() {
        let sent = serde_json::to_value(TeamsInitError::EmailCodeSent {
            email: "ana@example.com".into(),
            expires_in_secs: 900,
        })
        .unwrap();
        assert_eq!(sent["kind"], "email_code_sent");
        assert_eq!(sent["email"], "ana@example.com");
        assert_eq!(sent["expires_in_secs"], 900);

        let rejected = serde_json::to_value(TeamsInitError::EmailCodeRejected {
            email: "ana@example.com".into(),
        })
        .unwrap();
        assert_eq!(rejected["kind"], "email_code_rejected");

        let failed = serde_json::to_value(TeamsInitError::from(anyhow::anyhow!("boom"))).unwrap();
        assert_eq!(failed["kind"], "failed");
        assert_eq!(failed["message"], "boom");
    }
}
