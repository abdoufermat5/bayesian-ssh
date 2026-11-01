use super::*;
use crate::services::ai::audit::{self, LocalAudit};
use crate::services::ai::report;
use crate::services::ai::store::RunRecord;
use crate::services::ai::types::{Decision, ToolCall, ToolName};
use std::sync::Arc;

fn cfg(dir: &Path) -> AppConfig {
    AppConfig {
        database_path: dir.join("env").join("bssh.db"),
        ..Default::default()
    }
}

fn started(id: &str) -> AiEvent {
    AiEvent::RunStarted {
        run_id: id.into(),
        client: "test-agent".into(),
        on_behalf_of: "alice".into(),
        policy: "default".into(),
        policy_hash: "h".into(),
    }
}

fn call(id: &str, n: usize) -> AiEvent {
    AiEvent::Call {
        call: ToolCall {
            run_id: id.into(),
            call_id: format!("c{n}"),
            tool: ToolName::RunReadonly,
            host: Some("web-1".into()),
            command: Some(format!("echo {n}")),
            path: None,
            args: serde_json::json!({}),
        },
        decision: Decision::Allow {
            reason: "read-only".into(),
        },
    }
}

fn ended(id: &str) -> AiEvent {
    AiEvent::RunEnded {
        run_id: id.into(),
        reason: "done".into(),
    }
}

/// A run in the log: start, two calls and (when `finish`) an end, with a recording.
fn add_run(c: &AppConfig, a: &LocalAudit, id: &str, finish: bool) {
    a.append(&started(id)).unwrap();
    for n in 0..2 {
        a.append(&call(id, n)).unwrap();
        let cast = report::cast_path(c, id, &format!("c{n}"));
        std::fs::create_dir_all(cast.parent().unwrap()).unwrap();
        std::fs::write(cast, format!("cast of {id} {n}")).unwrap();
    }
    if finish {
        a.append(&ended(id)).unwrap();
    }
}

