//! Noticing that a repository's refs have changed, so the graph can reload by itself.
//!
//! Rather than asking git, this looks at the files git keeps refs in: `HEAD`, `packed-refs`,
//! the loose refs under `refs/` and a reftable's tables. git replaces such a file whenever it
//! moves a ref, which changes its modification time and that of its directory. Looking costs
//! a few hundred `stat` calls, cheap enough to repeat every second, and needs neither a git
//! process nor a file-watching library.
//!
//! A changed fingerprint means "maybe": `git pack-refs` or `git gc` rewrite the files without
//! moving any ref. Compare the reloaded snapshot with [`Repo::same_refs`](crate::Repo::same_refs).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::git::{Git, GitError};

/// Where one repository keeps its refs.
#[derive(Clone, Debug)]
pub struct RefStorage {
    /// Files, and directories to walk, whose metadata make up the fingerprint.
    paths: Vec<PathBuf>,
}

impl RefStorage {
    /// Finds the ref storage of the repository containing `dir`.
    pub fn locate(dir: &Path) -> Result<RefStorage, GitError> {
        let (git_dir, common) = Git::new(dir).git_dirs()?;
        let mut paths = vec![
            git_dir.join("HEAD"),
            common.join("packed-refs"),
            common.join("refs"),
            common.join("reftable"),
        ];
        // A linked worktree has refs of its own (`refs/bisect`, `refs/worktree`).
        if git_dir != common {
            paths.push(git_dir.join("refs"));
            paths.push(git_dir.join("reftable"));
        }
        Ok(RefStorage { paths })
    }

    /// A hash of the size and modification time of every file refs are kept in. It changes
    /// when a ref is created, moved or deleted.
    pub fn fingerprint(&self) -> u64 {
        let mut entries = Vec::new();
        for path in &self.paths {
            collect(path, &mut entries);
        }
        entries.sort_unstable();
        let mut hasher = DefaultHasher::new();
        entries.hash(&mut hasher);
        hasher.finish()
    }
}

/// One file or directory: its path, size and modification time in nanoseconds.
type Entry = (PathBuf, u64, u128);

/// Adds `path` and, for a directory, everything below it. Missing paths add nothing.
fn collect(path: &Path, out: &mut Vec<Entry>) {
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    out.push((path.to_owned(), meta.len(), modified));
    if meta.is_dir()
        && let Ok(dir) = std::fs::read_dir(path)
    {
        for entry in dir.flatten() {
            collect(&entry.path(), out);
        }
    }
}
