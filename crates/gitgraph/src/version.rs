//! The version string gitgraph reports, e.g. `0.3.0 (a1b2c3d)` for a release.
//!
//! `build.rs` includes this file and runs [`describe`] at build time; the app only reads the
//! result (`GITGRAPH_VERSION`). The app compiles this module just for its tests, so it must not
//! use anything outside `std`.

/// What `git` says about the checkout being built.
#[derive(Debug)]
pub struct GitState {
    /// Abbreviated hash of `HEAD`.
    pub commit: String,
    /// Tracked files differ from `HEAD`.
    pub dirty: bool,
    /// Tags pointing at `HEAD`.
    pub tags: Vec<String>,
}

/// The version string for a build of package version `pkg_version`.
///
/// A release build (`release_tag` set, by the release workflow) reports the plain version and
/// commit, `0.3.0 (a1b2c3d)`, and fails unless the tag is `v` + `pkg_version`, points at the
/// commit being built, and the checkout is clean. Every other build is a dev build, marked with
/// a `dev` pre-release and the commit as build metadata: `0.3.0-dev+a1b2c3d(.dirty)`, or just
/// `0.3.0-dev` without git (e.g. from a source archive).
pub fn describe(
    pkg_version: &str,
    release_tag: Option<&str>,
    git: Option<&GitState>,
) -> Result<String, String> {
    if let Some(tag) = release_tag {
        if tag.strip_prefix('v') != Some(pkg_version) {
            return Err(format!(
                "release tag {tag} doesn't match the version in Cargo.toml ({pkg_version}); \
                 expected tag v{pkg_version}"
            ));
        }
        let Some(git) = git else {
            return Err(format!(
                "release tag {tag} given, but git can't tell which commit is being built"
            ));
        };
        if !git.tags.iter().any(|t| t == tag) {
            return Err(format!(
                "release tag {tag} doesn't point at the commit being built ({})",
                git.commit
            ));
        }
        if git.dirty {
            return Err(format!(
                "release {tag} is being built from a checkout with local changes"
            ));
        }
        return Ok(format!("{pkg_version} ({})", git.commit));
    }
    let sep = if pkg_version.contains('-') { '.' } else { '-' };
    let dev = format!("{pkg_version}{sep}dev");
    let Some(git) = git else {
        return Ok(dev);
    };
    let dirty = if git.dirty { ".dirty" } else { "" };
    Ok(format!("{dev}+{}{dirty}", git.commit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(commit: &str, dirty: bool, tags: &[&str]) -> GitState {
        GitState {
            commit: commit.to_owned(),
            dirty,
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn dev_build_carries_commit_as_build_metadata() {
        let git = git("a1b2c3d", false, &[]);
        assert_eq!(
            describe("0.3.0", None, Some(&git)).unwrap(),
            "0.3.0-dev+a1b2c3d"
        );
    }

    #[test]
    fn dev_build_with_local_changes_says_dirty() {
        let git = git("a1b2c3d", true, &[]);
        assert_eq!(
            describe("0.3.0", None, Some(&git)).unwrap(),
            "0.3.0-dev+a1b2c3d.dirty"
        );
    }

    #[test]
    fn dev_build_without_git_has_no_build_metadata() {
        // E.g. built from a source archive, which has no .git directory.
        assert_eq!(describe("0.3.0", None, None).unwrap(), "0.3.0-dev");
    }

    #[test]
    fn dev_build_of_a_prerelease_extends_its_prerelease() {
        // Semver allows one pre-release part; "0.3.0-rc.1-dev" would be a single odd identifier.
        let git = git("a1b2c3d", false, &[]);
        assert_eq!(
            describe("0.3.0-rc.1", None, Some(&git)).unwrap(),
            "0.3.0-rc.1.dev+a1b2c3d"
        );
    }

    #[test]
    fn release_build_shows_plain_version_and_commit() {
        let git = git("a1b2c3d", false, &["v0.3.0"]);
        assert_eq!(
            describe("0.3.0", Some("v0.3.0"), Some(&git)).unwrap(),
            "0.3.0 (a1b2c3d)"
        );
    }

    #[test]
    fn release_tag_must_match_package_version() {
        let git = git("a1b2c3d", false, &["v0.3.1"]);
        let err = describe("0.3.0", Some("v0.3.1"), Some(&git)).unwrap_err();
        assert!(err.contains("v0.3.1") && err.contains("0.3.0"), "{err}");
    }

    #[test]
    fn release_tag_must_point_at_the_built_commit() {
        let git = git("a1b2c3d", false, &["v0.2.0"]);
        let err = describe("0.3.0", Some("v0.3.0"), Some(&git)).unwrap_err();
        assert!(err.contains("v0.3.0") && err.contains("a1b2c3d"), "{err}");
    }

    #[test]
    fn release_build_must_be_clean() {
        let git = git("a1b2c3d", true, &["v0.3.0"]);
        let err = describe("0.3.0", Some("v0.3.0"), Some(&git)).unwrap_err();
        assert!(err.contains("changes"), "{err}");
    }

    #[test]
    fn release_build_needs_git() {
        let err = describe("0.3.0", Some("v0.3.0"), None).unwrap_err();
        assert!(err.contains("git"), "{err}");
    }
}
