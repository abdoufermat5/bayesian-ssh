use crate::config::AppConfig;
use crate::database::Database;
use crate::models::SessionContext;
use chrono::{Datelike, Duration, Local, Timelike, Utc};
use std::path::{Path, PathBuf};

/// Lookback window for `prev_connection_id` (30 minutes).
const PREV_SESSION_LOOKBACK_MINUTES: i64 = 30;

/// Capture launch context for a CLI session.
///
/// Only records `cwd` and `git_root` when `cfg.prediction.capture_cwd` is
/// enabled and the paths are valid UTF-8.
pub fn capture(db: &Database, cfg: &AppConfig) -> SessionContext {
    let mut ctx = capture_without_cwd(db);
    if cfg.prediction.capture_cwd {
        if let Ok(cwd_path) = std::env::current_dir() {
            if let Some(cwd_str) = cwd_path.to_str() {
                ctx.cwd = Some(cwd_str.to_owned());
            }
            if let Some(git_root) = find_git_root(&cwd_path) {
                if let Some(root_str) = git_root.to_str() {
                    ctx.git_root = Some(root_str.to_owned());
                }
            }
        }
    }
    ctx
}

/// Capture launch context without a working directory (used by the desktop GUI).
pub fn capture_without_cwd(db: &Database) -> SessionContext {
    let now_local = Local::now();
    let cutoff = Utc::now() - Duration::minutes(PREV_SESSION_LOOKBACK_MINUTES);
    let prev_connection_id = db.last_connection_since(cutoff).ok().flatten();

    SessionContext {
        cwd: None,
        git_root: None,
        prev_connection_id,
        local_hour: Some(now_local.hour() as u8),
        local_weekday: Some(now_local.weekday().num_days_from_monday() as u8),
    }
}

/// Walk up from `start` looking for a `.git` entry (either a directory for a
/// normal repository or a file for a git worktree / submodule).
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if ancestor.join(".git").exists() {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Replace a leading `$HOME` prefix with `~` for display.
pub fn abbreviate_home(path: &str) -> String {
    if let Some(home) = dirs::home_dir().and_then(|h| h.to_str().map(str::to_owned)) {
        let home = home.trim_end_matches('/');
        if !home.is_empty() {
            if path == home {
                return "~".to_string();
            }
            if let Some(rest) = path.strip_prefix(home) {
                if rest.starts_with('/') {
                    return format!("~{rest}");
                }
            }
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::find_git_root;
    use tempfile::tempdir;

    #[test]
    fn predictive_find_git_root() {
        // 1. `.git` directory found from a nested directory
        let repo_dir = tempdir().unwrap();
        std::fs::create_dir(repo_dir.path().join(".git")).unwrap();
        let nested = repo_dir.path().join("crates/cli/src");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_git_root(&nested), Some(repo_dir.path().to_path_buf()));

        // 2. `.git` file (worktree/submodule) found from a nested directory
        let wt_dir = tempdir().unwrap();
        std::fs::write(
            wt_dir.path().join(".git"),
            "gitdir: /tmp/main/.git/worktrees/wt\n",
        )
        .unwrap();
        let wt_nested = wt_dir.path().join("sub/dir");
        std::fs::create_dir_all(&wt_nested).unwrap();
        assert_eq!(find_git_root(&wt_nested), Some(wt_dir.path().to_path_buf()));

        // 3. No `.git` ancestor -> None
        let plain_dir = tempdir().unwrap();
        let plain_nested = plain_dir.path().join("a/b");
        std::fs::create_dir_all(&plain_nested).unwrap();
        assert_eq!(find_git_root(&plain_nested), None);
    }
}
