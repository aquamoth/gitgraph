//! Sets `GITGRAPH_VERSION`, the version string `--version` and Help show (see `src/version.rs`).
//!
//! The release workflow sets `GITGRAPH_RELEASE_TAG` to the pushed tag; the build then fails
//! unless that tag matches `Cargo.toml` and the commit being built.

#[path = "src/version.rs"]
mod version;

use std::path::{Path, PathBuf};
use std::process::Command;

use version::GitState;

const RELEASE_TAG: &str = "GITGRAPH_RELEASE_TAG";

fn main() {
    let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap();
    println!("cargo:rerun-if-env-changed={RELEASE_TAG}");
    let release_tag = std::env::var(RELEASE_TAG).ok().filter(|t| !t.is_empty());

    let git = git_state(&dir);
    if git.is_some() {
        watch_git(&dir);
    }
    match version::describe(&pkg_version, release_tag.as_deref(), git.as_ref()) {
        Ok(v) => println!("cargo:rustc-env=GITGRAPH_VERSION={v}"),
        Err(e) => panic!("{e}"),
    }
}

/// `None` if this isn't a git checkout (e.g. a source archive) or git isn't installed.
fn git_state(dir: &Path) -> Option<GitState> {
    let commit = git(dir, &["rev-parse", "--short=7", "HEAD"])?;
    // No optional locks: a plain `git status` may rewrite the index, which would look like a
    // change to the files watched below.
    let status = git(
        dir,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=no",
        ],
    )?;
    let tags = git(dir, &["tag", "--points-at", "HEAD"])?;
    Some(GitState {
        commit,
        dirty: !status.is_empty(),
        tags: tags.lines().map(str::to_owned).collect(),
    })
}

/// Reruns this script when the commit or the sources change, so the hash and dirty flag stay
/// current without rerunning on every build.
fn watch_git(dir: &Path) {
    let mut paths = Vec::new();
    // HEAD's reflog changes on every commit, checkout and reset, including in worktrees.
    for name in ["HEAD", "logs/HEAD", "packed-refs"] {
        paths.extend(git(dir, &["rev-parse", "--git-path", name]));
    }
    if let Some(branch) = git(dir, &["rev-parse", "--symbolic-full-name", "HEAD"])
        && branch != "HEAD"
    {
        paths.extend(git(dir, &["rev-parse", "--git-path", &branch]));
    }
    let mut paths: Vec<PathBuf> = paths.into_iter().map(|p| dir.join(p)).collect();
    // Edits anywhere in the workspace's crates, or to its manifest and lock file (dirty flag).
    let crates = dir.join("..");
    let root = crates.join("..");
    paths.extend([crates, root.join("Cargo.toml"), root.join("Cargo.lock")]);
    // Cargo reruns every build for a path that doesn't exist, so skip those.
    for p in paths.iter().filter(|p| p.exists()) {
        println!("cargo:rerun-if-changed={}", p.display());
    }
}

/// Runs git in `dir`; its trimmed output, or `None` if it fails.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
