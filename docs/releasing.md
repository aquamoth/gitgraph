# Releasing

Releases are tag-driven. The version lives under `[workspace.package]` in the root
`Cargo.toml`, and both crates inherit it. The `parterre-core` dependency in the same file repeats
it (`version = "=X.Y.Z"`, which crates.io needs); the build fails until the two agree.

1. In a PR, bump both (semver) and merge it once CI passes.
2. Tag the merge commit on `main` and push the tag:

   ```sh
   git switch main && git pull
   git tag -a v0.3.0 -m "parterre 0.3.0"
   git push origin v0.3.0
   ```

3. `.github/workflows/release.yml` tests and builds on Linux (in an Ubuntu 22.04 container, so
   the binary runs on glibc 2.35 and newer), Windows and macOS (Apple silicon and Intel), then
   publishes a GitHub Release. The release has one archive per target
   (`parterre-0.3.0-<target>.tar.gz`, or `.zip` for Windows) and a `SHA256SUMS` file. Each
   archive holds the binary, the README, `LICENSE`, `NOTICE` and `THIRD-PARTY-NOTICES.html`.
   A tag with a pre-release part, such as `v0.3.0-rc.1`, publishes a pre-release.

The build fails if the tag isn't `v` + the `Cargo.toml` version, doesn't point at the commit
being built, or the sources have local changes. In that case delete the tag
(`git push origin :refs/tags/v0.3.0`), fix things and tag again.

4. Publish both crates to crates.io from the tag: `git switch --detach v0.3.0`, then
   `cargo publish --workspace`. A version on crates.io can be yanked but never replaced, so
   this comes after the release workflow has passed. Until the publish job of #20 exists this
   is done by hand, with a crates.io API token (`cargo login`). The very first publish of each
   crate always is.

Where else parterre is published, and why, is in [distribution.md](distribution.md).

## Version strings

`parterre --version` and the foot of the ☰ menu show which build is running:

| Build | Version |
|---|---|
| Release workflow, or any clean checkout of the tag `v0.3.0` | `parterre 0.3.0 (a1b2c3d)` |
| The published crate (`cargo install parterre`) | `parterre 0.3.0 (a1b2c3d)` |
| Anything else from a git checkout | `parterre 0.3.0-dev+a1b2c3d` |
| … with uncommitted changes to the sources (`crates/`, Cargo files) | `parterre 0.3.0-dev+a1b2c3d.dirty` |
| Without git (e.g. from GitHub's source archive) | `parterre 0.3.0-dev` |

A plain version means exactly the released sources were built, whoever built them. The
published crate has no `.git`; `cargo package` records the commit in `.cargo_vcs_info.json`,
which `build.rs` reads. git only counts when its top level is the workspace root, so sources
unpacked inside some other repository don't take that repository's commit. Dev builds carry a
`dev` pre-release and the commit as semver build metadata. The release workflow sets
`PARTERRE_RELEASE_TAG` to the tag, which makes anything but a clean build of that tag fail.
The logic is in `crates/parterre/src/version.rs`, which `crates/parterre/build.rs` runs.
