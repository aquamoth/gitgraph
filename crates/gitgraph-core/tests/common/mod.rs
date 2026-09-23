//! Builds throwaway git repositories with scripted histories for integration tests.

#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use gitgraph_core::Repo;
use tempfile::TempDir;

pub struct TestRepo {
    pub dir: TempDir,
    clock: u32,
}

impl TestRepo {
    pub fn new() -> TestRepo {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = TestRepo { dir, clock: 0 };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.com"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "tag.gpgsign", "false"]);
        repo
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        let date = format!("{} +0000", 1_700_000_000 + self.clock * 60);
        let out = Command::new("git")
            .current_dir(self.dir.path())
            .args(args)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Makes an empty commit with `message` as subject and returns its hash.
    pub fn commit(&mut self, message: &str) -> String {
        self.clock += 1;
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn checkout(&self, rev: &str) {
        self.git(&["checkout", "-q", rev]);
    }

    pub fn branch(&self, name: &str) {
        self.git(&["checkout", "-q", "-b", name]);
    }

    /// `git merge --no-ff` of `rev` into the current branch; returns the merge commit.
    pub fn merge(&mut self, rev: &str, message: &str) -> String {
        self.clock += 1;
        self.git(&["merge", "-q", "--no-ff", "-m", message, rev]);
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn load(&self) -> Repo {
        gitgraph_core::git::load_repo(self.path()).expect("load repo")
    }
}
