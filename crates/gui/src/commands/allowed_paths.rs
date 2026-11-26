//! Session-scoped allowlist of local filesystem paths picked by the user
//! through native OS dialogs (or discovered SSH keys).
//!
//! Commands that read or write local paths on behalf of the webview validate
//! incoming paths against this state so a compromised webview cannot read or
//! overwrite arbitrary files on the host.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Maximum number of entries retained per category so long sessions stay bounded.
const MAX_ENTRIES: usize = 512;

#[derive(Default)]
struct BoundedPathSet {
    set: HashSet<PathBuf>,
    order: VecDeque<PathBuf>,
}

impl BoundedPathSet {
    fn insert(&mut self, path: PathBuf) {
        if self.set.contains(&path) {
            return;
        }
        while self.set.len() >= MAX_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            } else {
                break;
            }
        }
        self.set.insert(path.clone());
        self.order.push_back(path);
    }

    fn contains(&self, path: &Path) -> bool {
        self.set.contains(path)
    }

    fn contains_ancestor_of(&self, path: &Path) -> bool {
        self.set.iter().any(|dir| path.starts_with(dir))
    }
}

/// Normalize a path that may not exist yet (e.g. a save-dialog destination) by
/// canonicalizing its nearest existing ancestor and re-appending the remaining
/// normal path components. Rejects `..` segments or symlinks at the leaf.
fn normalize_path(path: &Path) -> Result<PathBuf, String> {
    if let Ok(canon) = path.canonicalize() {
        return Ok(canon);
    }

    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path;
    loop {
        if let Ok(canon) = cursor.canonicalize() {
            let mut resolved = canon;
            for part in suffix.into_iter().rev() {
                resolved.push(part);
            }
            return Ok(resolved);
        }
        let Some(name) = cursor.file_name() else {
            return Err(format!("Path cannot be resolved: {}", path.display()));
        };
        if name == "." || name == ".." {
            return Err(format!("Invalid path component in {}", path.display()));
        }
        suffix.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            return Err(format!("Path has no existing parent: {}", path.display()));
        };
        if parent.as_os_str().is_empty() {
            return Err(format!("Path must be absolute: {}", path.display()));
        }
        cursor = parent;
    }
}

/// Tauri-managed state tracking local files and directories approved via native
/// OS dialogs during this session.
#[derive(Default)]
pub struct AllowedPaths {
    read_files: Mutex<BoundedPathSet>,
    write_files: Mutex<BoundedPathSet>,
    dirs: Mutex<BoundedPathSet>,
}

impl AllowedPaths {
    /// Record an existing (or to-be-resolved) file selected for reading (upload,
    /// backup import, SSH key selection).
    pub fn allow_read_file(&self, path: &Path) {
        if let Ok(norm) = normalize_path(path) {
            self.read_files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(norm);
        }
    }

    /// Record a destination file selected via a native Save dialog.
    pub fn allow_write_file(&self, path: &Path) {
        if let Ok(norm) = normalize_path(path) {
            self.write_files
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(norm);
        }
    }

    /// Record a directory selected via a native Folder dialog (upload folder,
    /// archive destination).
    pub fn allow_dir(&self, path: &Path) {
        if let Ok(norm) = normalize_path(path) {
            self.dirs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(norm);
        }
    }

    /// Check that `path` was previously approved as a read file.
    pub fn check_read_file(&self, path: &Path) -> Result<PathBuf, String> {
        let canon = path
            .canonicalize()
            .map_err(|_| format!("File not found: {}", path.display()))?;
        let set = self.read_files.lock().unwrap_or_else(|e| e.into_inner());
        if set.contains(&canon) {
            Ok(canon)
        } else {
            Err("Access denied: file must be selected through the file picker.".to_string())
        }
    }

