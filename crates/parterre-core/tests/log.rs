//! Log window core against real repositories: the log query's order compared with
//! `git log --date-order`, changed files, and git's hash length.

mod common;

use common::TestRepo;
use parterre_core::changed_files::{ChangedFile, FileStatus};
use parterre_core::git::Git;
use parterre_core::log::LogQuery;
use parterre_core::{CommitIx, Oid, Repo};

fn ix(repo: &Repo, hash: &str) -> CommitIx {
    repo.lookup(&Oid::from_hex(hash).unwrap())
        .expect("commit in snapshot")
}

fn hashes(repo: &Repo, list: &[CommitIx]) -> Vec<String> {
    list.iter().map(|&c| repo.commit(c).oid.to_hex()).collect()
}

/// `git log --date-order` with the given revisions, as full hashes.
fn git_log(r: &TestRepo, revs: &[&str]) -> Vec<String> {
    let mut args = vec!["log", "--date-order", "--format=%H"];
    args.extend(revs);
    r.git(&args)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

fn subjects(repo: &Repo, list: &[CommitIx]) -> Vec<String> {
    list.iter()
        .map(|&c| repo.commit(c).subject.clone())
        .collect()
}

/// A small deterministic random generator (xorshift), so failures reproduce.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// Branches that fork, merge and commit in random order, with equal and skewed commit times.
fn random_history(seed: u64, steps: usize) -> (TestRepo, Vec<String>) {
    let mut rng = Rng(seed);
    let mut r = TestRepo::new();
    let mut all = vec![r.commit("root")];
    let mut branches = vec!["main".to_owned()];
    let mut clock = 10u32;
    for step in 0..steps {
        // Mostly forward, sometimes the same minute, sometimes back in time.
        clock = match rng.below(10) {
            0 => clock.saturating_sub(5),
            1 | 2 => clock,
            _ => clock + 1,
        };
        r.set_clock(clock.saturating_sub(1));
        let branch = branches[rng.below(branches.len() as u64) as usize].clone();
        r.checkout(&branch);
        match rng.below(6) {
            0 if branches.len() < 5 => {
                let name = format!("b{step}");
                r.branch(&name);
                branches.push(name);
                all.push(r.commit(&format!("c{step}")));
            }
            1 if branches.len() > 1 => {
                let other = loop {
                    let o = &branches[rng.below(branches.len() as u64) as usize];
                    if *o != branch {
                        break o.clone();
                    }
                };
                all.push(r.merge(&other, &format!("m{step}")));
            }
            _ => all.push(r.commit(&format!("c{step}"))),
        }
    }
    (r, all)
}

#[test]
fn order_matches_git_log_date_order() {
    for seed in [1, 7, 42] {
        let (r, all) = random_history(seed, 60);
        let repo = r.load();
        for tip in all.iter().rev().step_by(7) {
            let ours = LogQuery::commit(ix(&repo, tip)).run(&repo);
            assert_eq!(
                hashes(&repo, &ours),
                git_log(&r, &[tip]),
                "seed {seed}, tip {tip}"
            );
        }
    }
}

#[test]
fn ranges_match_git_log_date_order() {
    for seed in [3, 11] {
        let (r, all) = random_history(seed, 50);
        let repo = r.load();
        let mut rng = Rng(seed * 31 + 1);
        for _ in 0..15 {
            let first = &all[rng.below(all.len() as u64) as usize];
            let second = &all[rng.below(all.len() as u64) as usize];
            let q = LogQuery::range(&repo, ix(&repo, first), ix(&repo, second));
            let (from, to) = (
                repo.commit(q.exclude[0]).oid.to_hex(),
                repo.commit(q.tips[0]).oid.to_hex(),
            );
            let expected = git_log(&r, &[&format!("{from}..{to}")]);
            assert_eq!(hashes(&repo, &q.run(&repo)), expected, "seed {seed}");
        }
    }
}

/// main: A - B - C
///            \
/// feature:    D - E
fn feature_branch() -> (TestRepo, [String; 5]) {
    let mut r = TestRepo::new();
    let a = r.commit("A");
    let b = r.commit("B");
    r.branch("feature");
    let d = r.commit("D");
    let e = r.commit("E");
    r.checkout("main");
    let c = r.commit("C");
    (r, [a, b, c, d, e])
}

#[test]
fn two_nodes_list_what_second_has_and_first_lacks() {
    let (r, [_, _, c, _, e]) = feature_branch();
    let repo = r.load();
    let q = LogQuery::range(&repo, ix(&repo, &c), ix(&repo, &e));
    assert_eq!(subjects(&repo, &q.run(&repo)), ["E", "D"]);
    let q = LogQuery::range(&repo, ix(&repo, &e), ix(&repo, &c));
    assert_eq!(subjects(&repo, &q.run(&repo)), ["C"]);
}

#[test]
fn two_nodes_swap_when_second_is_an_ancestor_of_first() {
    let (r, [a, _, _, _, e]) = feature_branch();
    let repo = r.load();
    // TortoiseGit would show `e..a`, which is empty.
    assert!(git_log(&r, &[&format!("{e}..{a}")]).is_empty());
    let q = LogQuery::range(&repo, ix(&repo, &e), ix(&repo, &a));
    assert_eq!(q.tips, [ix(&repo, &e)]);
    assert_eq!(q.exclude, [ix(&repo, &a)]);
    assert_eq!(subjects(&repo, &q.run(&repo)), ["E", "D", "B"]);
}

#[test]
fn names_resolve_and_label_the_range() {
    let (r, [a, _, c, _, e]) = feature_branch();
    let repo = r.load();
    assert_eq!(repo.resolve("main"), Some(ix(&repo, &c)));
    assert_eq!(repo.resolve("refs/heads/feature"), Some(ix(&repo, &e)));
    assert_eq!(repo.resolve("HEAD"), Some(ix(&repo, &c)));
    assert_eq!(repo.resolve(&a[..10]), Some(ix(&repo, &a)));
    assert_eq!(repo.resolve("nothing"), None);

    let refs = repo.refs_by_commit();
    let all = |_: &parterre_core::GitRef| true;
    let q = LogQuery::for_selection(&repo, &[ix(&repo, &c), ix(&repo, &e)]).unwrap();
    assert_eq!(q.label(&repo, &refs, all).to_string(), "main..feature");
    // No ref on A: its short hash, as long as git makes them.
    let q = LogQuery::for_selection(&repo, &[ix(&repo, &e), ix(&repo, &a)]).unwrap();
    let short = &a[..repo.abbrev_len];
    assert_eq!(
        q.label(&repo, &refs, all).to_string(),
        format!("{short}..feature")
    );
}

#[test]
fn two_nodes_from_unrelated_histories_list_all_of_second() {
    let mut r = TestRepo::new();
    let a1 = r.commit("A1");
    let a2 = r.commit("A2");
    r.git(&["checkout", "-q", "--orphan", "other"]);
    let o1 = r.commit("O1");
    let o2 = r.commit("O2");
    let repo = r.load();
    let q = LogQuery::range(&repo, ix(&repo, &a2), ix(&repo, &o2));
    assert_eq!(hashes(&repo, &q.run(&repo)), [o2.clone(), o1]);
    let q = LogQuery::range(&repo, ix(&repo, &o2), ix(&repo, &a2));
    assert_eq!(hashes(&repo, &q.run(&repo)), [a2, a1]);
}

fn changed(r: &TestRepo, rev: &str) -> Vec<ChangedFile> {
    let hash = r.git(&["rev-parse", rev]);
    Git::new(r.path())
        .changed_files(&Oid::from_hex(&hash).unwrap())
        .expect("changed files")
}

type Summary = (String, Option<String>, FileStatus, Option<u32>, Option<u32>);

fn summary(files: &[ChangedFile]) -> Vec<Summary> {
    files
        .iter()
        .map(|f| {
            (
                f.path.clone(),
                f.old_path.clone(),
                f.status,
                f.added,
                f.removed,
            )
        })
        .collect()
}

fn entry(path: &str, status: FileStatus, lines: Option<(u32, u32)>) -> Summary {
    (
        path.into(),
        None,
        status,
        lines.map(|l| l.0),
        lines.map(|l| l.1),
    )
}

#[test]
fn root_commit_is_compared_with_the_empty_tree() {
    let mut r = TestRepo::new();
    r.write("README", b"one\ntwo\n");
    r.write("src/lib.rs", b"x\n");
    r.commit_all("root");
    assert_eq!(
        summary(&changed(&r, "HEAD")),
        [
            entry("README", FileStatus::Added, Some((2, 0))),
            entry("src/lib.rs", FileStatus::Added, Some((1, 0))),
        ]
    );
}

#[test]
fn renames_modifications_deletions_and_binary_files() {
    let mut r = TestRepo::new();
    let text: String = (0..20).map(|i| format!("line {i}\n")).collect();
    r.write("old.txt", text.as_bytes());
    r.write("gone.txt", b"bye\n");
    r.write("edit.txt", b"a\nb\n");
    r.write("image.bin", b"\x00\x01\x02");
    r.commit_all("base");
    r.git(&["mv", "old.txt", "new.txt"]);
    r.write("new.txt", format!("{text}one more\n").as_bytes());
    r.git(&["rm", "-q", "gone.txt"]);
    r.write("edit.txt", b"a\nB\nc\n");
    r.write("image.bin", b"\x00\x03\x02\x04");
    r.commit_all("change");
    let mut renamed = entry("new.txt", FileStatus::Renamed, Some((1, 0)));
    renamed.1 = Some("old.txt".into());
    assert_eq!(
        summary(&changed(&r, "HEAD")),
        [
            entry("edit.txt", FileStatus::Modified, Some((2, 1))),
            entry("gone.txt", FileStatus::Deleted, Some((0, 1))),
            entry("image.bin", FileStatus::Modified, None),
            renamed,
        ]
    );
}

#[test]
fn merge_is_compared_with_its_first_parent() {
    let mut r = TestRepo::new();
    r.write("base", b"base\n");
    r.commit_all("base");
    r.branch("feature");
    r.write("feature.txt", b"f\n");
    r.commit_all("feature");
    r.checkout("main");
    r.write("main.txt", b"m\n");
    r.commit_all("main");
    r.merge("feature", "merge");
    assert_eq!(
        summary(&changed(&r, "HEAD")),
        [entry("feature.txt", FileStatus::Added, Some((1, 0)))]
    );
}

#[test]
fn odd_file_names_survive() {
    let mut r = TestRepo::new();
    let names = [
        "with space.txt",
        "ünïcødé ☃.md",
        "new\nline",
        "tab\there",
        ":colon",
        "dir with space/Ä.txt",
        "\"quoted\"",
    ];
    for name in names {
        r.stage(name, b"x\n");
    }
    r.commit("odd");
    let files = changed(&r, "HEAD");
    let mut got: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    got.sort_unstable();
    let mut want = names.to_vec();
    want.sort_unstable();
    assert_eq!(got, want);
    assert!(files.iter().all(|f| f.added == Some(1)));
}

#[test]
fn changed_files_are_in_path_order() {
    let mut r = TestRepo::new();
    for name in ["b.txt", "A/z.txt", "a.txt", "C.txt", "a/sub/x", "a/y"] {
        r.stage(name, b"x\n");
    }
    r.commit("tree");
    let files = changed(&r, "HEAD");
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        ["a.txt", "b.txt", "C.txt", "A/z.txt", "a/y", "a/sub/x"]
    );
}

#[test]
fn hash_length_follows_git() {
    let mut r = TestRepo::new();
    r.commit("one");
    // Several refs, so that several commits are sampled.
    r.git(&["tag", "t1"]);
    r.branch("side");
    r.commit("two");
    r.git(&["tag", "t2"]);
    let head = r.commit("three");
    let repo = r.load();
    assert_eq!(repo.abbrev_len, r.git(&["log", "-1", "--format=%h"]).len());
    assert_eq!(repo.abbrev_len, 7);
    assert_eq!(
        repo.commit(ix(&repo, &head)).oid.short(repo.abbrev_len),
        head[..7]
    );

    r.git(&["config", "core.abbrev", "12"]);
    assert_eq!(r.load().abbrev_len, 12);
}

#[test]
fn hash_length_of_an_empty_repository_is_the_default() {
    let r = TestRepo::new();
    assert_eq!(r.load().abbrev_len, 7);
}
