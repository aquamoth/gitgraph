//! Sets `PARTERRE_VERSION`, the version string `--version` and Help show (see `src/version.rs`),
//! and gives the Windows executable its icon.
//!
//! The release workflow sets `PARTERRE_RELEASE_TAG` to the pushed tag; the build then fails
//! unless that tag matches `Cargo.toml` and the commit being built.
//!
//! The sources are either a git checkout of the workspace, or a crate packaged by `cargo
//! package` (e.g. downloaded from crates.io by `cargo install`), which has no `.git` but records
//! its commit in `.cargo_vcs_info.json`.

#[path = "src/version.rs"]
mod version;

use std::path::{Path, PathBuf};
use std::process::Command;

use version::{GitState, Source};

const RELEASE_TAG: &str = "PARTERRE_RELEASE_TAG";

/// What goes into the binary, relative to the workspace root. Only changes here make a build
/// dirty, and they rerun this script so the flag stays current.
const SOURCES: [&str; 6] = [
    "crates",
    "packaging/icon",
    "Cargo.toml",
    "Cargo.lock",
    ".cargo",
    "rust-toolchain.toml",
];

fn main() {
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest_dir.join("../..");
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap();
    println!("cargo:rerun-if-env-changed={RELEASE_TAG}");
    let release_tag = std::env::var(RELEASE_TAG).ok().filter(|t| !t.is_empty());

    let packaged = manifest_dir.join("Cargo.toml.orig").exists();

    // The About dialog shows LICENSE and NOTICE. A packaged crate carries its own copies. In the
    // workspace the crate's are symlinks to the root's, which Windows checkouts may turn into
    // text files holding the link's target, so read the root's.
    let legal_dir = if packaged { &manifest_dir } else { &root };
    for name in ["LICENSE", "NOTICE"] {
        let path = legal_dir.join(name);
        println!("cargo:rustc-env=PARTERRE_{name}={}", path.display());
    }

    let source = if packaged {
        // Packaged: the workspace around it (if any) isn't what is being built.
        let info = std::fs::read_to_string(manifest_dir.join(".cargo_vcs_info.json"));
        Source::Package(
            info.map(|s| version::parse_vcs_info(&s))
                .unwrap_or_default(),
        )
    } else if let Some(git) = git_state(&root) {
        watch(&root);
        Source::Git(git)
    } else {
        Source::Unknown
    };
    let version = version::describe(&pkg_version, release_tag.as_deref(), &source)
        .unwrap_or_else(|e| panic!("{e}"));
    println!("cargo:rustc-env=PARTERRE_VERSION={version}");

    windows_icon(&manifest_dir);
}

/// Embeds `packaging/icon/parterre.ico` in the Windows executable (`parterre.rc`). Cosmetic, so
/// a missing resource compiler only warns: `rc.exe` comes with the Windows SDK, and checking
/// the `windows-gnu` target from Linux needs `x86_64-w64-mingw32-windres`. On other targets
/// this does nothing.
fn windows_icon(manifest_dir: &Path) {
    let rc = manifest_dir.join("parterre.rc");
    println!("cargo:rerun-if-changed={}", rc.display());
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir
            .join("../../packaging/icon/parterre.ico")
            .display()
    );
    embed_resource::compile(rc, embed_resource::NONE)
        .manifest_optional()
        .unwrap_or_else(|e| panic!("{e}"));
}

/// `None` if this isn't a git checkout (e.g. a source archive) or git isn't installed.
fn git_state(dir: &Path) -> Option<GitState> {
    // Only a repository of our own counts. Unpacked into some other checkout (a packaging
    // repository, say), git would describe that one instead.
    let top = git(dir, &["rev-parse", "--show-toplevel"])?;
    if Path::new(&top).canonicalize().ok()? != dir.canonicalize().ok()? {
        return None;
    }
    let commit = git(dir, &["rev-parse", "--short=7", "HEAD"])?;
    // No optional locks: a plain `git status` may refresh the index, and building shouldn't
    // write to the repository.
    let mut status = vec![
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=no",
        "--",
    ];
    status.extend(SOURCES);
    let status = git(dir, &status)?;
    let tags = git(dir, &["tag", "--points-at", "HEAD"])?;
    Some(GitState {
        commit,
        dirty: !status.is_empty(),
        tags: tags.lines().map(str::to_owned).collect(),
    })
}

/// Reruns this script when the commit or the sources change, so the hash and dirty flag stay
/// current without rerunning on every build.
fn watch(dir: &Path) {
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
    paths.extend(SOURCES.map(String::from));
    // Cargo reruns every build for a path that doesn't exist, so skip those.
    for p in paths.iter().map(|p| dir.join(p)).filter(|p| p.exists()) {
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
