//! In-memory snapshot of a repository's commit graph and refs.
//!
//! The snapshot holds every commit reachable from any ref (except notes), so that view options
//! such as "show remote branches" can be toggled without going back to git.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::oid::Oid;

/// Index of a commit in [`Repo::commits`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommitIx(pub u32);

impl CommitIx {
    pub fn ix(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Debug)]
pub struct Commit {
    pub oid: Oid,
    /// Parents present in the snapshot, in git order (first parent first).
    pub parents: Vec<CommitIx>,
    /// True if some parent is not in the snapshot (shallow clone boundary).
    pub truncated: bool,
    /// True if the commit's tree is the empty tree (e.g. `git commit --allow-empty` roots or
    /// svn-imported "create trunk" commits).
    pub empty_tree: bool,
    pub author_name: String,
    pub author_email: String,
    /// Author timestamp, seconds since the Unix epoch.
    pub author_time: i64,
    /// Committer timestamp, seconds since the Unix epoch.
    pub commit_time: i64,
    pub subject: String,
}

/// What kind of ref a label represents; drives label colour and visibility options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RefKind {
    /// `refs/heads/*`
    LocalBranch,
    /// `refs/remotes/*`
    RemoteBranch,
    /// `refs/tags/*`
    Tag,
    /// `refs/stash`
    Stash,
    /// A detached `HEAD` (only present when HEAD is not on a branch).
    DetachedHead,
    /// Anything else, e.g. `refs/pull/*` or tool-specific namespaces.
    Other,
}

#[derive(Clone, Debug)]
pub struct GitRef {
    /// Full name, e.g. `refs/remotes/origin/main`.
    pub full_name: String,
    /// Display name, e.g. `origin/main`.
    pub name: String,
    pub kind: RefKind,
    /// The commit the ref points at (tags are peeled).
    pub target: CommitIx,
    /// True for annotated tags (tag objects), false for lightweight tags and non-tags.
    pub annotated: bool,
    /// True if this is the branch `HEAD` points at.
    pub is_head: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// HEAD points at a branch; the commit is `None` for an unborn branch.
    Branch {
        name: String,
        target: Option<CommitIx>,
    },
    Detached(CommitIx),
}

#[derive(Clone, Debug)]
pub struct Repo {
    /// Working tree root, or the git dir for bare repositories.
    pub path: PathBuf,
    pub commits: Vec<Commit>,
    pub refs: Vec<GitRef>,
    pub head: Head,
    by_oid: HashMap<Oid, CommitIx>,
}

impl Repo {
    pub fn new(path: PathBuf, commits: Vec<Commit>, refs: Vec<GitRef>, head: Head) -> Repo {
        let by_oid = commits
            .iter()
            .enumerate()
            .map(|(i, c)| (c.oid, CommitIx(i as u32)))
            .collect();
        Repo {
            path,
            commits,
            refs,
            head,
            by_oid,
        }
    }

    pub fn commit(&self, ix: CommitIx) -> &Commit {
        &self.commits[ix.ix()]
    }

    pub fn lookup(&self, oid: &Oid) -> Option<CommitIx> {
        self.by_oid.get(oid).copied()
    }

    /// Finds commits whose hex id starts with `prefix` (case-insensitive).
    pub fn find_by_prefix(&self, prefix: &str) -> Vec<CommitIx> {
        let prefix = prefix.to_ascii_lowercase();
        if prefix.len() < 4 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Vec::new();
        }
        self.commits
            .iter()
            .enumerate()
            .filter(|(_, c)| c.oid.to_hex().starts_with(&prefix))
            .map(|(i, _)| CommitIx(i as u32))
            .collect()
    }

    pub fn head_commit(&self) -> Option<CommitIx> {
        match &self.head {
            Head::Branch { target, .. } => *target,
            Head::Detached(c) => Some(*c),
        }
    }

    /// Display name for the repository (directory name).
    pub fn display_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}