fn add_backup(c: &AppConfig, id: &str) -> PathBuf {
    let dir = ai_dir(c).join("backups").join(id).join("change-1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("s1.orig"), "original").unwrap();
    ai_dir(c).join("backups").join(id)
}

fn req(selection: PruneSelection, mode: PruneMode) -> PruneRequest {
    PruneRequest {
        selection,
        mode,
        what: PruneWhat::default(),
        archive_dir: None,
    }
}

fn runs_req(ids: &[&str]) -> PruneRequest {
    req(
        PruneSelection::Runs {
            ids: ids.iter().map(|s| s.to_string()).collect(),
        },
        PruneMode::Delete,
    )
}

fn seqs(c: &AppConfig) -> Vec<u64> {
    audit::read_entries(c)
        .unwrap()
        .iter()
        .map(|e| e.seq)
        .collect()
}

fn raw_lines(c: &AppConfig) -> Vec<audit::RawEntry> {
    std::fs::read_to_string(audit::events_path(c))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn three_runs() -> (tempfile::TempDir, AppConfig, LocalAudit) {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    for id in ["run-a", "run-b", "run-c"] {
        add_run(&c, &a, id, true);
    }
    (d, c, a)
}

fn mid_stub_fixture() -> (tempfile::TempDir, AppConfig, LocalAudit) {
    let (d, c, a) = three_runs();
    let out = execute(&c, &runs_req(&["run-b"])).unwrap();
    assert!(out.verify_ok);
    (d, c, a)
}

#[test]
fn stubs_verify_and_leave_kept_runs_unchanged() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    for id in ["run-a", "run-b", "run-c"] {
        add_run(&c, &a, id, true);
    }
    let before_a = report::run_report(&c, "run-a").unwrap().to_json().unwrap();
    let before_c = report::run_report(&c, "run-c").unwrap().to_json().unwrap();

    let out = execute(&c, &runs_req(&["run-b"])).unwrap();
    assert!(out.verify_ok);
    assert_eq!(out.plan.entries, 4);

    let v = audit::verify(&c).unwrap();
    assert!(v.ok, "{v:?}");
    let raw = raw_lines(&c);
    assert_eq!(raw.len(), 12, "a stub replaces each entry, none is dropped");
    let stubs: Vec<_> = raw.iter().filter(|e| e.pruned).collect();
    assert_eq!(stubs.len(), 4);
    assert!(stubs.iter().all(|e| e.event.is_null()));
    assert_eq!(
        audit::read_entries(&c).unwrap().len(),
        8,
        "stubs are skipped"
    );
    assert!(!audit::anchor_path(&c).exists(), "b is in the middle");
    assert_eq!(
        report::run_report(&c, "run-a").unwrap().to_json().unwrap(),
        before_a
    );
    assert_eq!(
        report::run_report(&c, "run-c").unwrap().to_json().unwrap(),
        before_c
    );
    assert!(report::run_report(&c, "run-b").map_or(true, |r| r.calls.is_empty()));
    // Its recordings are gone, the others' stay.
    assert!(report::run_casts(&c, "run-b").is_empty());
    assert_eq!(report::run_casts(&c, "run-a").len(), 2);
}

#[test]
fn leading_stubs_fold_into_the_anchor_and_append_continues() {
    let (_d, c, a) = three_runs();
    execute(&c, &runs_req(&["run-a"])).unwrap();
    let anchor: audit::Anchor =
        serde_json::from_str(&std::fs::read_to_string(audit::anchor_path(&c)).unwrap()).unwrap();
    assert_eq!(anchor.seq, 4);
    assert_eq!(raw_lines(&c)[0].seq, 5);
    assert!(audit::verify(&c).unwrap().ok);

    a.append(&started("run-d")).unwrap();
    assert_eq!(*seqs(&c).last().unwrap(), 13);
    assert!(audit::verify(&c).unwrap().ok);

    // Prune the rest of the ended runs: the file empties, the anchor moves to the end.
    let out = execute(&c, &req(PruneSelection::All, PruneMode::Delete)).unwrap();
    assert!(out.verify_ok);
    assert_eq!(out.plan.skipped_active, vec!["run-d".to_string()]);
    assert_eq!(raw_lines(&c).len(), 1, "only the live run's entry is left");
    a.append(&call("run-d", 0)).unwrap();
    assert_eq!(*seqs(&c).last().unwrap(), 14);
    let v = audit::verify(&c).unwrap();
    assert!(v.ok, "{v:?}");

    // Everything gone but a stub-free tail: empty log + anchor still appends.
    std::fs::write(audit::events_path(&c), "").unwrap();
    let a2 = LocalAudit::open(&c).unwrap();
    let seq = a2.append(&started("run-e")).unwrap();
    assert!(seq > 1, "continues after the anchor, got {seq}");
    assert!(audit::verify(&c).unwrap().ok);
}

#[test]
fn append_after_everything_is_dropped() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    add_run(&c, &a, "run-a", true);
    execute(&c, &req(PruneSelection::All, PruneMode::Delete)).unwrap();
    assert_eq!(std::fs::read_to_string(audit::events_path(&c)).unwrap(), "");
    assert!(audit::verify(&c).unwrap().ok);
    let seq = a.append(&started("run-b")).unwrap();
    assert_eq!(seq, 5);
    assert!(audit::verify(&c).unwrap().ok);
}

#[test]
fn wipe_resets_the_chain_and_removes_everything() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    add_run(&c, &a, "run-a", true);
    add_run(&c, &a, "run-b", true);
    add_backup(&c, "run-a");
    let store = AiStore::open(&c).unwrap();
    store
        .start_run(&RunRecord {
            id: "run-a".into(),
            client: "x".into(),
            on_behalf_of: "alice".into(),
            policy: "default".into(),
            started: Utc::now().to_rfc3339(),
            ended: None,
            reason: None,
        })
        .unwrap();
    store.end_run("run-a", "done").unwrap();
    execute(&c, &runs_req(&["run-a"])).unwrap();
    assert!(audit::anchor_path(&c).exists());
    let key = std::fs::read(audit::audit_dir(&c).join("device.key")).unwrap();

    let out = execute(&c, &req(PruneSelection::All, PruneMode::Wipe)).unwrap();
    assert!(out.verify_ok);
    assert!(!audit::events_path(&c).exists() && !audit::anchor_path(&c).exists());
    assert_eq!(
        std::fs::read(audit::audit_dir(&c).join("device.key")).unwrap(),
        key
    );
    assert!(!ai_dir(&c).join("backups").exists());
    assert!(all_casts(&c).is_empty());
    assert!(store.list_runs(false, 10).unwrap().is_empty());

    assert_eq!(a.append(&started("run-n")).unwrap(), 1, "back to genesis");
    assert!(audit::verify(&c).unwrap().ok);
}

