# TortoiseGit Revision Graph: what the context-menu items do

Research notes for matching the revision graph's right-click menu in parterre. Everything comes
from reading source at pinned commits. Where a statement is my own reading of the code rather
than something the code or docs say outright, it is marked **(derived)**. Things I could not
verify are marked **(unverified)**.

## Sources (pinned)

| Short name | What | Permalink base |
|---|---|---|
| TG | TortoiseGit `master` @ `acc10fc2` | https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/ |
| RGW | `TG/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp` | (same base) |
| RGF | `TG/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp` | (same base) |
| AU | `TG/src/TortoiseProc/AppUtils.cpp` | (same base) |
| GIT | `TG/src/Git/Git.cpp` | (same base) |
| POT | `TG/Languages/Tortoise.pot` (English UI strings) | (same base) |
| RC | `TG/src/Resources/TortoiseProcENG.rc` (dialog layouts) | (same base) |
| GITSRC | upstream git `master` @ `0f8e75ab` | https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/ |
| GFW-PKG | git-for-windows/MINGW-packages @ `c8d066fd` | https://github.com/git-for-windows/MINGW-packages/blob/c8d066fd85562a81dde31f59cae3e25b8b1317c6/ |
| GFW-BX | git-for-windows/build-extra @ `c8241c75` | https://github.com/git-for-windows/build-extra/blob/c8241c75c7c06d6db94ed6a8dd9b8ca76d1ed842/ |
| BREW | Homebrew/homebrew-core @ `461ceeaa` | https://github.com/Homebrew/homebrew-core/blob/461ceeaa5208cb5dbb8c8653e4de59465b836d2f/ |
| FED | Fedora `rpms/git` rawhide @ `a40d40b9` | https://src.fedoraproject.org/rpms/git/blob/a40d40b977bd5d619ff8f99c78674383cf09a5d6/f/git.spec |

## 0. Shared building blocks

