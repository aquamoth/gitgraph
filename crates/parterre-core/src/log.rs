//! The log query: which commits the log window lists, and in what order.
//!
//! A [`LogQuery`] is the tips to walk back from plus the commits to leave out (with everything
//! they reach). [`LogQuery::run`] turns the in-memory [`Repo`] snapshot into the ordered list
//! without asking git again. It knows nothing about the revision graph.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::repo::{CommitIx, Repo};

/// What the log lists: the commits reachable from any of `tips` but from none of `exclude`,
/// like `git log <tips> ^<exclude>`.
///
/// Built with [`LogQuery::commit`] or [`LogQuery::range`]. It is `non_exhaustive` so that
/// future filters (date range, author, text) can become new fields, defaulting to "no
/// filter", without breaking callers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogQuery {
    /// Where the walk starts. Their ancestors are listed too.
    pub tips: Vec<CommitIx>,
    /// Commits left out together with all their ancestors.
    pub exclude: Vec<CommitIx>,
}

impl LogQuery {
    /// One node: the commit and all its ancestors, like `git log <commit>`.
    pub fn commit(commit: CommitIx) -> LogQuery {
        LogQuery {
            tips: vec![commit],
            exclude: Vec::new(),
        }
    }

    /// Two nodes, in selection order: `first..second`, the commits reachable from `second` but
    /// not from `first`, as TortoiseGit does. Unrelated histories give all of `second`'s
    /// history; `first` itself is never listed.
    ///
    /// Deliberate deviation from TortoiseGit (decided in #28): when `second` is an ancestor of
    /// `first`, where TortoiseGit shows an empty list, the two are swapped so the commits in
    /// between are shown. Diverged pairs keep the selection order.
    ///
    /// The range label (`first..second`) reads `exclude[0]..tips[0]` of the result, which
    /// reflects the swap.
    pub fn range(repo: &Repo, first: CommitIx, second: CommitIx) -> LogQuery {
        let (first, second) = if first != second && is_ancestor(repo, second, first) {
            (second, first)
        } else {
            (first, second)
        };
        LogQuery {
            tips: vec![second],
            exclude: vec![first],
        }
    }

    /// The commits the query selects, newest first by committer date, never a parent before
    /// any of its children: the order of `git log --date-order`.
    ///
    /// Runs in O(n log n) over the commits reachable from the tips, with two flat arrays the
    /// size of the snapshot.
    pub fn run(&self, repo: &Repo) -> Vec<CommitIx> {
        let n = repo.commits.len();
        let mut state = vec![State::Unseen; n];
        // Everything reachable from the exclusions is out.
        let mut stack: Vec<CommitIx> = Vec::new();
        for &c in &self.exclude {
            if state[c.ix()] == State::Unseen {
                state[c.ix()] = State::Excluded;
                stack.push(c);
            }
        }
        while let Some(c) = stack.pop() {
            for &p in &repo.commit(c).parents {
                if state[p.ix()] == State::Unseen {
                    state[p.ix()] = State::Excluded;
                    stack.push(p);
                }
            }
        }

        // Walk from the tips, counting for every selected commit its selected children.
        let mut children = vec![0u32; n];
        let mut selected = 0usize;
        for &c in &self.tips {
            if state[c.ix()] == State::Unseen {
                state[c.ix()] = State::Selected;
                selected += 1;
                stack.push(c);
            }
        }
        while let Some(c) = stack.pop() {
            for &p in &repo.commit(c).parents {
                match state[p.ix()] {
                    State::Excluded => {}
                    State::Selected => children[p.ix()] += 1,
                    State::Unseen => {
                        state[p.ix()] = State::Selected;
                        selected += 1;
                        children[p.ix()] += 1;
                        stack.push(p);
                    }
                }
            }
        }

        // git's topological sort by commit date (`sort_in_topological_order`): a commit becomes
        // ready once all its children are out, and the newest ready commit goes next. Ties go
        // to the commit that became ready first, as in git's priority queue.
        let mut ready = BinaryHeap::new();
        let mut seq = 0u32;
        let mut push = |ready: &mut BinaryHeap<Ready>, c: CommitIx| {
            ready.push(Ready {
                time: repo.commit(c).commit_time,
                seq: Reverse(seq),
                commit: c,
            });
            seq += 1;
        };
        // Initial candidates in snapshot order, which is git's own date order.
        let mut tips: Vec<CommitIx> = self
            .tips
            .iter()
            .copied()
            .filter(|c| state[c.ix()] == State::Selected && children[c.ix()] == 0)
            .collect();
        tips.sort_unstable();
        tips.dedup();
        for c in tips {
            push(&mut ready, c);
        }
        let mut out = Vec::with_capacity(selected);
        while let Some(Ready { commit, .. }) = ready.pop() {
            out.push(commit);
            for &p in &repo.commit(commit).parents {
                if state[p.ix()] == State::Selected {
                    children[p.ix()] -= 1;
                    if children[p.ix()] == 0 {
                        push(&mut ready, p);
                    }
                }
            }
        }
        debug_assert_eq!(out.len(), selected);
        out
    }
}