#[test]
fn wipe_needs_all_and_no_active_runs() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    add_run(&c, &a, "run-a", true);
    add_run(&c, &a, "run-live", false);
    assert!(
        execute(
            &c,
            &req(PruneSelection::OlderThan { days: 0 }, PruneMode::Wipe)
        )
        .is_err()
    );
    let before = std::fs::read(audit::events_path(&c)).unwrap();
    let wipe = req(PruneSelection::All, PruneMode::Wipe);
    assert_eq!(plan(&c, &wipe).unwrap().skipped_active, vec!["run-live"]);
    let err = execute(&c, &wipe).unwrap_err();
    assert!(err.to_string().contains("active"), "{err}");
    assert_eq!(std::fs::read(audit::events_path(&c)).unwrap(), before);
    assert_eq!(report::run_casts(&c, "run-a").len(), 2);
}

#[test]
fn stub_tampering_is_detected() {
    let (_d, c, _a) = mid_stub_fixture();
    let path = audit::events_path(&c);
    let good = std::fs::read_to_string(&path).unwrap();
    let stub_idx = raw_lines(&c).iter().position(|e| e.pruned).unwrap();
    let mutate = |f: &dyn Fn(&mut audit::RawEntry)| {
        let mut raw = raw_lines(&c);
        f(&mut raw[stub_idx]);
        let text: String = raw
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect();
        std::fs::write(&path, text).unwrap();
        let v = audit::verify(&c).unwrap();
        std::fs::write(&path, &good).unwrap();
        v
    };
    assert!(audit::verify(&c).unwrap().ok);
    let v = mutate(&|e| e.hash = "ab".repeat(32));
    assert!(!v.ok, "{v:?}");
    let v = mutate(&|e| e.sig = "00".repeat(64));
    assert_eq!(v.error.as_deref(), Some("bad signature"));
    let v = mutate(&|e| e.prev_hash = "cd".repeat(32));
    assert!(!v.ok, "{v:?}");
    let v = mutate(&|e| e.seq += 100);
    assert!(!v.ok, "{v:?}");
    let v = mutate(&|e| e.event = serde_json::json!({"smuggled": true}));
    assert!(!v.ok, "{v:?}");
    // The untouched log still verifies.
    assert!(audit::verify(&c).unwrap().ok);
}

#[test]
fn anchor_forgery_is_detected() {
    let (_d, c, _a) = three_runs();
    execute(&c, &runs_req(&["run-a"])).unwrap();
    let path = audit::anchor_path(&c);
    let good = std::fs::read_to_string(&path).unwrap();
    assert!(audit::verify(&c).unwrap().ok);
    let edit = |f: &dyn Fn(&mut audit::Anchor)| {
        let mut a: audit::Anchor = serde_json::from_str(&good).unwrap();
        f(&mut a);
        std::fs::write(&path, serde_json::to_string(&a).unwrap()).unwrap();
        let v = audit::verify(&c).unwrap();
        std::fs::write(&path, &good).unwrap();
        v
    };
    assert!(!edit(&|a| a.hash = "ef".repeat(32)).ok);
    assert!(!edit(&|a| a.seq += 1).ok);
    assert!(!edit(&|a| a.sig = "00".repeat(64)).ok);
    // Removing the anchor leaves a log that no longer starts at genesis.
    std::fs::remove_file(&path).unwrap();
    assert!(!audit::verify(&c).unwrap().ok);
    std::fs::write(&path, &good).unwrap();
    assert!(audit::verify(&c).unwrap().ok);
}

