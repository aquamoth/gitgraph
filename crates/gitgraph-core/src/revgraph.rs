//! Reduction of the full commit graph to a revision graph: only "interesting" commits become
//! nodes, and the uninteresting history between them is collapsed into edges.
//!
//! The reduction mirrors TortoiseGit (see `docs/research/tortoisegit-revision-graph.md`):
//!
//! * [`Simplification::Decorated`] is TortoiseGit's default view, i.e. what
//!   `git log --simplify-by-decoration` keeps: commits with refs, roots, and merges that still
//!   join two independent kept lines. Parents are rewritten to their nearest kept ancestor and,
//!   as git's `simplify_merges` does, a parent that is an ancestor of another parent is dropped.
//! * [`Simplification::BranchesAndMerges`] is TortoiseGit's "Show branchings and merges": also
//!   keeps every merge, fork point and tip, and every commit whose only child is a merge.
//! * [`Simplification::AllCommits`] keeps everything.
//!
//! In the decorated mode, an undecorated root commit with an empty tree counts as unchanged
//! ("TREESAME") to git: it only appears as the end point of an edge, and merge parents leading
//! to it are dropped, so merges that join otherwise empty histories disappear.
//!
//! All modes share one pass over the commits in parents-first order that decides whether each
//! commit is kept and, for hidden commits, which kept commit represents them.

use serde::{Deserialize, Serialize};

use crate::repo::{CommitIx, RefKind, Repo};

/// How aggressively history is collapsed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Simplification {
    /// Commits with refs, roots and merges joining independent lines (TortoiseGit default).
    #[default]
    Decorated,
    /// Additionally every merge, fork point and merge source ("Show branchings and merges").
    BranchesAndMerges,
    /// Every commit is a node.
    AllCommits,
}

impl Simplification {
    pub const ALL: [Simplification; 3] = [
        Simplification::Decorated,
        Simplification::BranchesAndMerges,
        Simplification::AllCommits,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Simplification::Decorated => "Labelled commits",
            Simplification::BranchesAndMerges => "Branchings and merges",
            Simplification::AllCommits => "All commits",
        }
    }
}

/// Which parts of the repository the graph shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphOptions {
    pub simplification: Simplification,
    pub show_local_branches: bool,
    pub show_remote_branches: bool,
    pub show_tags: bool,
    /// TortoiseGit's "Show all tags": when off, a tag alone does not make a commit a node; tags
    /// still label commits that are shown for other reasons.
    pub tags_make_nodes: bool,
    pub show_stash: bool,
    /// Refs outside heads/remotes/tags/stash, e.g. `refs/pull/*` or tool checkpoints.
    pub show_other_refs: bool,
    /// Follow only first parents: merged-in side branches without refs of their own vanish.
    pub first_parent_only: bool,
    /// Show only the history of HEAD (TortoiseGit's "Current branch" filter). Other refs are
    /// still shown where they point into that history.
    pub current_branch_only: bool,
    /// Only refs whose name contains one of these comma-separated words (case-insensitive)
    /// start history; empty = all. Refs on commits shown anyway are always labelled.
    pub ref_filter: String,
}

impl Default for GraphOptions {
    fn default() -> Self {
        GraphOptions {
            simplification: Simplification::default(),
            show_local_branches: true,
            show_remote_branches: true,
            show_tags: true,
            tags_make_nodes: true,
            show_stash: true,
            show_other_refs: false,
            first_parent_only: false,
            current_branch_only: false,
            ref_filter: String::new(),
        }
    }
}

