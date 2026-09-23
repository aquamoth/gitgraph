mod common;

use common::TestRepo;
use gitgraph_core::revgraph::{self, GraphOptions, RevGraph, Simplification};
use gitgraph_core::{Head, RefKind, Repo};

/// Subjects of the graph's nodes, sorted, for order-independent comparison.
fn node_subjects(repo: &Repo, g: &RevGraph) -> Vec<String> {
    let mut s: Vec<String> = g
        .nodes
        .iter()
        .map(|n| repo.commit(n.commit).subject.clone())
        .collect();
    s.sort();
    s
}

/// Edges as (child subject, parent subject, first_parent, hidden), sorted.
fn edge_list(repo: &Repo, g: &RevGraph) -> Vec<(String, String, bool, u32)> {
    let subject = |node: u32| repo.commit(g.nodes[node as usize].commit).subject.clone();
    let mut e: Vec<_> = g
        .edges
        .iter()
        .map(|e| {
            (
                subject(e.child),
                subject(e.parent),
                e.first_parent,
                e.hidden,
            )
        })
        .collect();
    e.sort();
    e
}

fn with_mode(simplification: Simplification) -> GraphOptions {
    GraphOptions {
        simplification,
        ..GraphOptions::default()
    }
}

fn edge(c: &str, p: &str, first: bool, hidden: u32) -> (String, String, bool, u32) {
    (c.into(), p.into(), first, hidden)
}

/// main: A - B - C ------- M
///            \           /
/// feature:    D ------- E
fn feature_merge() -> TestRepo {
    let mut r = TestRepo::new();
    r.commit("A");
    r.commit("B");
    r.branch("feature");
    r.commit("D");
    r.commit("E");
    r.checkout("main");
    r.commit("C");
    r.merge("feature", "M");
    r
}

#[test]
fn loads_commits_refs_and_head() {
    let r = feature_merge();
    r.git(&["tag", "-a", "-m", "annotated", "v1", "HEAD~1"]);
    let repo = r.load();
    assert_eq!(repo.commits.len(), 6);
    assert!(
        matches!(&repo.head, Head::Branch { name, target: Some(_) } if name == "refs/heads/main")
    );
    let names: Vec<(&str, RefKind, bool)> = repo
        .refs
        .iter()
        .map(|r| (r.name.as_str(), r.kind, r.annotated))
        .collect();
    assert!(names.contains(&("main", RefKind::LocalBranch, false)));
    assert!(names.contains(&("feature", RefKind::LocalBranch, false)));
    assert!(names.contains(&("v1", RefKind::Tag, true)));
    let merge = repo.head_commit().unwrap();
    assert_eq!(repo.commit(merge).parents.len(), 2);
    let tag = repo.refs.iter().find(|r| r.name == "v1").unwrap();
    assert_eq!(
        repo.commit(tag.target).subject,
        "C",
        "annotated tags are peeled"
    );
}

#[test]
fn decorated_mode_matches_simplify_by_decoration() {
    let r = feature_merge();
    let repo = r.load();
    let g = revgraph::build(&repo, &with_mode(Simplification::Decorated));
    // B (fork point) and C are undecorated; M's first parent rewrites to A, which is an
    // ancestor of E, so git drops it and M hangs off E only.
    assert_eq!(node_subjects(&repo, &g), ["A", "E", "M"]);
    assert_eq!(
        edge_list(&repo, &g),
        [edge("E", "A", true, 2), edge("M", "E", false, 0)]
    );
}

#[test]
fn decorated_mode_keeps_merges_joining_independent_lines() {
    let mut r = feature_merge();
    // Tag C: now M's parents rewrite to C and E, neither an ancestor of the other.
    r.git(&["tag", "c-tag", "main~1"]);
    r.commit("F");
    let repo = r.load();
    let g = revgraph::build(&repo, &with_mode(Simplification::Decorated));
    assert_eq!(node_subjects(&repo, &g), ["A", "C", "E", "F", "M"]);
    assert_eq!(
        edge_list(&repo, &g),
        [
            edge("C", "A", true, 1),
            edge("E", "A", true, 2),
            edge("F", "M", true, 0),
            edge("M", "C", true, 0),
            edge("M", "E", false, 0),
        ]
    );
}

#[test]
fn branches_and_merges_mode_matches_tortoisegit_collapse() {
    let r = feature_merge();
    let repo = r.load();
    let g = revgraph::build(&repo, &with_mode(Simplification::BranchesAndMerges));
    // Kept: root A, fork point B, merge sources C and E, merge M. D is a pass-through.
    assert_eq!(node_subjects(&repo, &g), ["A", "B", "C", "E", "M"]);
    assert_eq!(
        edge_list(&repo, &g),
        [
            edge("B", "A", true, 0),
            edge("C", "B", true, 0),
            edge("E", "B", true, 1),
            edge("M", "C", true, 0),
            edge("M", "E", false, 0),
        ]
    );
}

#[test]
fn all_commits_mode_keeps_everything() {
    let r = feature_merge();
    let repo = r.load();
    let g = revgraph::build(&repo, &with_mode(Simplification::AllCommits));
    assert_eq!(g.nodes.len(), 6);
    assert_eq!(g.edges.len(), 6);
    assert!(g.edges.iter().all(|e| e.hidden == 0));
}

