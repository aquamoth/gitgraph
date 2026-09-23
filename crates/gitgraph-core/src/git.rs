//! Loading a [`Repo`] snapshot by running the `git` command-line tool.
//!
//! We shell out to `git` rather than linking a git library: it is always present where
//! gitgraph is useful, honours every repository configuration (worktrees, alternates,
//! packed refs, sha256, ...), and `git log` streams tens of thousands of commits in
//! milliseconds.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::oid::Oid;
use crate::repo::{Commit, CommitIx, GitRef, Head, RefKind, Repo};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("could not run git ({0}); is git installed and on PATH?")]
    Spawn(#[source] std::io::Error),
    #[error("`git {args}` failed: {stderr}")]
    Failed { args: String, stderr: String },
    #[error("{0} is not inside a git repository")]
    NotARepository(PathBuf),
    #[error("unexpected output from git: {0}")]
    Parse(String),
}

/// A handle for running git commands against one repository.
#[derive(Clone, Debug)]
pub struct Git {
    dir: PathBuf,
}

const FIELD: char = '\x1f';
const EMPTY_TREE_SHA1: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const EMPTY_TREE_SHA256: &str = "6ef19b41225c5369f1c104d45d8d85efa9b057b53b14b4b9b939dd74decc5321";
const RECORD: char = '\x1e';

impl Git {
    pub fn new(dir: impl Into<PathBuf>) -> Git {
        Git { dir: dir.into() }
    }

    fn command<I, S>(&self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&self.dir)
            .args(["-c", "core.quotepath=off"])
            .args(["-c", "log.showSignature=false"])
            .args(["-c", "i18n.logOutputEncoding=UTF-8"])
            .args(["-c", "color.ui=false"])
            .args(args)
            // Read-only tool: never take the index lock for opportunistic refreshes.
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            // gitgraph is a GUI-subsystem app on Windows; without this every git invocation
            // would flash a console window.
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    fn output(&self, args: &[&str]) -> Result<Output, GitError> {
        self.command(args).output().map_err(GitError::Spawn)
    }