impl GraphOptions {
    /// True if a ref with this display name passes [`GraphOptions::ref_filter`].
    pub fn filter_matches(&self, name: &str) -> bool {
        let words: Vec<String> = self
            .ref_filter
            .split(',')
            .map(|w| w.trim().to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        let name = name.to_lowercase();
        words.is_empty() || words.iter().any(|w| name.contains(w.as_str()))
    }

    pub fn shows(&self, kind: RefKind) -> bool {
        match kind {
            RefKind::LocalBranch => self.show_local_branches,
            RefKind::RemoteBranch => self.show_remote_branches,
            RefKind::Tag => self.show_tags,
            RefKind::Stash => self.show_stash,
            RefKind::DetachedHead => true,
            RefKind::Other => self.show_other_refs,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RevNode {
    pub commit: CommitIx,
    /// Indices into [`Repo::refs`] of the visible refs on this commit, in display order.
    pub refs: Vec<usize>,
    /// True if HEAD points at this commit (directly or through its branch).
    pub is_head: bool,
    /// True if the node has more than one parent edge.
    pub is_merge: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevEdge {
    /// Node index of the newer end.
    pub child: u32,
    /// Node index of the older end.
    pub parent: u32,
    /// True if the edge follows the child's first-parent line.
    pub first_parent: bool,
    /// Number of commits collapsed into this edge.
    pub hidden: u32,
}

#[derive(Clone, Debug, Default)]
pub struct RevGraph {
    pub nodes: Vec<RevNode>,
    /// Edges grouped by child in node order, each child's first-parent edge first.
    pub edges: Vec<RevEdge>,
    /// Node index of every commit that is a node.
    node_of: Vec<Option<u32>>,
    /// For every visible commit, the node that stands for it (itself, or the nearest kept
    /// ancestor it was collapsed into).
    represented_by: Vec<Option<u32>>,
    /// Number of commits reachable from the visible refs.
    pub visible_commits: usize,
}

impl RevGraph {
    pub fn node_of(&self, commit: CommitIx) -> Option<u32> {
        self.node_of.get(commit.ix()).copied().flatten()
    }

    /// The node a commit is shown as: itself if it is a node, else the kept ancestor it was
    /// collapsed into. `None` if the commit is not visible at all.
    pub fn represented_by(&self, commit: CommitIx) -> Option<u32> {
        self.represented_by.get(commit.ix()).copied().flatten()
    }
}

const NO_REP: u32 = u32::MAX;

/// Builds the revision graph for `repo` under `options`.
pub fn build(repo: &Repo, options: &GraphOptions) -> RevGraph {
    let n = repo.commits.len();
    let head = repo.head_commit().map(CommitIx::ix);

    // Visible refs, which commits they decorate, and per-commit parent restrictions.
    let mut refs_on: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut decorated = vec![false; n];
    let mut first_parent_only = vec![options.first_parent_only; n];
    for (i, r) in repo.refs.iter().enumerate() {
        if !(options.shows(r.kind) || r.is_head) {
            continue;
        }
        let t = r.target.ix();
        refs_on[t].push(i);
        decorated[t] |= r.kind != RefKind::Tag || options.tags_make_nodes;
        // A stash commit's other parents are its index/untracked snapshots: noise.
        first_parent_only[t] |= r.kind == RefKind::Stash;
    }
    if let Some(h) = head {
        decorated[h] = true;
    }
    let parents_of = |c: usize| -> &[CommitIx] {
        let parents = &repo.commits[c].parents;
        if first_parent_only[c] {
            &parents[..parents.len().min(1)]
        } else {
            parents
        }
    };

    // Commits reachable from the refs that start history (visible and passing the filters).
    let starts_history = |i: usize| {
        let r = &repo.refs[i];
        r.is_head || (!options.current_branch_only && options.filter_matches(&r.name))
    };
    let mut visible = vec![false; n];
    let mut stack: Vec<usize> = (0..n)
        .filter(|&c| refs_on[c].iter().any(|&i| starts_history(i)))
        .collect();
    stack.extend(head);
    let mut visible_commits = 0;
    while let Some(c) = stack.pop() {
        if !visible[c] {
            visible[c] = true;
            visible_commits += 1;
            stack.extend(parents_of(c).iter().map(|p| p.ix()));
        }
    }

    // Child counts, merge children, and a parents-first order (reverse Kahn from the tips).
    let mut children = vec![0u32; n];
    let mut child_is_merge = vec![false; n];
    for c in (0..n).filter(|&c| visible[c]) {
        let ps = parents_of(c);
        for p in ps {
            children[p.ix()] += 1;
            child_is_merge[p.ix()] |= ps.len() > 1;
        }
    }
    let order = parents_first_order(n, &visible, &children, &parents_of);

    // Kept commits and representatives, in one parents-first pass.
    let mode = options.simplification;
    let treesame_root = |c: usize| {
        mode == Simplification::Decorated
            && parents_of(c).is_empty()
            && repo.commits[c].empty_tree
            && !decorated[c]
    };
    let mut rep = vec![NO_REP; n];
    let mut depth = vec![0u32; n];
    let mut kept_edges: Vec<Vec<(u32, u32, bool)>> = vec![Vec::new(); n];
    let mut ancestry = Ancestry::new(n);
    let mut candidates: Vec<(u32, u32, bool)> = Vec::new();
    for &c in &order {
        let ps = parents_of(c);
        candidates.clear();
        for (k, p) in ps.iter().enumerate() {
            let p = p.ix();
            let target = rep[p];
            if target != NO_REP && !candidates.iter().any(|&(t, _, _)| t == target) {
                candidates.push((target, depth[p], k == 0));
            }
        }
        if mode == Simplification::Decorated && candidates.len() > 1 {
            ancestry.drop_redundant(&mut candidates, &kept_edges);
            // git's mark_treesame_root_parents, keeping one if that would leave none.
            if candidates
                .iter()
                .any(|&(t, _, _)| !treesame_root(t as usize))
            {
                candidates.retain(|&(t, _, _)| !treesame_root(t as usize));
            } else {
                candidates.truncate(1);
            }
        }
        let kept = match mode {
            Simplification::AllCommits => true,
            Simplification::BranchesAndMerges => {
                decorated[c] || ps.len() != 1 || children[c] != 1 || child_is_merge[c]
            }
            Simplification::Decorated => decorated[c] || ps.is_empty() || candidates.len() > 1,
        };
        if kept || candidates.is_empty() {
            rep[c] = c as u32;
            depth[c] = 0;
            ancestry.add(c, &candidates);
            kept_edges[c] = candidates.clone();
        } else {
            let (target, d, _) = candidates[0];
            rep[c] = target;
            depth[c] = d + 1;
        }
    }

    // Treesame roots are only shown where an edge ends on them.
    let mut referenced = vec![false; n];
    for c in (0..n).filter(|&c| rep[c] == c as u32) {
        for &(t, _, _) in &kept_edges[c] {
            referenced[t as usize] = true;
        }
    }
    let is_node =
        |c: usize| visible[c] && rep[c] == c as u32 && (referenced[c] || !treesame_root(c));

    // Nodes in commit order (git log order: newest first, roughly).
    let mut node_of = vec![None; n];
    let mut nodes = Vec::new();
    for c in (0..n).filter(|&c| is_node(c)) {
        node_of[c] = Some(nodes.len() as u32);
        let mut refs = std::mem::take(&mut refs_on[c]);
        // HEAD first, then TortoiseGit's order: by full ref name (heads, remotes, stash, tags).
        refs.sort_by(|&a, &b| {
            let (ra, rb) = (&repo.refs[a], &repo.refs[b]);
            (rb.kind == RefKind::DetachedHead)
                .cmp(&(ra.kind == RefKind::DetachedHead))
                .then(ra.full_name.cmp(&rb.full_name))
        });
        nodes.push(RevNode {
            commit: CommitIx(c as u32),
            refs,
            is_head: Some(c) == head,
            is_merge: kept_edges[c].len() > 1,
        });
    }

    let mut edges = Vec::new();
    for (ni, node) in nodes.iter().enumerate() {
        let mut out: Vec<RevEdge> = kept_edges[node.commit.ix()]
            .iter()
            .map(|&(target, hidden, first_parent)| RevEdge {
                child: ni as u32,
                parent: node_of[target as usize].expect("representatives are nodes"),
                first_parent,
                hidden,
            })
            .collect();
        out.sort_by_key(|e| !e.first_parent);
        edges.extend(out);
    }

    let represented_by = (0..n)
        .map(|c| {
            (rep[c] != NO_REP)
                .then(|| node_of[rep[c] as usize])
                .flatten()
        })
        .collect();

    RevGraph {
        nodes,
        edges,
        node_of,
        represented_by,
        visible_commits,
    }
}

/// Visible commits ordered so that every commit comes after all of its parents.
fn parents_first_order<'a>(
    n: usize,
    visible: &[bool],
    children: &[u32],
    parents_of: &impl Fn(usize) -> &'a [CommitIx],
) -> Vec<usize> {
    let mut pending = children.to_vec();
    let mut stack: Vec<usize> = (0..n)
        .rev()
        .filter(|&c| visible[c] && pending[c] == 0)
        .collect();
    let mut order = Vec::with_capacity(n);
    while let Some(c) = stack.pop() {
        order.push(c);
        for p in parents_of(c) {
            let p = p.ix();
            pending[p] -= 1;
            if pending[p] == 0 {
                stack.push(p);
            }
        }
    }
    order.reverse();
    order
}

/// Ancestry queries among kept commits, used to drop redundant merge parents.
struct Ancestry {
    /// Longest distance to a root over kept edges; an ancestor always has a smaller value.
    generation: Vec<u32>,
    stamp: Vec<u32>,
    search: u32,
}

impl Ancestry {
    fn new(n: usize) -> Ancestry {
        Ancestry {
            generation: vec![0; n],
            stamp: vec![0; n],
            search: 0,
        }
    }

    fn add(&mut self, c: usize, parents: &[(u32, u32, bool)]) {
        self.generation[c] = parents
            .iter()
            .map(|&(p, _, _)| self.generation[p as usize] + 1)
            .max()
            .unwrap_or(0);
    }

    /// Removes candidates that are ancestors of other candidates (git's `simplify_merges`).
    fn drop_redundant(
        &mut self,
        candidates: &mut Vec<(u32, u32, bool)>,
        edges: &[Vec<(u32, u32, bool)>],
    ) {
        let targets: Vec<u32> = candidates.iter().map(|c| c.0).collect();
        let redundant: Vec<bool> = targets
            .iter()
            .map(|&a| {
                targets
                    .iter()
                    .any(|&b| a != b && self.is_ancestor(a, b, edges))
            })
            .collect();
        let mut i = 0;
        candidates.retain(|_| {
            i += 1;
            !redundant[i - 1]
        });
    }

    /// True if kept commit `a` is a proper ancestor of kept commit `b`.
    fn is_ancestor(&mut self, a: u32, b: u32, edges: &[Vec<(u32, u32, bool)>]) -> bool {
        let floor = self.generation[a as usize];
        self.search += 1;
        let mut stack = vec![b];
        while let Some(x) = stack.pop() {
            for &(p, _, _) in &edges[x as usize] {
                if p == a {
                    return true;
                }
                let pi = p as usize;
                if self.generation[pi] > floor && self.stamp[pi] != self.search {
                    self.stamp[pi] = self.search;
                    stack.push(p);
                }
            }
        }
        false
    }
}