/// True if `ancestor` is reachable from `descendant` through parent links (a commit counts as
/// its own ancestor, as in `git merge-base --is-ancestor`).
pub fn is_ancestor(repo: &Repo, ancestor: CommitIx, descendant: CommitIx) -> bool {
    if ancestor == descendant {
        return true;
    }
    // Clocks skew, so walk everything reachable rather than pruning by commit date.
    let mut seen = vec![false; repo.commits.len()];
    let mut stack = vec![descendant];
    seen[descendant.ix()] = true;
    while let Some(c) = stack.pop() {
        for &p in &repo.commit(c).parents {
            if p == ancestor {
                return true;
            }
            if !seen[p.ix()] {
                seen[p.ix()] = true;
                stack.push(p);
            }
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Unseen,
    Excluded,
    Selected,
}

/// A commit whose children have all been listed. Max-heap order: newest first, then earliest
/// to become ready.
#[derive(PartialEq, Eq)]
struct Ready {
    time: i64,
    seq: Reverse<u32>,
    commit: CommitIx,
}

impl Ord for Ready {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.time, self.seq).cmp(&(other.time, other.seq))
    }
}

impl PartialOrd for Ready {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oid::Oid;
    use crate::repo::{Commit, Head};

    /// A repository from `(commit_time, parents)` pairs; commit `i` gets index `i`.
    fn repo(spec: &[(i64, &[u32])]) -> Repo {
        let commits = spec
            .iter()
            .enumerate()
            .map(|(i, &(time, parents))| Commit {
                oid: Oid::from_hex(&format!("{i:040x}")).unwrap(),
                parents: parents.iter().map(|&p| CommitIx(p)).collect(),
                truncated: false,
                empty_tree: false,
                author_name: String::new(),
                author_email: String::new(),
                author_time: time,
                author_date: String::new(),
                commit_time: time,
                subject: format!("c{i}"),
            })
            .collect();
        Repo::new(
            "/x".into(),
            commits,
            Vec::new(),
            Head::Detached(CommitIx(0)),
        )
    }

    fn ixs(v: &[u32]) -> Vec<CommitIx> {
        v.iter().map(|&i| CommitIx(i)).collect()
    }

    /// 0 is a merge of 1 (first parent) and 2; both come from 3.
    ///   0
    ///  / \
    /// 1   2
    ///  \ /
    ///   3
    fn diamond(t1: i64, t2: i64) -> Repo {
        repo(&[(10, &[1, 2]), (t1, &[3]), (t2, &[3]), (1, &[])])
    }

    #[test]
    fn one_node_lists_the_commit_and_its_ancestors_newest_first() {
        let r = diamond(5, 7);
        assert_eq!(LogQuery::commit(CommitIx(0)).run(&r), ixs(&[0, 2, 1, 3]));
        assert_eq!(LogQuery::commit(CommitIx(1)).run(&r), ixs(&[1, 3]));
    }