#[test]
fn archive_keeps_hash_matched_lines_and_copies() {
    let (d, c, _a) = three_runs();
    add_backup(&c, "run-a");
    let before: HashMap<u64, String> = audit::read_entries(&c)
        .unwrap()
        .into_iter()
        .map(|e| (e.seq, e.hash))
        .collect();
    let dest = d.path().join("out").join("arch");
    let mut r = runs_req(&["run-a", "run-b"]);
    r.mode = PruneMode::Archive;
    r.archive_dir = Some(dest.clone());
    let out = execute(&c, &r).unwrap();
    assert!(out.verify_ok);
    assert_eq!(out.archive_dir.as_deref(), Some(dest.as_path()));

    let archived: Vec<audit::RawEntry> = std::fs::read_to_string(dest.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(archived.len(), 8);
    for e in &archived {
        assert_eq!(before[&e.seq], e.hash, "archived hash matches the original");
        assert!(!e.event.is_null());
    }
    // The anchor stands for the last archived entry of the leading run of stubs.
    let anchor: audit::Anchor =
        serde_json::from_str(&std::fs::read_to_string(audit::anchor_path(&c)).unwrap()).unwrap();
    assert_eq!(anchor.hash, archived.last().unwrap().hash);
    assert_eq!(anchor.seq, 8);

    assert_eq!(
        std::fs::read_dir(dest.join("recordings")).unwrap().count(),
        4
    );
    assert_eq!(
        std::fs::read(dest.join("backups/run-a/change-1/s1.orig")).unwrap(),
        b"original"
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dest.join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["runs"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["entries"], 8);
    assert_eq!(manifest["bssh_version"], env!("CARGO_PKG_VERSION"));
    // Originals are gone.
    assert!(!ai_dir(&c).join("backups/run-a").exists());
    assert!(report::run_casts(&c, "run-a").is_empty());
    // An archive is never overwritten.
    let mut again = runs_req(&["run-c"]);
    again.mode = PruneMode::Archive;
    again.archive_dir = Some(dest);
    assert!(execute(&c, &again).is_err());
    assert_eq!(
        report::run_casts(&c, "run-c").len(),
        2,
        "failed archive deletes nothing"
    );
    // And not inside what gets deleted.
    again.archive_dir = Some(recordings_dir(&c).join("arch"));
    assert!(execute(&c, &again).is_err());
}

#[test]
fn default_archive_goes_under_ai_dir() {
    let (_d, c, _a) = three_runs();
    let mut r = runs_req(&["run-a"]);
    r.mode = PruneMode::Archive;
    let p = plan(&c, &r).unwrap();
    assert!(
        p.archive_dir
            .unwrap()
            .starts_with(ai_dir(&c).join("archive"))
    );
    let out = execute(&c, &r).unwrap();
    let dir = out.archive_dir.unwrap();
    assert!(dir.starts_with(ai_dir(&c).join("archive")) && dir.join("manifest.json").exists());
    assert!(storage(&c).unwrap().archives_bytes > 0);
}

#[test]
fn active_runs_are_never_pruned() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    add_run(&c, &a, "run-old", true);
    add_run(&c, &a, "run-live", false);
    add_backup(&c, "run-live");

    for sel in [
        PruneSelection::All,
        PruneSelection::OlderThan { days: 0 },
        PruneSelection::Runs {
            ids: vec!["run-live".into()],
        },
    ] {
        let out = execute(&c, &req(sel, PruneMode::Delete)).unwrap();
        assert!(out.verify_ok);
        assert!(
            out.plan.runs.iter().all(|r| r.run_id != "run-live"),
            "{:?}",
            out.plan
        );
    }
    assert!(plan(&c, &runs_req(&["run-live"])).unwrap().skipped_active == vec!["run-live"]);
    assert_eq!(report::run_casts(&c, "run-live").len(), 2);
    assert!(ai_dir(&c).join("backups/run-live").exists());
    let live: Vec<_> = audit::read_entries(&c).unwrap();
    assert_eq!(live.len(), 3, "start and two calls of the live run");
}

#[test]
fn what_decides_the_state_of_an_active_run() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    let now = Utc::now();
    add_run(&c, &a, "young", false);
    add_run(&c, &a, "ended-in-db", false);
    add_run(&c, &a, "done", true);
    let store = AiStore::open(&c).unwrap();
    let rec = |id: &str| RunRecord {
        id: id.into(),
        client: "x".into(),
        on_behalf_of: "a".into(),
        policy: "default".into(),
        started: now.to_rfc3339(),
        ended: None,
        reason: None,
    };
    store.start_run(&rec("ended-in-db")).unwrap();
    store.end_run("ended-in-db", "stopped").unwrap();
    store.start_run(&rec("young")).unwrap();

    let lines = audit::read_lines(&c).unwrap();
    let p = prepare(
        &c,
        &req(PruneSelection::All, PruneMode::Delete),
        &lines,
        now,
    )
    .unwrap();
    let mut got: Vec<_> = p.plan.runs.iter().map(|r| r.run_id.as_str()).collect();
    got.sort_unstable();
    assert_eq!(got, ["done", "ended-in-db"]);
    assert_eq!(p.plan.skipped_active, vec!["young"]);

    // A day and a bit later the run that never ended counts as abandoned.
    let later = now + Duration::hours(25);
    let lines = audit::read_lines(&c).unwrap();
    let p = prepare(
        &c,
        &req(PruneSelection::All, PruneMode::Delete),
        &lines,
        later,
    )
    .unwrap();
    assert!(
        p.plan.skipped_active.is_empty(),
        "{:?}",
        p.plan.skipped_active
    );
    assert_eq!(p.plan.runs.len(), 3);
}

