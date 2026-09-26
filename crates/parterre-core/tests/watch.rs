//! Noticing ref changes (automatic reload) against real repositories.

mod common;

use common::TestRepo;
use parterre_core::watch::RefStorage;

/// Runs `change` and reports whether the fingerprint moved.
fn changes(storage: &RefStorage, change: impl FnOnce()) -> bool {
    let before = storage.fingerprint();
    change();
    storage.fingerprint() != before
}

#[test]
fn fingerprint_follows_ref_changes() {
    let mut r = TestRepo::new();
    r.commit("a");
    let storage = RefStorage::locate(r.path()).expect("locate");

    assert!(!changes(&storage, || {
        r.git(&["status"]);
        r.git(&["log", "--oneline"]);
        r.load();
    }));
    assert!(changes(&storage, || {
        r.commit("b");
    }));
    assert!(changes(&storage, || {
        r.git(&["branch", "topic"]);
    }));
    assert!(changes(&storage, || {
        r.git(&["tag", "v1"]);
    }));
    assert!(changes(&storage, || r.checkout("topic")));
    assert!(changes(&storage, || {
        r.git(&["pack-refs", "--all"]);
    }));
    // A packed ref, moved and then deleted.
    assert!(changes(&storage, || {
        r.git(&["update-ref", "refs/tags/v1", "HEAD~1"]);
    }));
    assert!(changes(&storage, || {
        r.git(&["branch", "-D", "main"]);
    }));
}

#[test]
fn same_refs_tells_real_changes_from_rewrites() {
    let mut r = TestRepo::new();
    r.commit("a");
    r.git(&["branch", "topic"]);
    let before = r.load();

    r.git(&["pack-refs", "--all"]);
    assert!(before.same_refs(&r.load()), "packing moves no ref");

    r.checkout("topic");
    assert!(!before.same_refs(&r.load()), "HEAD moved to another branch");
    r.checkout("main");

    r.git(&["update-ref", "refs/heads/topic", "HEAD"]);
    r.commit("b");
    assert!(!before.same_refs(&r.load()), "main moved");
}

#[test]
fn linked_worktree_sees_its_own_head_and_shared_refs() {
    let mut r = TestRepo::new();
    r.commit("a");
    let wt = r.dir.path().join("wt");
    r.git(&["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()]);
    let storage = RefStorage::locate(&wt).expect("locate");

    // HEAD of the worktree lives in its own git dir.
    assert!(changes(&storage, || {
        r.git(&["-C", wt.to_str().unwrap(), "checkout", "-q", "--detach"]);
    }));
    // Branches are shared with the main working tree.
    assert!(changes(&storage, || {
        r.commit("b");
    }));
}
