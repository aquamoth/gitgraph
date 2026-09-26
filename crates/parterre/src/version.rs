//! The version string parterre reports, e.g. `0.3.0 (a1b2c3d)` for a release.
//!
//! `build.rs` includes this file and runs [`describe`] at build time; the app only reads the
//! result (`PARTERRE_VERSION`). The app compiles this module just for its tests, so it must not
//! use anything outside `std`. It isn't in `parterre-core` because the build script would then
//! have to compile all of core as a build dependency.

/// Where the sources being built come from.
#[derive(Debug)]
pub enum Source {
    /// A git checkout whose top level is the workspace root.
    Git(GitState),
    /// A crate made by `cargo package`, such as one downloaded from crates.io. Cargo records the
    /// commit it was packaged from in `.cargo_vcs_info.json` (see [`parse_vcs_info`]).
    Package(VcsInfo),
    /// Neither, e.g. a source archive without `.git`.
    Unknown,
}

/// What `git` says about the checkout being built.
#[derive(Debug)]
pub struct GitState {
    /// Abbreviated hash of `HEAD`.
    pub commit: String,
    /// The sources differ from `HEAD` (uncommitted changes).
    pub dirty: bool,
    /// Tags pointing at `HEAD`.
    pub tags: Vec<String>,
}

/// The commit a packaged crate was made from, as recorded in its `.cargo_vcs_info.json`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct VcsInfo {
    /// Abbreviated hash, or `None` if it was packaged outside git.
    pub commit: Option<String>,
    /// It was packaged with uncommitted changes (`cargo package --allow-dirty`).
    pub dirty: bool,
}

/// The version string for a build of package version `pkg_version`.
///
/// A build of exactly the released sources reports the plain version and commit,
/// `0.3.0 (a1b2c3d)`: a clean checkout of tag `v0.3.0`, or a packaged crate such as the one on
/// crates.io. Every other build is a dev build, marked with a `dev` pre-release and the commit
/// as build metadata: `0.3.0-dev+a1b2c3d(.dirty)`, or just `0.3.0-dev` without git.
///
/// The release workflow sets `release_tag`. Its version comes from the tag, and the build
/// fails unless that tag points at the commit being built and the checkout is clean.
pub fn describe(
    pkg_version: &str,
    release_tag: Option<&str>,
    source: &Source,
) -> Result<String, String> {
    let version = release_tag.map(parse_release_tag).transpose()?;
    let version = version.unwrap_or(pkg_version);
    let tag = format!("v{version}");
    if matches!(source, Source::Package(_)) && release_tag.is_some() && version != pkg_version {
        return Err(format!(
            "release tag {tag} doesn't match the packaged crate version {pkg_version}"
        ));
    }
    let (commit, dirty, released) = match source {
        Source::Git(git) => (Some(&git.commit), git.dirty, git.tags.contains(&tag)),
        Source::Package(info) => (info.commit.as_ref(), info.dirty, true),
        Source::Unknown => (None, false, false),
    };
    if let Some(tag) = release_tag {
        let Some(commit) = commit else {
            return Err(format!(
                "release tag {tag} given, but git can't tell which commit is being built"
            ));
        };
        if !released {
            return Err(format!(
                "release tag {tag} doesn't point at the commit being built ({commit})"
            ));
        }
        if dirty {
            return Err(format!(
                "release {tag} is being built from a checkout with local changes"
            ));
        }
    }
    if released && !dirty {
        return Ok(match commit {
            Some(commit) => format!("{version} ({commit})"),
            None => version.to_owned(),
        });
    }
    let sep = if pkg_version.contains('-') { '.' } else { '-' };
    let dev = format!("{pkg_version}{sep}dev");
    let Some(commit) = commit else {
        return Ok(dev);
    };
    let dirty = if dirty { ".dirty" } else { "" };
    Ok(format!("{dev}+{commit}{dirty}"))
}

