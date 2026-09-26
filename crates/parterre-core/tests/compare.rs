//! Comparing two commits: sides, and the files git lists between them.

mod common;

use common::TestRepo;
use parterre_core::changed_files::FileStatus;
use parterre_core::compare::Comparison;
use parterre_core::git::Git;
use parterre_core::{CommitIx, Oid, Repo};

fn ix(repo: &Repo, hash: &str) -> CommitIx {
    repo.lookup(&Oid::from_hex(hash).unwrap())
        .expect("commit in snapshot")
}

fn oid(hash: &str) -> Oid {
    Oid::from_hex(hash).unwrap()
}

/// `main` and `feature` forked from `base`: main changed `shared.txt` and added `main.txt`,
/// feature added `feature.txt`.
fn forked() -> (TestRepo, [String; 3]) {
    let mut r = TestRepo::new();
    r.write("shared.txt", b"one\n");
    let base = r.commit_all("base");
    r.branch("feature");
    r.write("feature.txt", b"f\n");
    let feature = r.commit_all("feature");
    r.checkout("main");
    r.write("shared.txt", b"one\ntwo\n");
    r.write("main.txt", b"m\n");
    let main = r.commit_all("main");
    (r, [base, feature, main])
}

fn listed(r: &TestRepo, c: Comparison) -> Vec<(String, FileStatus)> {
    let compared = c.run(&Git::new(r.path())).expect("compare");
    compared
        .files
        .into_iter()
        .map(|f| (f.path, f.status))
        .collect()
}

#[test]
fn an_ancestor_goes_on_the_left_whichever_is_picked_first() {
    let (r, [base, feature, main]) = forked();
    let repo = r.load();
    let c = Comparison::of(&repo, ix(&repo, &feature), ix(&repo, &base), false);
    assert_eq!((c.old, c.new), (oid(&base), oid(&feature)));
    // Diverged commits keep the order they were picked in.
    let c = Comparison::of(&repo, ix(&repo, &feature), ix(&repo, &main), false);
    assert_eq!((c.old, c.new), (oid(&feature), oid(&main)));
    let swapped = c.swapped();
    assert_eq!((swapped.old, swapped.new), (oid(&main), oid(&feature)));
}

#[test]
fn trees_are_compared_whole_or_since_the_common_ancestor() {
    let (r, [base, feature, main]) = forked();
    let repo = r.load();
    let c = Comparison::of(&repo, ix(&repo, &main), ix(&repo, &feature), false);
    assert_eq!(
        listed(&r, c),
        [
            ("feature.txt".into(), FileStatus::Added),
            ("main.txt".into(), FileStatus::Deleted),
            ("shared.txt".into(), FileStatus::Modified),
        ]
    );
    // Since the fork only what feature did counts.
    let c = Comparison {
        since_ancestor: true,
        ..c
    };
    let compared = c.run(&Git::new(r.path())).unwrap();
    assert_eq!(compared.base, Some(oid(&base)));
    assert_eq!(listed(&r, c), [("feature.txt".into(), FileStatus::Added)]);
}

#[test]
fn unrelated_histories_have_no_common_ancestor() {
    let (mut r, [_, _, main]) = forked();
    r.git(&["checkout", "-q", "--orphan", "other"]);
    r.git(&["rm", "-rqf", "."]);
    r.write("other.txt", b"o\n");
    let other = r.commit_all("other");
    let c = Comparison {
        old: oid(&main),
        new: oid(&other),
        since_ancestor: true,
    };
    let compared = c.run(&Git::new(r.path())).unwrap();
    assert_eq!(compared.base, None);
    assert!(compared.files.is_empty());
    let whole = listed(
        &r,
        Comparison {
            since_ancestor: false,
            ..c
        },
    );
    assert_eq!(whole.len(), 3, "{whole:?}");
}