#[test]
fn hiding_a_branch_hides_its_exclusive_history() {
    let mut r = TestRepo::new();
    r.commit("A");
    r.branch("side");
    r.commit("S");
    r.checkout("main");
    r.commit("B");
    let repo = r.load();
    let mut opts = with_mode(Simplification::AllCommits);
    assert_eq!(revgraph::build(&repo, &opts).nodes.len(), 3);
    opts.show_local_branches = false;
    // Only HEAD's branch remains visible.
    let g = revgraph::build(&repo, &opts);
    assert_eq!(node_subjects(&repo, &g), ["A", "B"]);
}

#[test]
fn tags_need_not_create_nodes() {
    let mut r = TestRepo::new();
    r.commit("A");
    r.commit("B");
    r.git(&["tag", "t"]);
    r.commit("C");
    let repo = r.load();
    let mut opts = with_mode(Simplification::Decorated);
    assert_eq!(
        node_subjects(&repo, &revgraph::build(&repo, &opts)),
        ["A", "B", "C"]
    );
    opts.tags_make_nodes = false;
    let g = revgraph::build(&repo, &opts);
    assert_eq!(node_subjects(&repo, &g), ["A", "C"]);
    assert_eq!(edge_list(&repo, &g), [edge("C", "A", true, 1)]);
}

#[test]
fn stash_shows_as_single_edge_to_its_base() {
    let mut r = TestRepo::new();
    r.commit("A");
    std::fs::write(r.path().join("f.txt"), "x").unwrap();
    r.git(&["add", "f.txt"]);
    r.git(&["stash", "-q"]);
    let repo = r.load();
    let g = revgraph::build(&repo, &with_mode(Simplification::AllCommits));
    let stash = g
        .nodes
        .iter()
        .position(|n| n.refs.iter().any(|&i| repo.refs[i].kind == RefKind::Stash))
        .expect("stash node");
    assert_eq!(
        g.edges.iter().filter(|e| e.child == stash as u32).count(),
        1
    );
    assert_eq!(g.nodes.len(), 2, "index snapshot commit is not shown");
}

#[test]
fn detached_head_gets_a_label() {
    let mut r = TestRepo::new();
    r.commit("A");
    r.commit("B");
    r.checkout("HEAD~1");
    let repo = r.load();
    assert!(matches!(repo.head, Head::Detached(_)));
    let g = revgraph::build(&repo, &GraphOptions::default());
    let head = g.nodes.iter().find(|n| n.is_head).expect("head node");
    assert_eq!(repo.commit(head.commit).subject, "A");
    assert_eq!(repo.refs[head.refs[0]].kind, RefKind::DetachedHead);
}

#[test]
fn decorated_mode_drops_merges_of_empty_rooted_histories() {
    // An unrelated history whose root has an empty tree, merged in (like an svn import).
    let mut r = TestRepo::new();
    std::fs::write(r.path().join("f.txt"), "x").unwrap();
    r.git(&["add", "f.txt"]);
    r.commit("A");
    r.git(&["checkout", "-q", "--orphan", "imported"]);
    r.git(&["rm", "-q", "-r", "-f", "."]);
    r.commit("R");
    std::fs::write(r.path().join("g.txt"), "y").unwrap();
    r.git(&["add", "g.txt"]);
    r.commit("S");
    r.checkout("main");
    r.git(&[
        "merge",
        "-q",
        "--allow-unrelated-histories",
        "-m",
        "M",
        "imported",
    ]);
    r.git(&["branch", "-D", "imported"]);
    let repo = r.load();
    let g = revgraph::build(&repo, &with_mode(Simplification::Decorated));
    // git log --simplify-by-decoration shows only M; A is shown as the end of M's edge.
    assert_eq!(node_subjects(&repo, &g), ["A", "M"]);
    assert_eq!(edge_list(&repo, &g), [edge("M", "A", true, 0)]);
    let g = revgraph::build(&repo, &with_mode(Simplification::BranchesAndMerges));
    assert!(node_subjects(&repo, &g).contains(&"R".to_owned()));
}

#[test]
fn filters_limit_history_to_matching_refs() {
    let mut r = TestRepo::new();
    r.commit("A");
    r.branch("feature/x");
    r.commit("X");
    r.checkout("main");
    r.branch("bugfix/y");
    r.commit("Y");
    r.checkout("main");
    r.commit("B");
    r.git(&["tag", "t-on-b"]);
    let repo = r.load();

    let mut opts = with_mode(Simplification::AllCommits);
    assert_eq!(
        node_subjects(&repo, &revgraph::build(&repo, &opts)),
        ["A", "B", "X", "Y"]
    );

    opts.current_branch_only = true;
    let g = revgraph::build(&repo, &opts);
    assert_eq!(node_subjects(&repo, &g), ["A", "B"]);
    let labels: Vec<&str> = g
        .nodes
        .iter()
        .flat_map(|n| n.refs.iter().map(|&i| repo.refs[i].name.as_str()))
        .collect();
    assert!(
        labels.contains(&"t-on-b"),
        "refs inside the shown history keep their labels"
    );

    opts.current_branch_only = false;
    opts.ref_filter = "FEATURE, nothing".into();
    assert_eq!(
        node_subjects(&repo, &revgraph::build(&repo, &opts)),
        ["A", "B", "X"]
    );
}
