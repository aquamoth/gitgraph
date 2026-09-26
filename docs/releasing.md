# Releasing

GitHub releases are tag-driven. The tag supplies the version shown by the binary and in the
release filenames. Tag a clean commit on `main`; the root `Cargo.toml` and
`Cargo.lock` do not need a version bump for a GitHub release.

1. Tag the chosen commit on `main` and push the tag:

   ```sh
   git switch main && git pull
   git tag -a v0.5.0-rc1 -m "parterre 0.5.0-rc1"
   git push origin v0.5.0-rc1
   ```

2. `.github/workflows/release.yml` tests and builds on Linux (in an Ubuntu 22.04 container, so
   the binary runs on glibc 2.35 and newer), Windows and macOS (Apple silicon and Intel), then
   publishes a GitHub Release. The release has one archive per target
   (`parterre-0.5.0-rc1-<target>.tar.gz`, or `.zip` for Windows) and a `SHA256SUMS` file. Each
   archive holds the binary, the README, `LICENSE`, `NOTICE` and `THIRD-PARTY-NOTICES.html`.
   Windows also gets an installer built from the same files,
   `parterre-0.5.0-rc1-x86_64-pc-windows-msvc.msi` (see
   [building.md](building.md#windows-installer)). A tag with a pre-release part publishes a
   pre-release; its MSI has version `0.5.0`, since MSI versions are numbers only.

The build fails if the tag is not `vX.Y.Z` with an optional pre-release suffix, does not point
at the commit being built, or the sources have local changes. In that case delete the tag
(`git push origin :refs/tags/v0.5.0-rc1`), fix things and tag again.

## crates.io

Cargo requires a package version in `Cargo.toml` and an equal version on the `parterre-core`
dependency. After the GitHub release workflow passes, use Python 3.11 or newer to generate a
separate checkout from the tag:

```sh
scripts/prepare-crates-release.py v0.5.0-rc1
```

The script prints the checkout path and publish command. It updates both versions and the two
workspace entries in `Cargo.lock`, verifies them with `cargo metadata --locked`, and makes a
local commit so `cargo publish` sees clean sources. The commit exists only in that disposable
checkout; the pushed tag and `main` still point at the same original commit. From the generated
checkout, run `cargo publish --workspace` with a crates.io API token (`cargo login`). Until the
publish job of #20 exists this is done by hand. The first publish of each crate always is.
A version on crates.io can be yanked but never replaced, so check the generated versions before
publishing. The published crate's recorded commit is the local packaging commit; the GitHub
binary reports the tagged source commit.

## Version strings

`parterre --version` and the foot of the ☰ menu show which build is running:

| Build | Version |
|---|---|
| Release workflow for `v0.5.0-rc1` | `parterre 0.5.0-rc1 (a1b2c3d)` |
| A clean checkout of a tag matching the Cargo version | `parterre 0.4.0 (a1b2c3d)` |
| A clean checkout of `v0.5.0-rc1` built without the release workflow | `parterre 0.4.0-dev+a1b2c3d` |
| The published crate (`cargo install parterre --version 0.5.0-rc1`) | `parterre 0.5.0-rc1 (b4c5d6e)` |
| Anything else from a git checkout | `parterre 0.4.0-dev+a1b2c3d` |
| … with uncommitted changes to the sources (`crates/`, Cargo files) | `parterre 0.4.0-dev+a1b2c3d.dirty` |
| Without git (e.g. from GitHub's source archive) | `parterre 0.4.0-dev` |

A plain version means a clean release build. The published crate has no `.git`; `cargo package`
records its packaging commit in `.cargo_vcs_info.json`, which `build.rs` reads. Git only counts when its
top level is the workspace root, so sources unpacked inside some other repository don't take
that repository's commit. Dev builds carry a `dev` pre-release and the commit as semver build
metadata. The release workflow sets `PARTERRE_RELEASE_TAG` to the tag, which supplies the binary
version and makes anything but a clean build of that tag fail. The logic is in
`crates/parterre/src/version.rs`, which `crates/parterre/build.rs` runs.