    #[test]
    fn a_parent_never_comes_before_its_child_despite_clock_skew() {
        // The parent 1 claims to be newer than its child 0 and than 2.
        let r = repo(&[(5, &[1]), (100, &[3]), (7, &[3]), (1, &[])]);
        let q = LogQuery {
            tips: ixs(&[0, 2]),
            exclude: Vec::new(),
        };
        assert_eq!(q.run(&r), ixs(&[2, 0, 1, 3]));
    }

    #[test]
    fn equal_times_keep_the_order_commits_became_ready() {
        let r = diamond(5, 5);
        // 1 (first parent) becomes ready before 2.
        assert_eq!(LogQuery::commit(CommitIx(0)).run(&r), ixs(&[0, 1, 2, 3]));
    }

    #[test]
    fn range_excludes_what_first_reaches() {
        // 0 - 1 - 2 - 3 (3 is the root)
        let r = repo(&[(4, &[1]), (3, &[2]), (2, &[3]), (1, &[])]);
        let q = LogQuery::range(&r, CommitIx(2), CommitIx(0));
        assert_eq!((q.tips.clone(), q.exclude.clone()), (ixs(&[0]), ixs(&[2])));
        assert_eq!(q.run(&r), ixs(&[0, 1]));
    }

    #[test]
    fn range_swaps_when_second_is_an_ancestor_of_first() {
        let r = repo(&[(4, &[1]), (3, &[2]), (2, &[3]), (1, &[])]);
        let q = LogQuery::range(&r, CommitIx(0), CommitIx(2));
        assert_eq!((q.tips.clone(), q.exclude.clone()), (ixs(&[0]), ixs(&[2])));
        assert_eq!(q.run(&r), ixs(&[0, 1]));
    }

    #[test]
    fn range_keeps_diverged_pairs_in_selection_order() {
        let r = diamond(5, 7);
        let q = LogQuery::range(&r, CommitIx(1), CommitIx(2));
        assert_eq!(q.run(&r), ixs(&[2]));
        let q = LogQuery::range(&r, CommitIx(2), CommitIx(1));
        assert_eq!(q.run(&r), ixs(&[1]));
    }

    #[test]
    fn range_of_unrelated_histories_is_all_of_seconds_history() {
        // Two roots: 0 - 1 and 2 - 3.
        let r = repo(&[(4, &[1]), (3, &[]), (2, &[3]), (1, &[])]);
        assert_eq!(
            LogQuery::range(&r, CommitIx(0), CommitIx(2)).run(&r),
            ixs(&[2, 3])
        );
    }

    #[test]
    fn range_of_a_node_with_itself_is_empty() {
        let r = diamond(5, 7);
        assert!(
            LogQuery::range(&r, CommitIx(1), CommitIx(1))
                .run(&r)
                .is_empty()
        );
    }

    #[test]
    fn ancestry() {
        let r = diamond(5, 7);
        assert!(is_ancestor(&r, CommitIx(3), CommitIx(0)));
        assert!(is_ancestor(&r, CommitIx(2), CommitIx(0)));
        assert!(!is_ancestor(&r, CommitIx(0), CommitIx(3)));
        assert!(!is_ancestor(&r, CommitIx(1), CommitIx(2)));
    }

    #[test]
    fn large_linear_history_is_fast_and_complete() {
        let n = 100_000u32;
        let parents: Vec<[u32; 1]> = (0..n).map(|i| [i + 1]).collect();
        let spec: Vec<(i64, &[u32])> = (0..n)
            .map(|i| {
                let p: &[u32] = if i + 1 < n { &parents[i as usize] } else { &[] };
                (i64::from(n - i), p)
            })
            .collect();
        let r = repo(&spec);
        let start = std::time::Instant::now();
        let out = LogQuery::commit(CommitIx(0)).run(&r);
        let took = start.elapsed();
        assert_eq!(out.len(), n as usize);
        assert!(out.windows(2).all(|w| w[0].0 + 1 == w[1].0));
        // Generous: debug builds on slow CI. Release takes a few milliseconds.
        assert!(took.as_secs() < 5, "took {took:?}");
    }
}
