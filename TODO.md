# TODO

## Open questions (HITL)

_Decisions I made on my own that you may want to overrule. Newest last._

1. **Fidelity quirks in the default ("Labelled commits") mode.** TortoiseGit uses
   `git log --simplify-by-decoration`, so it inherits some of git's simplifications:
   - A `--no-ff` merge whose first parent is an ancestor of its second parent is folded away.
   - A merge that brings in a history whose root has an **empty tree** (svn imports,
     `--allow-empty` initial commits) is folded away, along with the root.

   gitgraph copies this exactly. On `Cosmo/Apps` its node set is identical to git's: 266 nodes.
   The "Branchings and merges" and "All commits" modes show the real topology. Keep this?
2. **Stash** is shown, as in TortoiseGit, but only as a single edge to its base commit. The
   internal index and untracked-files snapshot commits are hidden.
3. **`origin/HEAD`**-style symbolic refs are hidden, because they duplicate `origin/main`.
   TortoiseGit shows them.
4. **Other refs** such as `refs/t3/*` are hidden by default, with a toggle to show them.
   TortoiseGit doesn't decorate them either.

## Planned

- [ ] GUI: window, pan/zoom, node rendering with TortoiseGit colours, edges with arrows.
- [ ] View options panel: the TortoiseGit toggles, plus direction and ranking.
- [ ] Drag a node so the rest of the graph follows (spider web): prototype several models.
- [ ] Tooltips, search (Ctrl+F), jump to HEAD, overview map.
- [ ] Windows build and CI.

## Done

- [x] Workspace scaffold, lints, release profile.
- [x] Git loading through the git CLI (about 100 ms for 15k commits).
- [x] Revision-graph reduction in TortoiseGit's three modes, with integration tests.
- [x] Layered layout: network-simplex ranking, median crossing reduction, and L1 coordinate
      assignment by isotonic regression. About 1 ms for 277 nodes and 200 ms for 15k.
