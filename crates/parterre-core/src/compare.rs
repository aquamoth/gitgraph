//! Comparing two commits, TortoiseGit's "Compare revisions": which commit goes on which side,
//! and the files that differ between them.
//!
//! The file list compares the two trees, as TortoiseGit does by default (`git diff A B`).
//! [`Comparison::since_ancestor`] instead compares the right side with where the two histories
//! forked (`git diff A...B`), what a pull request shows; TortoiseGit has it as "diff against
//! the common ancestor" in its Diff Options.
//!
//! Either side can be the working tree (TortoiseGit's "Compare with working tree"): its files
//! as they are on disk, staged or not, as `git diff <commit>` compares them.

use crate::changed_files::ChangedFile;
use crate::file_diff::Rev;
use crate::git::{Git, GitError};
use crate::log::is_ancestor;
use crate::oid::Oid;
use crate::repo::{CommitIx, Repo};

/// Two versions to compare: `old` on the left, `new` on the right. Either can be the working
/// tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Comparison {
    pub old: Rev,
    pub new: Rev,
    /// Compare `new` with the common ancestor of the two rather than with `old`. The working
    /// tree counts as `HEAD` in finding it, as with `git diff --merge-base`.
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
            old: Rev::Commit(repo.commit(old).oid),
            new: Rev::Commit(repo.commit(new).oid),
            since_ancestor,
        }
    }

    /// A commit against the working tree, which goes on the right, as in TortoiseGit.
    pub fn with_working_tree(commit: Oid, since_ancestor: bool) -> Comparison {
        Comparison {
            old: Rev::Commit(commit),
            new: Rev::WorkingTree,
            since_ancestor,
        }
    }

    /// The same two versions the other way round.
    pub fn swapped(self) -> Comparison {
        Comparison {
            old: self.new,
            new: self.old,
            ..self
        }
    }

    /// True if a side is the working tree, whose files can change at any time.
    pub fn reads_working_tree(&self) -> bool {
        self.old == Rev::WorkingTree || self.new == Rev::WorkingTree
    }

    /// Asks git for the files that differ.
    pub fn run(&self, git: &Git) -> Result<Compared, GitError> {
        let base = if self.since_ancestor {
            let name = |rev: Rev| rev.commit().map_or("HEAD".to_owned(), |o| o.to_hex());
            git.merge_base(&name(self.old), &name(self.new))?
                .map(Rev::Commit)
        } else {
            Some(self.old)
        };
        let files = match (base, self.new) {
            (None, _) | (Some(Rev::WorkingTree), Rev::WorkingTree) => Vec::new(),
            (Some(Rev::Commit(a)), Rev::Commit(b)) => git.changed_between(&a, &b)?,
            (Some(Rev::Commit(a)), Rev::WorkingTree) => git.changed_in_working_tree(&a, false)?,
            (Some(Rev::WorkingTree), Rev::Commit(b)) => git.changed_in_working_tree(&b, true)?,
        };
        Ok(Compared { base, files })
    }
}

/// What [`Comparison::run`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compared {
    /// Where the left side of the file diffs is read from: `old`, or the common ancestor.
    /// `None` when asked for the common ancestor of unrelated histories; `files` is then empty.
    pub base: Option<Rev>,
    pub files: Vec<ChangedFile>,
}
