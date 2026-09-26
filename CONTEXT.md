# parterre

A standalone viewer for a git repository's revision graph, in the style of TortoiseGit, with a
small set of git actions reached from the graph.

## Language

### The graph

**Node**:
A commit that the revision graph draws as a box, because a ref points at it or because the
chosen mode keeps it (branchings, merges, or every commit).
_Avoid_: vertex, box

**Edge**:
A line from a node to a parent node, standing for that parent link and any commits collapsed
into it.
_Avoid_: link, arrow

### The log

**Log window**:
The separate window that lists commits one per row, shows the selected commit's details and
its changed files. Opened with *Show log*.
_Avoid_: log dialog, history view

**Log query**:
What the log window lists: the tips to walk back from and the commits to leave out (a
two-node range). It knows nothing about the graph.
_Avoid_: log filter, range spec

**Log layout**:
One of a fixed set of arrangements of the log window's three panes (commits, details, changed
files): stacked (A), side by side (B), details and files below (C), files on the right (D).
_Avoid_: view, perspective, docking

**Changed files**:
The files a commit changed compared with its first parent, with their status and line counts.
_Avoid_: file list, diff, changeset
