# Releasing

Releases are tag-driven. The version lives in one place, `version` under `[workspace.package]`
in the root `Cargo.toml`, and both crates inherit it.

1. In a PR, bump `version` (semver) and merge it once CI passes.
2. Tag the merge commit on `main` and push the tag:

   ```sh
   git switch main && git pull
   git tag -a v0.3.0 -m "gitgraph 0.3.0"
   git push origin v0.3.0
   ```

3. `.github/workflows/release.yml` tests and builds on Linux, Windows and macOS (Apple silicon
   and Intel), then publishes a GitHub Release. The release has one archive per target
   (`gitgraph-0.3.0-<target>.tar.gz`, or `.zip` for Windows) and a `SHA256SUMS` file. A tag
   with a pre-release part, such as `v0.3.0-rc.1`, publishes a pre-release.

The build fails if the tag isn't `v` + the `Cargo.toml` version, doesn't point at the commit
being built, or the sources have local changes. In that case delete the tag
(`git push origin :refs/tags/v0.3.0`), fix things and tag again.

## Version strings

`gitgraph --version` and the Help menu show which build is running:

| Build | Version |
|---|---|
| Release workflow | `gitgraph 0.3.0 (a1b2c3d)` |
| Anything else from a git checkout | `gitgraph 0.3.0-dev+a1b2c3d` |
| … with uncommitted changes to the sources (`crates/`, Cargo files) | `gitgraph 0.3.0-dev+a1b2c3d.dirty` |
| Without git (e.g. from a source archive) | `gitgraph 0.3.0-dev` |

Dev builds carry a `dev` pre-release and the commit as semver build metadata. A local build of
a tagged commit is a dev build too, so only the release workflow's binaries show a plain
version. The workflow gets them by setting `GITGRAPH_RELEASE_TAG` to the tag. The logic is in
`crates/gitgraph/src/version.rs`, which `crates/gitgraph/build.rs` runs.
