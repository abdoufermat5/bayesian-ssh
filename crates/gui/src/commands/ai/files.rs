//! Files the AI view wrote in this session, and the only ones the webview may
//! ask the app to open or reveal.
//!
//! `ai_open_path` / `ai_reveal_path` take a path from the webview. They accept
//! it only if the app itself wrote that file (a report export or the temporary
//! HTML view) since it started, so a compromised page can't use them to launch
//! arbitrary files or programs.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Upper bound so a long session can't grow the set without limit.
const MAX_FILES: usize = 256;

#[derive(Default)]
pub struct ReportFiles {
    allowed: Mutex<HashSet<PathBuf>>,
}

impl ReportFiles {
    /// Remember a file the app just wrote. Stored canonical, so a later
    /// symlink swap or `..` segment can't change what matches.
    pub fn allow(&self, path: &Path) -> Result<(), String> {
        let canonical = path
            .canonicalize()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut set = self.allowed.lock().unwrap_or_else(|e| e.into_inner());
        if set.len() >= MAX_FILES && !set.contains(&canonical) {
            set.clear();
        }
        set.insert(canonical);
        Ok(())
    }

    /// The canonical path if the app wrote it this session and it still
    /// resolves to the same file; an error otherwise.
    pub fn check(&self, path: &str) -> Result<PathBuf, String> {
        let canonical = Path::new(path)
            .canonicalize()
            .map_err(|_| "That file is not available any more".to_string())?;
        let set = self.allowed.lock().unwrap_or_else(|e| e.into_inner());
        if set.contains(&canonical) {
            Ok(canonical)
        } else {
            Err("Only report files saved by this app can be opened".to_string())
        }
    }
}

/// `<cache dir>/ai-reports`, created private (0700 on Unix).
pub fn temp_dir(base: &Path) -> Result<PathBuf, String> {
    let dir = base.join("ai-reports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(dir)
}

/// Write `bytes` to `path`, replacing it, readable by the owner only.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    // Replace rather than reuse: a leftover file may have other permissions
    // or be a symlink.
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// A file name made only of ASCII letters, digits and `-`, from a run id.
pub fn safe_stem(run_id: &str) -> String {
    let s: String = run_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(48)
        .collect();
    if s.is_empty() { "run".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allowed_files_pass() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.html");
        let b = dir.path().join("b.html");
        std::fs::write(&a, "x").unwrap();
        std::fs::write(&b, "x").unwrap();
        let files = ReportFiles::default();
        files.allow(&a).unwrap();
        assert!(files.check(a.to_str().unwrap()).is_ok());
        assert!(files.check(b.to_str().unwrap()).is_err());
        assert!(files.check("/etc/passwd").is_err());
        assert!(files.check("/does/not/exist").is_err());
    }

    #[test]
    fn dotdot_and_symlinks_resolve_to_the_allowed_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.html");
        std::fs::write(&a, "x").unwrap();
        let files = ReportFiles::default();
        files.allow(&a).unwrap();
        let sneaky = dir.path().join("sub/../a.html");
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(files.check(sneaky.to_str().unwrap()).is_ok());
        // A symlink to another file is not allowed just because its target dir is.
        #[cfg(unix)]
        {
            let other = dir.path().join("secret");
            std::fs::write(&other, "s").unwrap();
            let link = dir.path().join("link.html");
            std::os::unix::fs::symlink(&other, &link).unwrap();
            assert!(files.check(link.to_str().unwrap()).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn private_files_are_0600_and_dir_0700() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let dir = temp_dir(base.path()).unwrap();
        let p = dir.join("r.html");
        write_private(&p, b"one").unwrap();
        write_private(&p, b"two").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"two");
        let mode = |q: &Path| std::fs::metadata(q).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&p), 0o600);
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn stem_is_filename_safe() {
        assert_eq!(safe_stem("run-7f3a/../x"), "run-7f3ax");
        assert_eq!(safe_stem("///"), "run");
    }
}
