//! Multi-host upload commands.
//!
//! Fans one or more local files/directories out to several servers in
//! parallel (bounded by `parallel`), streaming per-host progress to the
//! frontend as `upload-event` and a single terminal `upload-finished`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bayesian_ssh::models::Connection;
use bayesian_ssh::services::upload::{
    upload_to_hosts, HostUploadResult, UploadEvent, UploadEventFn, UploadRequest,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::task::AbortHandle;
use uuid::Uuid;

use super::get_db_and_config;

/// Hosts uploaded to at once when the caller does not specify a value.
const DEFAULT_PARALLEL_HOSTS: usize = 8;
/// Remote permission bits applied to uploaded files.
const UPLOAD_MODE: u32 = 0o644;

/// Live upload jobs, keyed by job id. Each entry keeps the abort handle so
/// `cancel_upload` can stop a running job; the task removes its own entry when
/// it finishes.
#[derive(Default)]
pub struct UploadState {
    jobs: Arc<Mutex<HashMap<String, AbortHandle>>>,
}

impl UploadState {
    fn jobs(&self) -> &Arc<Mutex<HashMap<String, AbortHandle>>> {
        &self.jobs
    }
}

/// Payload of the `upload-event` Tauri event.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UploadEventPayload {
    job_id: String,
    local_path: String,
    event: UploadEvent,
}

/// Per-local-path outcome carried by `upload-finished`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LocalUploadResults {
    local_path: String,
    hosts: Vec<HostUploadResult>,
}

/// Payload of the `upload-finished` Tauri event.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct UploadFinishedPayload {
    job_id: String,
    cancelled: bool,
    results: Vec<LocalUploadResults>,
    error: Option<String>,
}

/// Start a background upload job and return its id immediately.
///
/// Connection ids may be UUIDs or connection names, and are resolved in the
/// order given. Invalid input is rejected up front; per-host failures are
/// reported through `upload-event` / `upload-finished`.
#[tauri::command]
pub async fn start_upload(
    app: AppHandle,
    state: State<'_, UploadState>,
    connection_ids: Vec<String>,
    local_paths: Vec<String>,
    remote_dir: String,
    parallel: Option<usize>,
) -> Result<String, String> {
    if connection_ids.is_empty() {
        return Err("Select at least one host.".to_string());
    }
    if local_paths.is_empty() {
        return Err("Add at least one file or folder to upload.".to_string());
    }

    let remote_dir = remote_dir.trim();
    if remote_dir.is_empty() {
        return Err("Enter a destination directory.".to_string());
    }

    let (db, config) = get_db_and_config()?;
    let all = db
        .list_connections(None, false)
        .map_err(|e| e.to_string())?;
    let targets: Vec<Connection> = connection_ids
        .iter()
        .filter_map(|id| {
            all.iter()
                .find(|c| &c.id.to_string() == id || &c.name == id)
                .cloned()
        })
        .collect();
    if targets.is_empty() {
        return Err("No valid target connections selected".to_string());
    }

    let mut locals = Vec::with_capacity(local_paths.len());
    for path in &local_paths {
        let path = PathBuf::from(path);
        if !path.exists() {
            return Err(format!("Local path not found: {}", path.display()));
        }
        locals.push(path);
    }

    // Destination directory without its trailing slash; each entry below is
    // resolved against it.
    let dest_root = remote_dir.trim_end_matches('/').to_string();
    let parallel = parallel.unwrap_or(DEFAULT_PARALLEL_HOSTS).max(1);

    let job_id = Uuid::new_v4().to_string();
    let jobs = Arc::clone(state.jobs());
    let jobs_task = Arc::clone(&jobs);

    // The task waits on this channel until its abort handle is registered, so
    // a cancel that races the spawn can never miss the job.
    let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();
    let app_task = app.clone();
    let job_task = job_id.clone();

    let handle = tokio::spawn(async move {
        if start_rx.await.is_err() {
            return;
        }

        let mut results = Vec::with_capacity(locals.len());
        let mut error: Option<String> = None;

        for local in locals {
            let local_label = local.to_string_lossy().to_string();
            let is_dir = local.is_dir();
            // Files: the trailing slash keeps the local basename. Directories:
            // the service copies the tree's *contents* into the remote path
            // as-is, so the folder basename has to be appended here.
            let remote = if is_dir {
                let name = local
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                match (dest_root.is_empty(), name.is_empty()) {
                    (true, true) => "/".to_string(),
                    (true, false) => format!("/{name}"),
                    (false, true) => format!("{dest_root}/"),
                    (false, false) => format!("{dest_root}/{name}"),
                }
            } else {
                format!("{dest_root}/")
            };

            let event_app = app_task.clone();
            let event_job = job_task.clone();
            let event_local = local_label.clone();
            let on_event: UploadEventFn = Arc::new(move |event| {
                let _ = event_app.emit(
                    "upload-event",
                    UploadEventPayload {
                        job_id: event_job.clone(),
                        local_path: event_local.clone(),
                        event,
                    },
                );
            });

            let req = UploadRequest {
                recursive: is_dir,
                local,
                remote,
                mode: UPLOAD_MODE,
                offset: 0,
            };

            match upload_to_hosts(&config, targets.clone(), req, parallel, Some(on_event)).await {
                Ok(hosts) => results.push(LocalUploadResults {
                    local_path: local_label,
                    hosts,
                }),
                Err(e) => error = Some(e.to_string()),
            }
        }

        let _ = app_task.emit(
            "upload-finished",
            UploadFinishedPayload {
                job_id: job_task.clone(),
                cancelled: false,
                results,
                error,
            },
        );
        if let Ok(mut map) = jobs_task.lock() {
            map.remove(&job_task);
        }
    });

    jobs.lock()
        .map_err(|_| "Upload state lock poisoned".to_string())?
        .insert(job_id.clone(), handle.abort_handle());
    let _ = start_tx.send(());

    Ok(job_id)
}

/// Abort a running upload job. Emits `upload-finished` with `cancelled: true`
/// so the UI can settle regardless of how far the job got.
#[tauri::command]
pub fn cancel_upload(
    app: AppHandle,
    state: State<'_, UploadState>,
    job_id: String,
) -> Result<(), String> {
    let handle = state
        .jobs()
        .lock()
        .map_err(|_| "Upload state lock poisoned".to_string())?
        .remove(&job_id);

    // Nothing to abort: the job already finished (and announced itself).
    let Some(handle) = handle else {
        return Ok(());
    };

    handle.abort();
    let _ = app.emit(
        "upload-finished",
        UploadFinishedPayload {
            job_id,
            cancelled: true,
            results: Vec::new(),
            error: None,
        },
    );
    Ok(())
}