    /// Check that `path` was previously approved via a native Save dialog.
    pub fn check_write_file(&self, path: &Path) -> Result<PathBuf, String> {
        if path
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink())
        {
            return Err("Access denied: refusing to write through a symbolic link.".to_string());
        }
        let norm = normalize_path(path)?;
        let set = self.write_files.lock().unwrap_or_else(|e| e.into_inner());
        if set.contains(&norm) {
            Ok(norm)
        } else {
            Err(
                "Access denied: destination path must be chosen through the save dialog."
                    .to_string(),
            )
        }
    }

    /// Check that `path` is either an approved upload file or a path inside an
    /// approved upload folder.
    pub fn check_upload_path(&self, path: &Path) -> Result<PathBuf, String> {
        let canon = path
            .canonicalize()
            .map_err(|_| format!("Local path not found: {}", path.display()))?;
        {
            let files = self.read_files.lock().unwrap_or_else(|e| e.into_inner());
            if files.contains(&canon) {
                return Ok(canon);
            }
        }
        let dirs = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
        if dirs.contains_ancestor_of(&canon) {
            Ok(canon)
        } else {
            Err("Access denied: upload path must be selected through the file picker.".to_string())
        }
    }

    /// Check that `path` is an approved directory (or a subdirectory inside one).
    pub fn check_dir(&self, path: &Path) -> Result<PathBuf, String> {
        let norm = normalize_path(path)?;
        let dirs = self.dirs.lock().unwrap_or_else(|e| e.into_inner());
        if dirs.contains_ancestor_of(&norm) {
            Ok(norm)
        } else {
            Err("Access denied: directory must be selected through the folder picker.".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_file_allowlist_handles_nonexistent_targets_and_rejects_others() {
        let dir = tempfile::tempdir().unwrap();
        let allowed_out = dir.path().join("backup.json");
        let other_out = dir.path().join("other.json");

        let state = AllowedPaths::default();
        assert!(state.check_write_file(&allowed_out).is_err());

        state.allow_write_file(&allowed_out);
        assert!(state.check_write_file(&allowed_out).is_ok());
        assert!(state.check_write_file(&other_out).is_err());
    }

    #[test]
    fn upload_path_allows_picked_file_and_files_inside_picked_folder() {
        let dir = tempfile::tempdir().unwrap();
        let picked_file = dir.path().join("single.txt");
        let folder = dir.path().join("folder");
        let nested = folder.join("sub/nested.txt");
        let outside = dir.path().join("outside.txt");

        std::fs::write(&picked_file, "a").unwrap();
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, "b").unwrap();
        std::fs::write(&outside, "c").unwrap();

        let state = AllowedPaths::default();
        state.allow_read_file(&picked_file);
        state.allow_dir(&folder);

        assert!(state.check_upload_path(&picked_file).is_ok());
        assert!(state.check_upload_path(&folder).is_ok());
        assert!(state.check_upload_path(&nested).is_ok());
        assert!(state.check_upload_path(&outside).is_err());
    }

    #[test]
    fn dir_allowlist_allows_picked_dir_and_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        let archive_root = dir.path().join("archives");
        let other_dir = dir.path().join("elsewhere");
        std::fs::create_dir_all(&archive_root).unwrap();
        std::fs::create_dir_all(&other_dir).unwrap();

        let state = AllowedPaths::default();
        state.allow_dir(&archive_root);

        assert!(state.check_dir(&archive_root).is_ok());
        assert!(state.check_dir(&archive_root.join("2026-11-26")).is_ok());
        assert!(state.check_dir(&other_dir).is_err());
    }

    #[test]
    fn bounded_set_evicts_oldest_entries() {
        let dir = tempfile::tempdir().unwrap();
        let state = AllowedPaths::default();
        let first = dir.path().join("file_0.txt");
        state.allow_write_file(&first);
        for i in 1..=MAX_ENTRIES {
            let p = dir.path().join(format!("file_{i}.txt"));
            state.allow_write_file(&p);
        }
        assert!(state.check_write_file(&first).is_err());
        assert!(
            state
                .check_write_file(&dir.path().join(format!("file_{MAX_ENTRIES}.txt")))
                .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_file_rejects_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("secret.txt");
        let link = dir.path().join("link.txt");
        std::fs::write(&target, "secret").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let state = AllowedPaths::default();
        state.allow_write_file(&link);
        assert!(state.check_write_file(&link).is_err());
    }
}