#[cfg(unix)]
#[test]
fn a_live_socket_keeps_an_old_run_active() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    add_run(&c, &a, "run-live", false);
    add_run(&c, &a, "run-dead", false);
    let ai = ai_dir(&c);
    control_client::ensure_runs_dir(&ai).unwrap();
    let _listener =
        std::os::unix::net::UnixListener::bind(control_client::socket_path(&ai, "run-live"))
            .unwrap();
    // A socket nobody listens on is a crashed run.
    drop(
        std::os::unix::net::UnixListener::bind(control_client::socket_path(&ai, "run-dead"))
            .unwrap(),
    );
    let later = Utc::now() + Duration::days(40);
    let lines = audit::read_lines(&c).unwrap();
    let p = prepare(
        &c,
        &req(PruneSelection::All, PruneMode::Delete),
        &lines,
        later,
    )
    .unwrap();
    assert_eq!(p.plan.skipped_active, vec!["run-live"]);
    assert_eq!(p.plan.runs.len(), 1);
    assert_eq!(p.plan.runs[0].run_id, "run-dead");
}

#[test]
fn older_than_uses_the_end_time() {
    let (_d, c, _a) = three_runs();
    let now = Utc::now() + Duration::days(40);
    let lines = audit::read_lines(&c).unwrap();
    let older = |days| {
        prepare(
            &c,
            &req(PruneSelection::OlderThan { days }, PruneMode::Delete),
            &lines,
            now,
        )
        .unwrap()
        .plan
        .runs
        .len()
    };
    assert_eq!(older(30), 3);
    assert_eq!(older(60), 0);
}