**Selection.** A plain click selects one node (`m_SelectedEntry1`). A Ctrl+click adds a second
(`m_SelectedEntry2`), so node 1 is the node picked first and node 2 the node picked second
([RGW#L467-L496](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L467-L496)).
A right-click on an unselected node replaces a single selection with that node. When two nodes
are selected and the right-click lands on neither, no menu opens
([RGW#L993-L1017](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L993-L1017)).
No menu opens while the graph is still loading
([RGW#L1122-L1123](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1122-L1123)).

**The ref map.** `m_HashMap` maps each commit to its **full** ref names: `refs/heads/x`,
`refs/remotes/origin/x`, `refs/tags/v1`, and `refs/tags/v1^{}` for annotated tags (the tag is
peeled to its commit and `^{}` is appended). Each list is sorted alphabetically, which puts
`refs/heads/*` before `refs/remotes/*` before `refs/tags/*`
([GIT#L2189-L2256](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L2189-L2256);
loaded by `ReloadHashMap`,
[RevisionGraphWnd.h#L116-L124](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.h#L116-L124)).

- `GetFriendRefName(node)` returns the node's **first** full ref name, or its full 40-hex hash
  if the node has no refs
  ([RGF#L401-L410](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L401-L410)).
  Browse, Compare and Unified diff all use it.
- `GetFriendRefNames(node, exclude, type)`: with no type filter it returns the **full** names.
  With a type filter it returns **short** names (`x`, `origin/x`, `v1`). `exclude` drops any ref
  whose short name equals the current branch's name, whatever the ref's type
  ([RGF#L412-L435](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L412-L435);
  type classification in
  [GIT#L3016-L3073](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L3016-L3073)).

**Out-of-process commands.** Show log, Browse, and the Compare items don't open a window in the
graph's own process. They build a `/command:...` line and start a **new `TortoiseGitProc.exe`
process** without waiting for it, adding `/hwnd:` and `/groupuuid:`
([CommonAppUtils.cpp#L176-L191](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Utils/CommonAppUtils.cpp#L176-L191)).
So those windows are separate and non-modal as far as the graph is concerned. Switch and Delete
run inside the graph's process and are modal.

**The path.** The graph's `m_sPath` is the working-tree root (`g_Git.m_CurrentDir`)
([RevisionGraphCommand.cpp#L28](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/RevisionGraphCommand.cpp#L28)).
TortoiseGitProc makes `/path` relative to the repo root, so the root becomes an empty relative
path ([Command.h#L56-L66](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/Command.h#L56-L66)).
That means log and compare cover the whole repository, with no path filter **(derived)**.

## 1. Menu layout and when items appear

Built in `OnContextMenu`
([RGW#L1120-L1218](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1120-L1218)).
A separator goes in wherever the command-ID group (`id & 0xff00`) changes
([RGW#L49-L70](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L49-L70),
[RGW#L1041-L1061](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1041-L1061)).

**One node selected**, in this order:

| Item (English label, POT) | Condition |
|---|---|
| Show &log | always |
| &Browse repository | always |
| `&Switch/Checkout to "<b>"` | exactly one local branch on the node besides the current branch |
| `&Switch/Checkout to` ▸ submenu of branches | two or more local branches besides the current one |
| `Sw&itch/Checkout to this...` | no such local branch, and the node has a remote-tracking branch, a lightweight tag or an annotated tag (checked in that priority order). The first ref of the first matching type is preselected in the dialog |
| Copy ref names | always |
| `&Delete <fullref>` | exactly one ref besides the current branch |
| `&Delete branch/tag` ▸ each full ref, then `All` | two or more refs besides the current branch |
| Compare &HEAD revisions | always |
| Unified &diff of HEAD revisions | always |
| Compare with &working tree | always |

**Two nodes selected:** Show &log, &Compare revisions, &Unified diff.

Labels: POT [L1186-L1187](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L1186-L1187)
(Switch/Checkout to), [L9070-L9071](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L9070-L9071)
(Switch/Checkout to this...), [L495-L496](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L495-L496)
(Delete branch/tag), [L479-L480](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L479-L480)
(&Delete), [L2815-L2816](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L2815-L2816)
(Compare with working tree), [L383-L384](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L383-L384)
(Browse repository); RC [L3621](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3621)
(All), [L3677-L3684](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3677-L3684)
(Compare/Unified diff labels), [L3893](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3893)
(Copy ref names), [L3946](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3946) (Show log).

Things the menu does **not** check **(derived from the code above)**:
- Nothing is hidden or greyed when the node is HEAD. "Compare HEAD revisions" still shows on
  the HEAD node, and there are no disabled (`MF_GRAYED`) items at all. The only HEAD-specific
  effect is that the **current branch** is left out of the Switch and Delete lists.
- A node whose only ref is the current branch, or a node with no refs at all (a
  branching/merge point), gets **no** Switch item. No item offers checking out a bare hash.
- There is no bare-repository check in the menu. Switch and "Compare with working tree" still
  appear, and would fail inside git. `SwitchCommand` requires a working tree (the default
  `PathRequirement::WorkingTreeRequired`,
  [Command.h#L96](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/Command.h#L96)),
  but the graph calls `CAppUtils` directly and skips that check. log/repobrowser/showcompare
  accept bare repos (`WorkingTreeOrBareRepoRequired`, e.g.
  [ShowCompareCommand.h#L33](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/ShowCompareCommand.h#L33)).

## 2. Show log

`DoShowLog` ([RGW#L1063-L1082](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1063-L1082)):

- One node: `/command:log /path:<root> /endrev:<hash> /rev:<hash>`, where `hash` is the node's
  full commit hash (not a ref name).
- Two nodes: `/command:log /path:<root> /startrev:<hash1> /endrev:<hash2>`.

`LogCommand` turns these into a range: `startrev` gives `"<start>.."` and `endrev` is appended,
so a single node shows `<hash>` (that commit and its ancestors) and two nodes show
`<hash1>..<hash2>`. `/rev` only chooses which row is selected and scrolled to
([LogCommand.cpp#L30-L55, L73, L85](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/LogCommand.cpp#L30-L85);
[LogDlg.cpp#L225-L243, L446-L454](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L225-L254)).
Any non-empty range turns off the log's "All branches" mode and removes the commit-count limit
([LogDlg.cpp#L234-L243](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L234-L243)).
The range is asymmetric: node 1 is excluded, node 2 is shown with its ancestors. If you pick the
newer node first, the range comes out empty **(derived)**. The window is the standard Log
dialog, in its own process. Nothing is refreshed afterwards.

## 3. Browse repository

`DoBrowseRepo` ([RGW#L1089-L1100](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1089-L1100)):
`/command:repobrowser /path:<root> /rev:<GetFriendRefName(node)>`. The revision is the node's
first full ref name (for example `refs/heads/main`), or its hash if it has none.
`RepositoryBrowserCommand` opens `CRepositoryBrowser(rev)`, falling back to HEAD when no `/rev`
is given ([RepositoryBrowserCommand.cpp#L23-L33](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/RepositoryBrowserCommand.cpp#L23-L33)).
The window ("Repository Browser") has a folder tree, a file list, and a "Revision:" button for
choosing a different revision
([RC#L325-L340](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L325-L340)).
Its file context menu offers Open, Open with, Compare with working tree, Show log, Blame,
Save as, Revert to this revision, Prepare/compare diff, Copy path, and Copy hash
([RepositoryBrowser.cpp#L736-L812](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RepositoryBrowser.cpp#L736-L812)).

## 4. Switch/Checkout

### 4a. Direct: `Switch/Checkout to "<branch>"` and the branch submenu

Choosing an entry calls `DoSwitch(shortName)` and then `m_parent->UpdateFullHistory()`
([RGW#L1246-L1259](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1246-L1259)).
`DoSwitch` calls `CAppUtils::PerformSwitch(hwnd, rev)` with the default arguments: no force, no
new branch, no merge ([RGW#L1084-L1087](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1084-L1087)).

- **No dialog and no confirmation.** A modal `CProgressDlg` opens straight away and runs
  `git.exe checkout --end-of-options <branch> --`
  ([AU#L1288-L1310](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L1288-L1310)).
- **Dirty working trees are left to git.** Nothing is checked beforehand, so git carries
  uncommitted changes across or refuses. On **failure**, the progress dialog offers these
  buttons: "Stash changes" (StashSave), "&Retry", and "Checkout with merge" (reruns with
  `--merge`). On **success** it offers "Update Submodules" (only if the repo has submodules),
  "&Merge..." (merge the previous branch), "&Pull..." (if on a branch now) and "&Commit..."
  ([AU#L1318-L1375](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L1318-L1375);
  labels in POT [L8924-L8925](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L8924-L8925),
  [L1082-L1083](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L1082-L1083),
  [L2460-L2461](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/Languages/Tortoise.pot#L2460-L2461)).
  With `--merge`, conflicts count as a failure and offer a "Resolve" button (L1352-L1374).
- The progress dialog stays open until the user closes it, because the default
  `AutoCloseGitProgress` is 0 (AUTOCLOSE_NO)
  ([ProgressDlg.cpp#L58-L70](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/ProgressDlg.cpp#L58-L70)).
- **After:** the graph reloads fully (`UpdateFullHistory`: refetch and relayout), whatever the
  outcome ([RevisionGraphDlg.cpp#L646-L651](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlg.cpp#L646-L651)).

### 4b. `Switch/Checkout to this...` (remote branch or tag)

`CAppUtils::Switch(hwnd, shortName)` runs, then `UpdateFullHistory()`
([RGW#L1260-L1270](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1260-L1270)).
`Switch` opens the modal **`CGitSwitchDlg`** with `m_initialRefName` set, and on OK calls
`PerformSwitch` with the dialog's choices
([AU#L1266-L1286](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L1266-L1286)).
The resulting command is
`git checkout [-f] [--track|--no-track] [-b|-B <new>] [--merge] --end-of-options <ref> --`
([AU#L1293-L1310](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L1293-L1310)).
Failure and success handling are the same as in 4a.

The dialog is titled "Switch/Checkout"
([RC#L1259-L1284](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L1259-L1284)):
- **Switch To:** radio buttons Branch (combo, plus "..." to browse refs), Tag (combo), and
  Commit (editable combo, plus "..." to pick from the log).
- **Option:** "Create &New Branch" [name box]; "Overwrite working tree changes (&force)" → `-f`;
  "&Merge" → `--merge`; "T&rack", a three-state box that gives `--track`, `--no-track`, or
  neither (tooltip [RC#L4912-L4913](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L4912-L4913));
  "&Override branch if exists" → `-B` instead of `-b`.

How the preselected ref sets the defaults:
- `SelectRef` expands the short name to a full ref. `refs/remotes/...` selects the **Branch**
  radio with `remotes/origin/x`. `refs/tags/...` selects the **Tag** radio. Anything else goes
  to **Commit**
  ([ChooseVersion.h#L165-L205](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/ChooseVersion.h#L165-L205)).
- `SetDefaultName` ([GitSwitchDlg.cpp#L173-L223](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitSwitchDlg.cpp#L173-L223)):
  - **Remote branch** `remotes/<remote>/x`: Create New Branch is **checked**, the name is `x`,
    and Track is enabled and indeterminate, so no track flag is passed and git's default
    `branch.autoSetupMerge` applies. The default command is
    `git checkout -b x remotes/origin/x` **(derived)**.
  - **Tag** `v1`: the name defaults to `Branch_v1`. Create New Branch is checked when the
    registry value `SwitchToTagNewBranch` is set, which it is by default
    ([GitSwitchDlg.cpp#L41-L42](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitSwitchDlg.cpp#L41-L42)).
    Unchecking it gives a detached-HEAD checkout of the tag. The dialog remembers that choice
    (L164-L167).
- OK validation: the branch name must be valid; an existing branch is refused unless Override
  is checked; a tag with the same name asks Continue/Abort
  ([GitSwitchDlg.cpp#L135-L172](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitSwitchDlg.cpp#L135-L172)).

## 5. Copy ref names

`DoCopyRefs` ([RGW#L1102-L1118](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1102-L1118))
copies **every** ref on the node, the current branch included (no exclude), as **full** names,
one per line with **CRLF** separators and no trailing newline. The order is the sorted map order.
Annotated tags appear as `refs/tags/<name>^{}`, with the peel suffix (see §0). A node with no
refs copies its full 40-hex hash. The text goes to the clipboard through
`WriteAsciiStringToClipboard`.

## 6. Delete

Menu handler: [RGW#L1274-L1302](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1274-L1302).
The work is done by **`CAppUtils::DeleteRef(CWnd*, const CString& fullRef)`**
([AU#L3788-L3870](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L3788-L3870)),
which calls `CGit::DeleteRef` ([GIT#L3369-L3428](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L3369-L3428))
and `CGit::DeleteRemoteRefs` ([GIT#L2089-L2134](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L2089-L2134)).

- **Remote-tracking ref** `refs/remotes/<r>/<b>`: a three-button message box. The text is
  "The branch "refs/remotes/…" is a remote-tracking branch which locally represents a remote
  branch. Do you really want to delete it?"
  ([RC#L4531-L4535](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L4531-L4535)). The buttons:
  1. "**Delete branch on remote && local remote-tracking branch**": splits off the remote name
     at the **first** `/`, then pushes a deletion, `git push -- <r> :refs/heads/<b>`, or the
     libgit2 `git_remote_push` equivalent, while a "Deleting remote refs..." progress window is
     shown. The local tracking ref is not deleted separately; git removes it when the push
     succeeds **(derived)**. A push error shows a message box, but the function **still returns
     true**, so the graph refreshes anyway (AU L3796-L3815).
  2. "**Delete &local remote-tracking branch**": `git branch -r -D <r>/<b>`, or libgit2
     `git_branch_delete`. Nothing is pushed.
  3. "A&bort".
- **Local branch** `refs/heads/x`: a two-button box, "Do you really want to delete
  "refs/heads/x"?", with Delete and Abort
  ([RC#L4537](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L4537)).
  If the branch is not an ancestor of HEAD (`!IsFastForward(ref, "HEAD")`), the text gains
  "This branch is not fully merged into HEAD."
  ([RC#L4887-L4888](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L4887-L4888)).
  Deletion is **always forced**: `git branch -D`, or libgit2 `git_branch_delete`, which does not
  check whether the branch is merged. There is no `-d` path; the warning in the box is the only
  safeguard.
- **Tag** `refs/tags/v1` or `refs/tags/v1^{}`: the same two-button box (no merge warning). The
  `^{}` is stripped, then `git tag -d v1` or `git_tag_delete` runs. This is **local only**; no
  remote tag is ever deleted.
- **refs/stash** (appears only if the stash commit is on the graph): "Do you really want to
  delete ALL %d stash?" with "&Delete" (`git stash clear`), "Drop &one stash"
  (`git stash drop refs/stash@{0}`) and "A&bort".
- Other ref types, such as notes, fail with "unsupported reference type" on the git.exe path
  (GIT L3410-L3413).
- **"All"**: calls `DeleteRef` once per ref, so each ref gets its **own** confirmation. It
  stops at the first Abort or failure. If at least one ref was deleted the graph reloads;
  otherwise it doesn't.
- **After** a single successful delete: `UpdateFullHistory()`. Abort or failure: no refresh.

## 7. Compare HEAD revisions / Compare revisions / Compare with working tree

All three call `CompareRevs(revTo)`
([RGF#L437-L455](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L437-L455)),
which launches
`/command:showcompare /path:<root> /revision1:<FriendRef(node1)> /revision2:<X> [/alternative]`:

| Item | revision1 | revision2 (X) |
|---|---|---|
| Compare HEAD revisions | node's first full ref, or its hash | literal `HEAD` |
| Compare revisions | node 1's ref | node 2's ref |
| Compare with working tree | node's ref | `0000…0000` (40 zeros, `CGitHash().ToString()` = `GIT_REV_ZERO`, the working-tree marker; [GitRev.h#L101](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitRev.h#L101)) |

The menu dispatch is at [RGW#L1231-L1234, L1306-L1313](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphWnd.cpp#L1306-L1313).

`ShowCompareCommand` without `/unified` calls `CGitDiff::DiffCommit(hwnd, path, rev2, rev1, alt)`
([ShowCompareCommand.cpp#L24-L42](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/ShowCompareCommand.cpp#L24-L42)).
The path is empty (the repo root), so `DiffCommit` opens the modal **`CFileDiffDlg`**
("Changed Files"), calling `SetDiff(nullptr, rev1, rev2)`. Note that `bAlternative` is **not**
passed on to the dialog
([GitDiff.cpp#L515-L532](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitDiff.cpp#L515-L532)).
Following the two argument swaps gives `m_rev1 = revision1` (the selected node) and
`m_rev2 = revision2` (HEAD, node 2, or the working tree) **(derived)**.

- **Which side is which:** "Version 1 (**Base**)" is revision1, the node, and "Version 2" is
  revision2 ([RC#L813-L833](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L813-L833)).
  So the node is the old/left side and HEAD, node 2 or the working tree is the new/right side.
- **File list command:** `git diff-tree -r --raw -C<n>% -M<n>% --numstat -z [ignore-ws] <rev1> <rev2> --`.
  For the working tree it is `git diff -r --raw -C -M --numstat -z <rev1> --` (after an index
  refresh)
  ([FileDiffDlg.cpp#L297-L320](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L297-L320),
  [GIT#L1435-L1474](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L1435-L1474)).
- **Per file** (double-click or "Compare revisions"): `CGitDiff::Diff` writes each committed
  version to a read-only temp file with `GetOneFile` and starts the configured external diff
  tool with base = m_rev1 (left) and mine = m_rev2 (right). The working-tree side is the real
  file, not a copy. Added and deleted files are diffed against nothing (`DiffNull`)
  ([FileDiffDlg.cpp#L412-L436](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L412-L436),
  [GitDiff.cpp#L361-L484](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitDiff.cpp#L361-L484)).
- **Which tool:** `PickDiffTool` checks, in order, a per-filename tool, a per-extension tool,
  TortoiseGitIDiff for images, then the generic `Software\TortoiseGit\Diff`. If none is set it
  uses TortoiseGitMerge
  ([AU#L429-L527](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L429-L527)).
- **Other features of the dialog:** editable revision boxes with Browse refs/Log/Reflog buttons,
  a Swap left/right button (disabled for the working tree), Diff Options (whitespace handling,
  diffing against the common ancestor), Show log, a filter box, and "View Patch>>". The per-file
  menu has Compare, "Show changes as unified diff", Revert to rev1/rev2 (not in bare repos),
  Show log, Blame, Export, Save list, and Copy
  ([FileDiffDlg.cpp#L246-L264, L560-L602](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L560-L602)).
- **After:** nothing is refreshed in the graph.

## 8. Unified diff of HEAD revisions / Unified diff

`UnifiedDiffRevs(bHead)` runs in the graph's own process
([RGF#L457-L466](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L457-L466)).
It calls `CAppUtils::StartShowUnifiedDiff(hwnd, "", FriendRef(node1), "", bHead ? "HEAD" : FriendRef(node2), shift)`
([AU#L913-L969](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L913-L969)):

1. Creates a temp file, then runs
   `git.exe diff-tree -r -p [--unified=<diff.context>] --stat --end-of-options <rev1> <rev2> --`
   with its output redirected into that file (or does the libgit2 equivalent)
   ([GIT#L3090-L3119, L3268-L3292](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L3268-L3292)).
   Old is node 1 and new is HEAD or node 2. The patch starts with a `--stat` summary.
2. Marks the file read-only and opens it in the configured `Software\TortoiseGit\DiffViewer`,
   or, if none is set, in **TortoiseGitUDiff.exe** `/patchfile:<tmp> /title:"<rev1>:<rev2>"`
   ([AU#L529-L579](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L529-L579)).
   The viewer is started without waiting. Nothing is refreshed afterwards.

## 9. Shift = alternative diff tool

Both `CompareRevs` and `UnifiedDiffRevs` read `GetAsyncKeyState(VK_SHIFT)` at the moment the
menu command runs, i.e. while the item is clicked
([RGF#L442, L462](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/RevisionGraph/RevisionGraphDlgFunc.cpp#L437-L466)).
"Alternative" swaps external and internal: when an external tool is configured, Shift uses the
built-in TortoiseGitMerge or TortoiseGitUDiff; when the configured value is commented out with a
leading `#`, Shift uses that external tool
([AU#L476-L489, L536-L547](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L476-L489)).

- For the **Unified diff** items, Shift takes effect.
- For the **Compare** items, `/alternative` reaches `ShowCompareCommand` but `DiffCommit`
  drops it on the way to `CFileDiffDlg` (§7), so **Shift has no effect** from the graph.
  Inside the Changed Files dialog, Shift is read again for each file diff
  (FileDiffDlg L418-L433).

---

## Cross-platform facts

### (a) TortoiseGitProc command lines

The command names are registered in [Command.cpp#L186-L236](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/Command.cpp#L186-L236).
User documentation is in [tgit_app_automation.xml](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_app_automation.xml).

| Command | Parameters (exact names) | Source |
|---|---|---|
| `/command:log` | `/path`, `/rev` (row to highlight), `/endrev`, `/startrev` (only together with endrev, gives `start..end`), `/range` (overrides both), `/limit:"N SCALE"`, `/findstring`, `/findtype`, `/findtext`, `/findregex`, `/outfile`. Deprecated aliases: `/revstart`, `/revend` | [LogCommand.cpp#L30-L91](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/LogCommand.cpp#L30-L91) |
| `/command:repobrowser` | `/path`, `/rev` (default `HEAD`) | [RepositoryBrowserCommand.cpp#L25-L29](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/RepositoryBrowserCommand.cpp#L25-L29) |
| `/command:showcompare` | `/path`, `/revision1` (base), `/revision2`, `/unified`, `/alternative` | [ShowCompareCommand.cpp#L29-L41](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/ShowCompareCommand.cpp#L29-L41) |
| `/command:switch` | `/path`, `/rev` (the ref to preselect; the code supports it but the docs don't mention it). Always opens the Switch dialog | [SwitchCommand.cpp#L24-L29](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/SwitchCommand.cpp#L24-L29) |
| `/command:diff` | `/path`, `/path2`, `/startrev` (base), `/endrev`, `/unified`, `/alternative`, `/line`. Given a directory with no path2, it opens the Changed (working-tree status) dialog instead | [DiffCommand.cpp#L30-L120](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/DiffCommand.cpp#L30-L120) |
| `/command:revisiongraph` | `/path`, `/output:<file>` (renders to a file without showing the window) | [RevisionGraphCommand.cpp#L24-L37](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Commands/RevisionGraphCommand.cpp#L24-L37) |

The rough git-CLI equivalents of what the graph does are: log `git log <hash>` or
`git log <h1>..<h2>`; compare `git diff-tree -r <a> <b>` or `git diff <a>`; unified
`git diff-tree -r -p --stat <a> <b>`; switch `git checkout <branch>`. These equivalents are my
own mapping **(derived)**.

### (b) `gitk <rev>` and `git gui browser <rev>` availability

- **Syntax:** `gitk [<options>] [<revision-range>] [--] [<path>...]`, plus
  `--select-commit=<ref>` ([gitk.adoc#L11, L130-L133](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/Documentation/gitk.adoc#L11)).
  `git gui browser <commit>` opens "a tree browser showing all files in the specified commit";
  the documented example is `git gui browser maint`
  ([git-gui.adoc#L36-L39, L96-L100](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/Documentation/git-gui.adoc#L36-L100)).
- **Git for Windows:** the full installer and the portable build ship both, with Tk. The
  installer's main package is `git-for-windows-addons`, which **depends on `mingw-w64-gitk` and
  `mingw-w64-git-gui`**
  ([GFW-PKG mingw-w64-git/PKGBUILD#L557-L563](https://github.com/git-for-windows/MINGW-packages/blob/c8d066fd85562a81dde31f59cae3e25b8b1317c6/mingw-w64-git/PKGBUILD#L557-L563)).
  Those depend on `mingw-w64-tk` (L472-L475, L490-L493), and `cmd/gitk.exe` and
  `cmd/git-gui.exe` wrappers are built (L146). The file list adds `mingw-w64-tk`
  ([GFW-BX make-file-list.sh#L201-L213](https://github.com/git-for-windows/build-extra/blob/c8241c75c7c06d6db94ed6a8dd9b8ca76d1ed842/make-file-list.sh#L201-L213)),
  and the installer creates a "Git GUI" Start-menu entry and an optional "Open Git GUI here"
  ([GFW-BX installer/install.iss#L103, L139](https://github.com/git-for-windows/build-extra/blob/c8241c75c7c06d6db94ed6a8dd9b8ca76d1ed842/installer/install.iss#L103)).
  **MinGit** (`MINIMAL_GIT`) **does not** include them: it drops `cmd/gitk.exe`,
  `cmd/git-gui.exe`, `bin/gitk` and Tk
  ([make-file-list.sh#L207-L213, L310-L322](https://github.com/git-for-windows/build-extra/blob/c8241c75c7c06d6db94ed6a8dd9b8ca76d1ed842/make-file-list.sh#L310-L322)).
- **Debian 13 (trixie) / Ubuntu 24.04 (noble):** these are separate binary packages, `gitk` and
  `git-gui`. Both depend on `git` and `tk`; `git-gui` recommends `gitk`. The `git` package only
  **suggests** them, so a default `apt install git` has neither.
  (https://packages.debian.org/trixie/gitk, https://packages.debian.org/trixie/git-gui,
  https://packages.debian.org/trixie/git, https://packages.ubuntu.com/noble/gitk,
  https://packages.ubuntu.com/noble/git-gui. On Ubuntu both are in `universe`. Versions
  1:2.47.3-0+deb13u1 and 1:2.43.0-1ubuntu7.3, retrieved 2026-09-26.)
- **Fedora:** the subpackages are `gitk` (Requires git, git-gui, tk) and `git-gui` (Requires
  gitk, tk >= 8.4). They require each other, so installing either one pulls in both. The
  `git-all` meta-package requires both; plain `git` does not
  ([FED git.spec L322-L345, L436-L441, L452-L456](https://src.fedoraproject.org/rpms/git/blob/a40d40b977bd5d619ff8f99c78674383cf09a5d6/f/git.spec#_436);
  https://packages.fedoraproject.org/pkgs/git/gitk/).
- **Homebrew (macOS):** the `git` formula builds with `NO_TCLTK=1`, and its caveat says "The
  Tcl/Tk GUIs (e.g. gitk, git-gui) are now in the `git-gui` formula"
  ([BREW Formula/g/git.rb#L106-L116, L217](https://github.com/Homebrew/homebrew-core/blob/461ceeaa5208cb5dbb8c8653e4de59465b836d2f/Formula/g/git.rb#L106-L116)).
  `brew install git-gui` **depends on `tcl-tk`** and installs both `git-gui` and `gitk`, using
  Homebrew's own `wish`
  ([BREW Formula/g/git-gui.rb#L18-L41](https://github.com/Homebrew/homebrew-core/blob/461ceeaa5208cb5dbb8c8653e4de59465b836d2f/Formula/g/git-gui.rb#L18-L41)).
  I have not checked whether Apple's Xcode Command Line Tools git ships gitk or git-gui
  **(unverified)**.

### (c) `git difftool --dir-diff A B` with no `diff.tool` configured

- **Documentation:** "If the configuration variable `diff.tool` is not set, `git difftool` will
  pick a suitable default." `--dir-diff` "never prompts before launching the diff tool"
  ([git-difftool.adoc#L23-L26, L51-L54](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/Documentation/git-difftool.adoc#L23-L54)).
- **Configured-tool lookup:** `diff.tool` first, then **`merge.tool`**. With `--gui`, the order
  is `diff.guitool`, `merge.guitool`, `diff.tool`, `merge.tool`
  ([git-mergetool--lib.sh#L438-L467](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/git-mergetool--lib.sh#L438-L467)).
- **When nothing is configured:** `guess_merge_tool` prints to stderr "This message is displayed
  because 'diff.tool' is not configured. … 'git difftool' will now attempt to use one of the
  following tools: …". It then runs the **first available** candidate. If none is found it
  prints "No known diff tool is available." and exits 1
  ([git-mergetool--lib.sh#L417-L436, L507-L524](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/git-mergetool--lib.sh#L417-L436)).
  The candidate order ([L347-L377](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/git-mergetool--lib.sh#L347-L377)):
  - If `$DISPLAY` is set: `opendiff kdiff3 tkdiff xxdiff meld kompare gvimdiff diffuse diffmerge
    ecmerge p4merge araxis bc codecompare smerge`, with meld moved to the front when
    `GNOME_DESKTOP_SESSION_ID` is set.
  - Always, last: `emerge vimdiff nvimdiff`, or `vimdiff`/`nvimdiff` first when
    `$VISUAL`/`$EDITOR` names vim or nvim.
  - Without `$DISPLAY`, only `kompare` and the terminal tools remain.
- **Dir-diff path:** `builtin/difftool.c` fills temporary left and right directories, then runs
  `git difftool--helper <ldir> <rdir>` once, with `GIT_DIFFTOOL_DIRDIFF=true`
  ([difftool.c#L599-L607](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/builtin/difftool.c#L599-L607)).
  The helper calls `get_merge_tool` and then `run_merge_tool` directly, with no prompt
  ([git-difftool--helper.sh#L69-L89](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/git-difftool--helper.sh#L69-L89)).
- **Consequences for a GUI caller (derived):**
  - On macOS `$DISPLAY` is normally unset. `opendiff` is only a candidate when DISPLAY is set,
    so an unconfigured macOS system usually falls through to `vimdiff`, a terminal tool that
    would run with no terminal if spawned from a GUI app.
  - The same happens on Linux when no GUI diff tool is installed.
  - A GUI caller should check `git config diff.tool`/`merge.tool` (or
    `git difftool --tool-help`) before relying on this.