    /// Runs git and returns stdout, failing on a non-zero exit status.
    fn run(&self, args: &[&str]) -> Result<String, GitError> {
        let out = self.output(args)?;
        if !out.status.success() {
            return Err(GitError::Failed {
                args: args.join(" "),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Runs git and returns trimmed stdout, or `None` on a non-zero exit status (for queries
    /// such as `symbolic-ref -q` that signal "no" through the exit code).
    fn query(&self, args: &[&str]) -> Result<Option<String>, GitError> {
        let out = self.output(args)?;
        Ok(out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned()))
    }

    /// Resolves the repository root: the working tree, or the git dir for a bare repository.
    pub fn repo_root(&self) -> Result<PathBuf, GitError> {
        let Some(out) = self.query(&["rev-parse", "--is-bare-repository", "--absolute-git-dir"])?
        else {
            return Err(GitError::NotARepository(self.dir.clone()));
        };
        let mut lines = out.lines();
        let bare = lines.next() == Some("true");
        let git_dir = lines
            .next()
            .ok_or_else(|| GitError::Parse("rev-parse printed no git dir".into()))?;
        if bare {
            return Ok(PathBuf::from(git_dir));
        }
        let top = self.run(&["rev-parse", "--show-toplevel"])?;
        Ok(PathBuf::from(top.trim()))
    }

    /// Loads every commit reachable from any ref (notes excluded) together with all refs.
    pub fn load(&self) -> Result<Repo, GitError> {
        let root = self.repo_root()?;
        let git = Git::new(&root);

        let log_format = format!(
            "--format=%H{FIELD}%P{FIELD}%T{FIELD}%an{FIELD}%ae{FIELD}%at{FIELD}%ad{FIELD}%ct{FIELD}%s{RECORD}"
        );
        let log = git.run(&[
            "log",
            "--no-color",
            "--no-decorate",
            "--date=format-local:%Y-%m-%d %H:%M",
            &log_format,
            "--exclude=refs/notes/*",
            "--all",
        ])?;
        let (commits, by_oid) = parse_log(&log)?;

        let ref_format = format!(
            "--format=%(refname){FIELD}%(objecttype){FIELD}%(objectname){FIELD}%(*objecttype){FIELD}%(*objectname){FIELD}%(symref)"
        );
        let refs_out = git.run(&["for-each-ref", &ref_format])?;
        let head_branch = git.query(&["symbolic-ref", "-q", "HEAD"])?;
        let head_oid = git
            .query(&["rev-parse", "-q", "--verify", "HEAD^{commit}"])?
            .and_then(|s| Oid::from_hex(&s));

        let lookup = |oid: &Oid| by_oid.get(oid).copied();
        let head = match (&head_branch, head_oid) {
            (Some(branch), target) => Head::Branch {
                name: branch.clone(),
                target: target.and_then(|o| lookup(&o)),
            },
            (None, Some(oid)) => Head::Detached(
                lookup(&oid).ok_or_else(|| GitError::Parse(format!("HEAD {oid} not in log")))?,
            ),
            (None, None) => {
                return Err(GitError::Parse(
                    "HEAD is neither a branch nor a commit".into(),
                ));
            }
        };

        let mut refs = parse_refs(&refs_out, head_branch.as_deref(), lookup);
        if let Head::Detached(c) = head {
            refs.push(GitRef {
                full_name: "HEAD".into(),
                name: "HEAD".into(),
                kind: RefKind::DetachedHead,
                target: c,
                annotated: false,
                is_head: true,
            });
        }
        Ok(Repo::new(root, commits, refs, head))
    }
}

impl Git {
    /// The full commit message (subject and body) of a commit.
    pub fn message(&self, oid: &Oid) -> Result<String, GitError> {
        let out = self.run(&["log", "-1", "--no-color", "--format=%B", &oid.to_hex()])?;
        Ok(out.trim_end().to_owned())
    }
}

/// Convenience wrapper: load the repository containing `dir`.
pub fn load_repo(dir: &Path) -> Result<Repo, GitError> {
    Git::new(dir).load()
}

fn parse_log(log: &str) -> Result<(Vec<Commit>, HashMap<Oid, CommitIx>), GitError> {
    struct Raw<'a> {
        oid: Oid,
        parents: &'a str,
        empty_tree: bool,
        author_name: &'a str,
        author_email: &'a str,
        author_time: i64,
        author_date: &'a str,
        commit_time: i64,
        subject: &'a str,
    }

    let mut raw = Vec::new();
    for record in log.split(RECORD) {
        let record = record.trim_start_matches(['\n', '\r']);
        if record.is_empty() {
            continue;
        }
        let f: Vec<&str> = record.splitn(9, FIELD).collect();
        let [hash, parents, tree, an, ae, at, ad, ct, subject] = f[..] else {
            return Err(GitError::Parse(format!("bad log record {record:?}")));
        };
        raw.push(Raw {
            oid: Oid::from_hex(hash).ok_or_else(|| GitError::Parse(format!("bad hash {hash}")))?,
            parents,
            empty_tree: tree == EMPTY_TREE_SHA1 || tree == EMPTY_TREE_SHA256,
            author_name: an,
            author_email: ae,
            author_time: at.parse().unwrap_or(0),
            author_date: ad,
            commit_time: ct.parse().unwrap_or(0),
            subject,
        });
    }

    let by_oid: HashMap<Oid, CommitIx> = raw
        .iter()
        .enumerate()
        .map(|(i, r)| (r.oid, CommitIx(i as u32)))
        .collect();

    let commits = raw
        .into_iter()
        .map(|r| {
            let mut truncated = false;
            let parents = r
                .parents
                .split_ascii_whitespace()
                .filter_map(|p| {
                    let ix = Oid::from_hex(p).and_then(|o| by_oid.get(&o).copied());
                    truncated |= ix.is_none();
                    ix
                })
                .collect();
            Commit {
                oid: r.oid,
                parents,
                truncated,
                empty_tree: r.empty_tree,
                author_name: r.author_name.to_owned(),
                author_email: r.author_email.to_owned(),
                author_time: r.author_time,
                author_date: r.author_date.to_owned(),
                commit_time: r.commit_time,
                subject: r.subject.to_owned(),
            }
        })
        .collect();
    Ok((commits, by_oid))
}

fn parse_refs(
    out: &str,
    head_branch: Option<&str>,
    lookup: impl Fn(&Oid) -> Option<CommitIx>,
) -> Vec<GitRef> {
    let mut refs = Vec::new();
    for line in out.lines() {
        let f: Vec<&str> = line.split(FIELD).collect();
        let [full_name, obj_type, obj, peeled_type, peeled, symref] = f[..] else {
            continue;
        };
        if !symref.is_empty() {
            // e.g. refs/remotes/origin/HEAD -> origin/main: a duplicate label, skip it.
            continue;
        }
        let (annotated, commit_oid) = match (obj_type, peeled_type) {
            ("commit", _) => (false, obj),
            ("tag", "commit") => (true, peeled),
            // Trees, blobs, or tags of tags: nothing to draw.
            _ => continue,
        };
        let Some(target) = Oid::from_hex(commit_oid).and_then(|o| lookup(&o)) else {
            continue;
        };
        let (kind, name) = classify_ref(full_name);
        if kind == RefKind::Other && full_name.starts_with("refs/notes/") {
            continue;
        }
        refs.push(GitRef {
            full_name: full_name.to_owned(),
            name,
            kind,
            target,
            annotated,
            is_head: Some(full_name) == head_branch,
        });
    }
    refs
}

/// Classifies a full ref name and produces its display name.
pub fn classify_ref(full_name: &str) -> (RefKind, String) {
    let strip = |prefix: &str| full_name.strip_prefix(prefix).map(str::to_owned);
    if let Some(n) = strip("refs/heads/") {
        (RefKind::LocalBranch, n)
    } else if let Some(n) = strip("refs/remotes/") {
        (RefKind::RemoteBranch, n)
    } else if let Some(n) = strip("refs/tags/") {
        (RefKind::Tag, n)
    } else if full_name == "refs/stash" {
        (RefKind::Stash, "stash".to_owned())
    } else {
        (
            RefKind::Other,
            full_name
                .strip_prefix("refs/")
                .unwrap_or(full_name)
                .to_owned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_refs() {
        assert_eq!(
            classify_ref("refs/heads/feature/x"),
            (RefKind::LocalBranch, "feature/x".into())
        );
        assert_eq!(
            classify_ref("refs/remotes/origin/main"),
            (RefKind::RemoteBranch, "origin/main".into())
        );
        assert_eq!(
            classify_ref("refs/tags/v1.0"),
            (RefKind::Tag, "v1.0".into())
        );
        assert_eq!(classify_ref("refs/stash"), (RefKind::Stash, "stash".into()));
        assert_eq!(
            classify_ref("refs/pull/1/head"),
            (RefKind::Other, "pull/1/head".into())
        );
    }

    #[test]
    fn parses_log_records_and_links_parents() {
        let a = "a".repeat(40);
        let b = "b".repeat(40);
        let missing = "c".repeat(40);
        let tree = "d".repeat(40);
        let log = format!(
            "{b}\x1f{a} {missing}\x1f{tree}\x1fAnn\x1fann@x\x1f20\x1f2024-01-02 03:04\x1f21\x1fsecond\x1fwith sep\x1e\n\
             {a}\x1f\x1f{EMPTY_TREE_SHA1}\x1fBob\x1fbob@x\x1f10\x1f2024-01-01 00:00\x1f11\x1ffirst\x1e\n"
        );
        let (commits, by_oid) = parse_log(&log).unwrap();
        assert_eq!(commits.len(), 2);
        let second = &commits[by_oid[&Oid::from_hex(&b).unwrap()].ix()];
        assert_eq!(second.parents, vec![by_oid[&Oid::from_hex(&a).unwrap()]]);
        assert!(
            second.truncated,
            "missing parent marks the commit truncated"
        );
        assert_eq!(second.subject, "second\x1fwith sep");
        assert_eq!(second.author_time, 20);
        assert_eq!(second.author_date, "2024-01-02 03:04");
        assert_eq!(second.commit_time, 21);
        let first = &commits[1];
        assert!(first.parents.is_empty() && !first.truncated);
        assert!(first.empty_tree && !second.empty_tree);
    }
}
