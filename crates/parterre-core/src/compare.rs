//! Comparing two commits, TortoiseGit's "Compare revisions": which commit goes on which side,
//! and the files that differ between them.
//!
//! The file list compares the two trees, as TortoiseGit does by default (`git diff A B`).
//! [`Comparison::since_ancestor`] instead compares the right side with where the two histories
//! forked (`git diff A...B`), what a pull request shows; TortoiseGit has it as "diff against
//! the common ancestor" in its Diff Options.

use crate::changed_files::ChangedFile;
use crate::git::{Git, GitError};
use crate::log::is_ancestor;
use crate::oid::Oid;
use crate::repo::{CommitIx, Repo};

/// Two commits to compare: `old` on the left, `new` on the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Comparison {
    pub old: Oid,
    pub new: Oid,
    /// Compare `new` with the common ancestor of the two rather than with `old`.
    pub since_ancestor: bool,
}

impl Comparison {
    /// `first` and `second` in the order they were picked, `first` on the left, except that an
    /// ancestor always goes on the left, as with the range of Show log (#28).
    pub fn of(repo: &Repo, first: CommitIx, second: CommitIx, since_ancestor: bool) -> Comparison {
        let (old, new) = if first != second && is_ancestor(repo, second, first) {
            (second, first)
        } else {
            (first, second)
        };
        Comparison {
            old: repo.commit(old).oid,
            new: repo.commit(new).oid,
            since_ancestor,
        }
    }

    /// The same two commits the other way round.
    pub fn swapped(self) -> Comparison {
        Comparison {
            old: self.new,
            new: self.old,
            ..self
        }
    }

    /// Asks git for the files that differ.
    pub fn run(&self, git: &Git) -> Result<Compared, GitError> {
        let base = if self.since_ancestor {
            git.merge_base(&self.old, &self.new)?
        } else {
            Some(self.old)
        };
        let files = match base {
            Some(base) => git.changed_between(&base, &self.new)?,
            None => Vec::new(),
        };
        Ok(Compared { base, files })
    }
}

/// What [`Comparison::run`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compared {
    /// The commit the left side of the file diffs is taken from: `old`, or the common ancestor.
    /// `None` when asked for the common ancestor of unrelated histories; `files` is then empty.
    pub base: Option<Oid>,
    pub files: Vec<ChangedFile>,
}