#[test]
fn run_selection_takes_prefixes_and_rejects_the_unknown() {
    let (_d, c, _a) = three_runs();
    assert!(plan(&c, &runs_req(&["nope"])).is_err());
    assert!(
        plan(&c, &runs_req(&["run-"]))
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    let p = plan(&c, &runs_req(&["run-b", "run-b", "run-"][..2])).unwrap();
    assert_eq!(p.runs.len(), 1);
    assert_eq!((p.entries, p.recordings), (4, 2));
    assert!(p.bytes > 0 && p.log_bytes_before > 0);
}

#[test]
fn plan_changes_nothing_and_what_narrows() {
    let (_d, c, _a) = three_runs();
    add_backup(&c, "run-a");
    let log = std::fs::read(audit::events_path(&c)).unwrap();
    let mut r = runs_req(&["run-a"]);
    r.what = PruneWhat {
        audit: false,
        recordings: false,
        backups: true,
        runs_db: false,
    };
    let p = plan(&c, &r).unwrap();
    assert_eq!((p.entries, p.recordings), (0, 0));
    assert_eq!(std::fs::read(audit::events_path(&c)).unwrap(), log);
    assert!(ai_dir(&c).join("backups/run-a").exists());

    execute(&c, &r).unwrap();
    assert!(!ai_dir(&c).join("backups/run-a").exists());
    assert_eq!(
        std::fs::read(audit::events_path(&c)).unwrap(),
        log,
        "log untouched"
    );
    assert_eq!(report::run_casts(&c, "run-a").len(), 2, "recordings kept");
}

#[test]
fn runs_db_rows_follow_the_run() {
    let (_d, c, _a) = three_runs();
    let store = AiStore::open(&c).unwrap();
    for id in ["run-a", "run-b"] {
        store
            .start_run(&RunRecord {
                id: id.into(),
                client: "x".into(),
                on_behalf_of: "a".into(),
                policy: "default".into(),
                started: Utc::now().to_rfc3339(),
                ended: None,
                reason: None,
            })
            .unwrap();
        store.end_run(id, "done").unwrap();
        store
            .note(id, crate::services::ai::store::CounterKind::Call)
            .unwrap();
    }
    let s = storage(&c).unwrap();
    assert_eq!(s.runs, 3);
    assert!(s.log_bytes > 0 && s.recordings_bytes > 0 && s.runs_db_bytes > 0);
    assert!(s.oldest.is_some());
    execute(&c, &runs_req(&["run-a"])).unwrap();
    assert!(store.run("run-a").unwrap().is_none());
    assert!(store.run("run-b").unwrap().is_some());
}

#[test]
fn a_run_id_that_extends_another_keeps_its_recordings() {
    let d = tempfile::tempdir().unwrap();
    let c = cfg(d.path());
    let a = LocalAudit::open(&c).unwrap();
    add_run(&c, &a, "run", true);
    add_run(&c, &a, "run-x", false);
    execute(&c, &runs_req(&["run"])).unwrap();
    assert_eq!(report::run_casts(&c, "run-x").len(), 2);
}

/// A writer that is waiting for the lock holds a handle on the old file. The prune replaces
/// the file, so the writer must notice and append to the new one.
#[test]
fn a_waiting_append_lands_in_the_new_file() {
    let (_d, c, a) = three_runs();
    let c = Arc::new(c);
    let mut log = LockedLog::lock(&c).unwrap();
    let writer = {
        let c = c.clone();
        std::thread::spawn(move || {
            let a = LocalAudit::open(&c).unwrap();
            a.append(&started("run-waiting")).unwrap()
        })
    };
    // Let it open the old file and block on the lock.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let stub: HashSet<u64> = (1..=8).collect();
    let done = log.rewrite(&stub).unwrap();
    assert_eq!((done.stubbed, done.dropped), (8, 8));
    drop(log);
    let seq = writer.join().unwrap();
    assert_eq!(seq, 13);
    let v = audit::verify(&c).unwrap();
    assert!(v.ok, "{v:?}");
    assert!(
        seqs(&c).contains(&13),
        "the waiting append is in the log at the path"
    );
    drop(a);
}

#[test]
fn concurrent_appends_survive_repeated_prunes() {
    const PER_WRITER: usize = 40;
    let d = tempfile::tempdir().unwrap();
    let c = Arc::new(cfg(d.path()));
    LocalAudit::open(&c)
        .unwrap()
        .append(&started("live"))
        .unwrap();
    let writers: Vec<_> = (0..3)
        .map(|t| {
            let c = c.clone();
            std::thread::spawn(move || {
                let a = LocalAudit::open(&c).unwrap();
                for n in 0..PER_WRITER {
                    a.append(&call("live", t * 1000 + n)).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(3));
                }
            })
        })
        .collect();
    let a = LocalAudit::open(&c).unwrap();
    let mut round = 0;
    while round < 5 || writers.iter().any(|h| !h.is_finished()) {
        let id = format!("done-{round}");
        a.append(&started(&id)).unwrap();
        a.append(&ended(&id)).unwrap();
        let out = execute(&c, &req(PruneSelection::All, PruneMode::Delete)).unwrap();
        assert!(out.verify_ok, "round {round}");
        assert_eq!(out.plan.skipped_active, vec!["live"]);
        round += 1;
    }
    for h in writers {
        h.join().unwrap();
    }
    let v = audit::verify(&c).unwrap();
    assert!(v.ok, "{v:?}");
    let live = audit::read_entries(&c)
        .unwrap()
        .into_iter()
        .filter(|e| event_run_id(&e.event) == "live")
        .count();
    assert_eq!(live, 3 * PER_WRITER + 1, "no append was lost to a prune");
}

#[test]
fn an_interrupted_prune_still_verifies_and_can_be_repeated() {
    let (_d, c, _a) = three_runs();
    // The anchor was written, the log not rewritten yet.
    let lines = audit::read_lines(&c).unwrap();
    let last = &lines[3].entry;
    let before = std::fs::read(audit::events_path(&c)).unwrap();
    execute(&c, &runs_req(&["run-a"])).unwrap();
    let anchor = std::fs::read(audit::anchor_path(&c)).unwrap();
    std::fs::write(audit::events_path(&c), &before).unwrap();
    assert_eq!(
        serde_json::from_slice::<audit::Anchor>(&anchor)
            .unwrap()
            .hash,
        last.hash
    );
    let v = audit::verify(&c).unwrap();
    assert!(v.ok, "{v:?}");
    // Redoing the prune finishes the job.
    execute(&c, &runs_req(&["run-a"])).unwrap();
    assert!(audit::verify(&c).unwrap().ok);
}
