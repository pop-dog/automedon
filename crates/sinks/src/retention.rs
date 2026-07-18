//! Run-directory retention: a count cap that keeps the most recent `keep` Runs
//! and removes the rest, oldest first. UUIDv7 names sort chronologically, so the
//! lexicographically smallest entries are the oldest.

use std::path::Path;

/// Prune `runs_dir` to its newest `keep` entries, deleting older Run
/// directories. A missing `runs_dir` is a no-op (nothing has been logged yet).
/// Best-effort: an entry that fails to delete is left in place.
pub fn prune(runs_dir: &Path, keep: usize) -> std::io::Result<()> {
    let mut names: Vec<String> = match std::fs::read_dir(runs_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };

    if names.len() <= keep {
        return Ok(());
    }

    // Ascending order makes the oldest (smallest UUIDv7) entries the prefix to drop.
    names.sort();
    let drop_count = names.len() - keep;
    for name in &names[..drop_count] {
        let _ = std::fs::remove_dir_all(runs_dir.join(name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::prune;
    use std::path::{Path, PathBuf};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("ao-prune-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn run_dirs(parent: &Path, names: &[&str]) {
        for name in names {
            std::fs::create_dir_all(parent.join(name)).unwrap();
        }
    }

    fn remaining(parent: &Path) -> Vec<String> {
        let mut got: Vec<String> = std::fs::read_dir(parent)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        got.sort();
        got
    }

    #[test]
    fn keeps_newest_and_removes_oldest() {
        let tmp = TempDir::new();
        run_dirs(&tmp.0, &["a", "b", "c", "d"]);
        prune(&tmp.0, 2).unwrap();
        assert_eq!(remaining(&tmp.0), vec!["c", "d"]);
    }

    #[test]
    fn under_cap_is_left_untouched() {
        let tmp = TempDir::new();
        run_dirs(&tmp.0, &["a", "b"]);
        prune(&tmp.0, 5).unwrap();
        assert_eq!(remaining(&tmp.0), vec!["a", "b"]);
    }

    #[test]
    fn missing_runs_dir_is_a_no_op() {
        let tmp = TempDir::new();
        let absent = tmp.0.join("never-created");
        assert!(prune(&absent, 3).is_ok());
    }
}