/// Accept `vX.Y.Z` with an optional semver pre-release suffix. Build metadata is left out of
/// release tags so the same version can later be used as a Cargo package version.
pub fn parse_release_tag(tag: &str) -> Result<&str, String> {
    let Some(version) = tag.strip_prefix('v') else {
        return Err(format!(
            "invalid release tag {tag}: expected vX.Y.Z[-prerelease]"
        ));
    };
    let (numbers, pre) = version
        .split_once('-')
        .map_or((version, None), |(n, p)| (n, Some(p)));
    let valid_number = |n: &str| {
        !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && (n == "0" || !n.starts_with('0'))
    };
    if numbers.split('.').count() != 3
        || !numbers.split('.').all(valid_number)
        || pre.is_some_and(|p| {
            p.split('.').any(|part| {
                part.is_empty()
                    || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    || (part.bytes().all(|b| b.is_ascii_digit())
                        && part.len() > 1
                        && part.starts_with('0'))
            })
        })
    {
        return Err(format!(
            "invalid release tag {tag}: expected vX.Y.Z[-prerelease]"
        ));
    }
    Ok(version)
}

/// Reads the commit from the `.cargo_vcs_info.json` that `cargo package` writes, e.g.
/// `{"git": {"sha1": "a1b2c3d…", "dirty": true}, "path_in_vcs": "crates/parterre"}`.
/// `dirty` is only present when true. Hand-parsed, as the build script has no dependencies.
pub fn parse_vcs_info(json: &str) -> VcsInfo {
    let value_after = |key: &str| {
        let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
        let rest = rest.trim_start().strip_prefix(':')?.trim_start();
        Some(rest)
    };
    let commit = value_after("sha1").and_then(|rest| {
        let hex = rest.strip_prefix('"')?;
        let hex = &hex[..hex.find('"')?];
        (hex.len() >= 7 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hex[..7].to_owned())
    });
    let dirty = value_after("dirty").is_some_and(|rest| rest.starts_with("true"));
    VcsInfo { commit, dirty }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(commit: &str, dirty: bool, tags: &[&str]) -> Source {
        Source::Git(GitState {
            commit: commit.to_owned(),
            dirty,
            tags: tags.iter().map(|t| t.to_string()).collect(),
        })
    }

    fn package(commit: Option<&str>, dirty: bool) -> Source {
        Source::Package(VcsInfo {
            commit: commit.map(str::to_owned),
            dirty,
        })
    }

    #[test]
    fn dev_build_carries_commit_as_build_metadata() {
        let git = git("a1b2c3d", false, &[]);
        assert_eq!(describe("0.3.0", None, &git).unwrap(), "0.3.0-dev+a1b2c3d");
    }

    #[test]
    fn dev_build_with_local_changes_says_dirty() {
        let git = git("a1b2c3d", true, &[]);
        assert_eq!(
            describe("0.3.0", None, &git).unwrap(),
            "0.3.0-dev+a1b2c3d.dirty"
        );
    }

    #[test]
    fn dev_build_without_git_has_no_build_metadata() {
        // E.g. built from GitHub's source archive, which has no .git directory.
        assert_eq!(
            describe("0.3.0", None, &Source::Unknown).unwrap(),
            "0.3.0-dev"
        );
    }

    #[test]
    fn dev_build_of_a_prerelease_extends_its_prerelease() {
        // Semver allows one pre-release part; "0.3.0-rc.1-dev" would be a single odd identifier.
        let git = git("a1b2c3d", false, &[]);
        assert_eq!(
            describe("0.3.0-rc.1", None, &git).unwrap(),
            "0.3.0-rc.1.dev+a1b2c3d"
        );
    }

    #[test]
    fn release_build_shows_plain_version_and_commit() {
        let git = git("a1b2c3d", false, &["v0.3.0"]);
        assert_eq!(
            describe("0.3.0", Some("v0.3.0"), &git).unwrap(),
            "0.3.0 (a1b2c3d)"
        );
    }

    #[test]
    fn release_version_comes_from_tag() {
        let git = git("a1b2c3d", false, &["v0.5.0-rc1"]);
        assert_eq!(
            describe("0.4.0", Some("v0.5.0-rc1"), &git).unwrap(),
            "0.5.0-rc1 (a1b2c3d)"
        );
    }

    #[test]
    fn release_tag_must_be_a_version() {
        let git = git("a1b2c3d", false, &["vnot-a-version"]);
        assert!(describe("0.4.0", Some("vnot-a-version"), &git).is_err());
        assert!(describe("0.4.0", Some("v0.5.0-"), &git).is_err());
        assert!(describe("0.4.0", Some("v0.5.0-01"), &git).is_err());
    }

    #[test]
    fn release_tag_must_point_at_the_built_commit() {
        let git = git("a1b2c3d", false, &["v0.2.0"]);
        let err = describe("0.3.0", Some("v0.3.0"), &git).unwrap_err();
        assert!(err.contains("v0.3.0") && err.contains("a1b2c3d"), "{err}");
    }

    #[test]
    fn release_build_must_be_clean() {
        let git = git("a1b2c3d", true, &["v0.3.0"]);
        let err = describe("0.3.0", Some("v0.3.0"), &git).unwrap_err();
        assert!(err.contains("changes"), "{err}");
    }

    #[test]
    fn release_build_needs_git() {
        let err = describe("0.3.0", Some("v0.3.0"), &Source::Unknown).unwrap_err();
        assert!(err.contains("git"), "{err}");
    }

    #[test]
    fn clean_checkout_of_the_release_tag_shows_plain_version() {
        // E.g. a distribution building the tagged sources, or `cargo install --git --tag`.
        let git = git("a1b2c3d", false, &["v0.3.0"]);
        assert_eq!(describe("0.3.0", None, &git).unwrap(), "0.3.0 (a1b2c3d)");
    }

    #[test]
    fn checkout_of_the_release_tag_with_local_changes_is_a_dev_build() {
        let git = git("a1b2c3d", true, &["v0.3.0"]);
        assert_eq!(
            describe("0.3.0", None, &git).unwrap(),
            "0.3.0-dev+a1b2c3d.dirty"
        );
    }

    #[test]
    fn other_tags_dont_make_a_release() {
        let git = git("a1b2c3d", false, &["v0.2.0", "latest"]);
        assert_eq!(describe("0.3.0", None, &git).unwrap(), "0.3.0-dev+a1b2c3d");
    }

    #[test]
    fn packaged_crate_shows_plain_version() {
        // What `cargo install parterre` builds from crates.io.
        let pkg = package(Some("a1b2c3d"), false);
        assert_eq!(describe("0.3.0", None, &pkg).unwrap(), "0.3.0 (a1b2c3d)");
        let pkg = package(None, false);
        assert_eq!(describe("0.3.0", None, &pkg).unwrap(), "0.3.0");
    }

    #[test]
    fn crate_packaged_with_local_changes_is_a_dev_build() {
        let pkg = package(Some("a1b2c3d"), true);
        assert_eq!(
            describe("0.3.0", None, &pkg).unwrap(),
            "0.3.0-dev+a1b2c3d.dirty"
        );
    }

    #[test]
    fn release_workflow_may_build_the_packaged_crate() {
        // `cargo publish` in the release workflow compiles the package it made.
        let pkg = package(Some("a1b2c3d"), false);
        assert_eq!(
            describe("0.3.0", Some("v0.3.0"), &pkg).unwrap(),
            "0.3.0 (a1b2c3d)"
        );
        let err = describe("0.3.0", Some("v0.3.1"), &pkg).unwrap_err();
        assert!(err.contains("v0.3.1"), "{err}");
    }

    #[test]
    fn vcs_info_gives_abbreviated_commit_and_dirty_flag() {
        let clean = r#"{
  "git": {
    "sha1": "e6f2c0c0a1b2c3d4e5f60718293a4b5c6d7e8f90"
  },
  "path_in_vcs": "crates/parterre"
}"#;
        assert_eq!(
            parse_vcs_info(clean),
            VcsInfo {
                commit: Some("e6f2c0c".into()),
                dirty: false
            }
        );
        let dirty = r#"{"git":{"sha1":"e6f2c0c0a1b2c3d4e5f60718293a4b5c6d7e8f90","dirty":true},"path_in_vcs":""}"#;
        assert_eq!(
            parse_vcs_info(dirty),
            VcsInfo {
                commit: Some("e6f2c0c".into()),
                dirty: true
            }
        );
        assert_eq!(parse_vcs_info(r#"{"path_in_vcs": ""}"#), VcsInfo::default());
        assert_eq!(parse_vcs_info("not json"), VcsInfo::default());
    }
}
