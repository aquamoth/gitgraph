# parterre-core

The GUI-free core of [parterre](https://crates.io/crates/parterre), a TortoiseGit-style revision
graph viewer: loading a repository through the `git` command-line tool, reducing it to a
revision graph, laying it out, and the physics for dragging nodes.

It is published only because `parterre` depends on it. It is internal to parterre and makes no
stability promises: any release may change its API. Its version always equals parterre's.

GNU General Public License, version 3 only, with the additional terms in
[NOTICE](https://github.com/aquamoth/parterre/blob/main/NOTICE).
