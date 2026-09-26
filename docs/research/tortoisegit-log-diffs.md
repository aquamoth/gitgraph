# TortoiseGit: seeing what a commit changed, starting from the Log dialog

Research notes for giving parterre's log window functional parity with TortoiseGit's diffs. The
goal is parity in *what a user can find out*, not a copy of TortoiseGit's UI. Each workflow gets
three answers: how it is reached, what it shows, and which git command it runs. The viewers count
only in read-only use. Blame is a stretch goal and is covered briefly.

Everything comes from reading source and the manual at pinned commits. No GUI was run. Where a
statement is my own reading of the code rather than something the code or docs say outright, it
is marked **(derived)**. Things I could not verify are marked **(unverified)**.

## Sources (pinned)

| Short name | What | Permalink base |
|---|---|---|
| TG | TortoiseGit `master` @ `acc10fc2` | https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/ |
| GLB | `TG/src/TortoiseProc/GitLogListBase.cpp` (commit list and its menu) | (same base) |
| GLA | `TG/src/TortoiseProc/GitLogListAction.cpp` (commit-list menu actions) | (same base) |
| LD | `TG/src/TortoiseProc/LogDlg.cpp` (the Log dialog) | (same base) |
| SLC | `TG/src/Git/GitStatusListCtrl.cpp` (the changed-files list) | (same base) |
| GD | `TG/src/TortoiseProc/GitDiff.cpp` (per-file diff launch) | (same base) |
| FDD | `TG/src/TortoiseProc/FileDiffDlg.cpp` (the "Changed Files" dialog) | (same base) |
| AU | `TG/src/TortoiseProc/AppUtils.cpp` (viewer launch) | (same base) |
| GIT | `TG/src/Git/Git.cpp` | (same base) |
| LLC | `TG/src/Resources/TortoiseLoglistCommon.rc2` (log menu labels) | (same base) |
| RC | `TG/src/Resources/TortoiseProcENG.rc` (dialog layouts, strings) | (same base) |
| UD | `TG/src/TortoiseUDiff/` (TortoiseGitUDiff) | (same base) |
| TM | `TG/src/TortoiseMerge/` (TortoiseGitMerge) and `TG/src/Resources/TortoiseMergeENG.rc` | (same base) |
| BL | `TG/src/TortoiseGitBlame/` (TortoiseGitBlame) | (same base) |
| DOC | `TG/doc/source/en/TortoiseGit/tgit_dug/` (manual sources) | (same base) |
| GITDOC | upstream git `master` @ `0f8e75ab`, `Documentation/config/diff.adoc` | https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/Documentation/config/diff.adoc |

English UI labels come from the `.rc`/`.rc2` files. `TortoiseProc.rc2` pulls in the shell strings
and `TortoiseLoglistCommon.rc2`
([TortoiseProc.rc2#L50-L51](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProc.rc2#L50-L51)). The TortoiseGitMerge
manual is not in the source tree I read, so viewer facts come from code and resource files only.

---

## 0. Shared building blocks

Every workflow below ends in one of two kinds of view.

**A. Per-file side-by-side diff.** `CGitDiff::Diff` resolves both revisions to hashes (adding
`^{}` so that tags peel). It writes each side of the file to a temp file, marks the temp files
read-only, and starts the diff tool. The old side is on the left ("base"), the new side on the
right ("mine"). The titles are `<path>: <short hash>`
([GD#L361-L484](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitDiff.cpp#L361-L484)).
- **Added or deleted file:** `CGitDiff::DiffNull` diffs the one existing version against an
  empty temp file ([GD#L100-L160](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitDiff.cpp#L100-L160)).
- **Renamed or copied file:** the caller passes the old path for the old side
  ([SLC#L3035-L3040](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L3035-L3040)).
- **Submodule (a directory entry):** no file diff. A modal "Submodule Diff" dialog shows the old
  and new submodule commits with their subjects, and a change type (new, deleted, fast-forward,
  rewind, newer or older by time). It works this out with `git log -n1` and a fast-forward check
  inside the submodule ([GD#L162-L359](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitDiff.cpp#L162-L359)).
- **Which tool:** `PickDiffTool` checks a per-filename tool, then a per-extension tool. Next it
  sends image extensions (`.png`, `.jpg`, `.svg`, …) to TortoiseGitIDiff. Last comes the generic
  `Software\TortoiseGit\Diff` setting. If that is empty, or commented out with `#`, it starts
  **TortoiseGitMerge** with `/base /mine /basename /minename /basereflectedname /minereflectedname`
  ([AU#L429-L525](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L429-L525)).
- `/readonly` is added only when the caller asks for it (`DiffFlags::bReadOnly`, default false),
  and no diff caller asks. The temp files' read-only attribute is all that marks them
  ([AppUtils.h#L40-L50](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.h#L40-L50)). What that does in the viewer
  is in §6.

**B. Unified patch.** `CAppUtils::StartShowUnifiedDiff` reads git's `diff.context`, writes a patch
to a temp file and marks it read-only. It opens the file in the `Software\TortoiseGit\DiffViewer`
program, or in **TortoiseGitUDiff** `/patchfile:<tmp> /title:"<rev1>:<rev2>"` when that setting
is empty ([AU#L913-L947](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L913-L947),
[AU#L528-L578](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L528-L578)). The patch comes from
([GIT#L3090-L3119](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L3090-L3119)):

```
git diff-tree -r -p [-m] [-c] [--unified=<diff.context>] --stat --end-of-options <rev1> <rev2> -- [<path>]
```

- The patch starts with a `--stat` summary.
- **No `-M`.** `diff-tree` is plumbing, and git's `diff.renames` default applies only to porcelain
  such as `git diff` and `git log`
  ([GITDOC#L157-L164](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/Documentation/config/diff.adoc#L157-L164)).
  So these patches show a renamed file as a deletion plus an addition **(derived)**. The same
  reason explains why TortoiseGit passes `diff.context` itself **(derived)**.
- With default settings this runs `git.exe`, not libgit2: `GIT_CMD_DIFF` is not in
  `DEFAULT_USE_LIBGIT2_MASK` ([Git.h#L34](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.h#L34),
  [GIT#L3268-L3292](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L3268-L3292)).

**Shift = the other tool.** Holding Shift when the command runs swaps the configured external
tool and the built-in one, for both kinds of view
([AU#L476-L489](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L476-L489),
[AU#L536-L547](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L536-L547)).

**How the changed-files list is computed.** `GitRevLoglist::SafeFetchFullInfo` diffs the commit
against **each parent in turn** (a root commit against the empty tree). It tags every file with
the parent's index (`m_ParentNo`). By default this runs in-process through TortoiseGit's git fork
with the arguments `-C50% -M50% -r`, so renames and copies are detected. Binary files get `-` for
their line counts ([GitRevLoglist.cpp#L322-L404](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitRevLoglist.cpp#L322-L404),
[Git.h#L268-L285](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.h#L268-L285)). The git-CLI equivalent is
`git diff-tree -r -M50% -C50% --numstat <parent> <commit>`, once per parent **(derived)**. The 50
is `Software\TortoiseGit\DiffSimilarityIndexThreshold`, default 50
([GIT#L38-L47](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L38-L47)).

## 1. Where diffs live in the Log dialog

The Log dialog ("Log Messages") has three panes: the commit list, the message, and the changed
files. Under them sit a file filter box and a **View** button
([RC#L210-L243](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L210-L243)). A user reaches a diff from:

1. the commit list's context menu, and optionally double-click or Enter on a commit (§2, §3);
2. the **View Patch** pane, a patch window docked to the dialog (§4);
3. the changed-files list: double-click, Enter, and its context menu (§5).

Selecting two or more commits empties the changed-files list. Only a single selection fills it
([LD#L822-L1040](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L822-L1040)) **(derived)**.

## 2. Commit list, one commit selected

The menu's diff items, in menu order
([GLB#L1741-L1840](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListBase.cpp#L1741-L1840); labels
[LLC#L30-L87](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseLoglistCommon.rc2#L30-L87)):

| Item | Shown when | What it does |
|---|---|---|
| Compare with &working tree | a working tree exists | Changed Files dialog, this commit vs the working tree. Not "what the commit changed"; listed for completeness |
| Show changes as &unified diff | one parent, and a working tree exists | Unified patch, parent vs commit (§2b) |
| &Unified diff with ▸ | a merge commit, and a working tree exists | Submenu: All Parents, Only Merged Files, Show extra changes after merge, then `Parent 1: "<subject, 20 chars>..." (<short hash>)`, `Parent 2: …` (§2b) |
| &Compare with previous revision | always; a submenu of parents for a merge | Changed Files dialog or a file diff, parent vs commit (§2a) |

The menu code also has `Blame` and `Blame previous revision`, but the Log dialog switches them off
([GLB#L98-L100](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListBase.cpp#L98-L100)). Only TortoiseGitBlame's own
log list turns them back on
([LogListBlameAction.cpp#L33-L50](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseGitBlame/LogListBlameAction.cpp#L33-L50)). The
"working tree exists" condition on the unified-diff items looks accidental, since the patch never
touches the working tree **(derived)**.

**Double-click and Enter on a commit do nothing by default.** With the setting "Can double-click
in log list to compare with previous revision" (`DiffByDoubleClickInLog`, default off), both run
*Compare with previous revision* against parent 1. That item then becomes the menu's bold
default ([GLB#L2719-L2732](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListBase.cpp#L2719-L2732),
[GLB#L2760-L2766](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListBase.cpp#L2760-L2766),
[GLB#L2494-L2507](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListBase.cpp#L2494-L2507),
[LD#L1784-L1790](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L1784-L1790),
[SetDialogs.cpp#L55](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Settings/SetDialogs.cpp#L55),
[RC#L736](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L736)). The manual says it is off because
"fetching the diff is often a long process"
([dug_settings_general.xml#L288-L301](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_settings_general.xml#L288-L301)).

### 2a. Compare with previous revision

Handler: [GLA#L371-L428](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListAction.cpp#L371-L428). The parent is
parent 1, or the one picked in the submenu. A root commit gets "No previous version."
([RC#L4520](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L4520)).

- **Log of the whole repository or a folder:** `CGitDiff::DiffCommit` opens the modal **Changed
  Files** dialog. The parent is "Version 1 (Base)" and the commit is "Version 2"; a folder log
  filters the list to that folder
  ([GD#L515-L532](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitDiff.cpp#L515-L532),
  [FDD#L116-L122](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L116-L122)).
- **Log of one file:** goes straight to a side-by-side diff of that file. With "follow renames" on,
  it walks the list to find the file's name at that commit and at the parent.

The **Changed Files** dialog is the same one the revision graph's Compare items open (see
`tortoisegit-context-menu-actions.md` §7):
- **File list:** `git diff-tree -r --raw -C50% -M50% --numstat -z [ignore-ws] <parent> <commit> --`,
  run in a background thread ([FDD#L292-L320](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L292-L320),
  [GIT#L1435-L1474](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L1435-L1474)). For a merge this covers the one chosen
  parent only.
- **Double-click or Enter on a file:** side-by-side diff. Added and deleted files are diffed
  against nothing; a renamed file has its old name on the old side
  ([FDD#L411-L436](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L411-L436)).
- **File menu:** Compare revisions (default), Show changes as unified diff, Revert to either side,
  Show log, Blame revisions, Export, Show log of submodule, Save list, Copy
  ([FDD#L560-L602](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L560-L602)).
- **Diff Options button:** four whitespace toggles (ignore space at EOL, space change, all space,
  blank lines) and "common ancestor". The whitespace toggles pass git's `--ignore-*` flags to the
  file-list command, so files with only whitespace changes drop out of the list
  ([FDD#L1359-L1370](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L1359-L1370)) **(derived)**.
- Editable revision boxes, a swap button, a path filter, and "View Patch>>". That opens a patch
  pane like §4, remembered in the git config value `tgit.diffshowpatch`
  ([FDD#L246-L264](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L246-L264)).

### 2b. Show changes as unified diff

Handler: [GLA#L187-L274](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListAction.cpp#L187-L274). Every choice
ends in the §0B patch command and TortoiseGitUDiff.

| Choice | git command | What you see |
|---|---|---|
| Plain item (one parent) | `git diff-tree -r -p --stat <parent> <commit> --` | The commit's patch |
| Parent N | `git diff-tree -r -p --stat <parentN> <commit> --` | Everything the merge brought in, relative to that parent |
| All Parents | `git diff-tree -r -p -m --stat <commit> --` | One patch per parent, one after the other |
| Only Merged Files | `git diff-tree -r -p -c --stat <commit> --` | Combined diff: only files that differ from every parent |
| Show extra changes after merge | `git diff-tree --cc <commit>` | Dense combined diff: conflict resolutions and edits made in the merge itself. If empty, a message box says "No extra changes after merge" ([RC#L4459-L4460](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L4459-L4460)) |

`--unified=<n>` is added when `diff.context` is set. A log limited to a path adds that path (the
old name, when following renames). The viewer title is `<rev1>:<rev2>`, or just the commit for
All Parents and Only Merged Files ([AU#L935](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L935)).

## 3. Commit list, two commits selected

Menu: [GLB#L2093-L2133](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListBase.cpp#L2093-L2133). "First" is the
upper selected row and "last" the lower one. In the default newest-first order, "last" is the
older commit **(derived)**.

| Item | Shown when | What it does |
|---|---|---|
| &Compare revisions | two selected, or a contiguous run | Changed Files dialog; base = lower row, other = upper row ([GLA#L300-L333](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListAction.cpp#L300-L333)) |
| Show changes as &unified diff | exactly two, and a working tree exists | `git diff-tree -r -p --stat <lower> <upper> --` in TortoiseGitUDiff ([GLA#L276-L298](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListAction.cpp#L276-L298)) |
| Comp&are change sets | exactly two | Writes each commit's patch (`git format-patch --stdout <h>~1..<h>`) to a temp file and shows the two patches side by side in TortoiseGitMerge: a "diff of diffs" ([GLA#L429-L452](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitLogListAction.cpp#L429-L452)) |

The "Show log of a..b" items in the same block list commits, not changes. The manual describes
the first two items ([dug_log.xml#L421-L443](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_log.xml#L421-L443)).

## 4. The View Patch pane

- **Reached by** the **View** button's menu → "View Patch", a checked toggle
  ([LD#L3427-L3450](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L3427-L3450),
  [LD#L3511-L3512](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L3511-L3512)). The state is saved as
  `tgit.logshowpatch` in the repository's **local** git config, and restored when the dialog
  opens. So it is off by default, and remembered per repository
  ([LD#L456-L457](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L456-L457),
  [LD#L1137-L1153](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L1137-L1153),
  [Git.h#L419](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.h#L419)). The manual does not mention it.
- **Looks like** a separate "View Patch" window glued to the right edge of the Log dialog. It moves
  and resizes with the dialog, and its width is remembered
  ([PatchViewDlg.cpp#L242-L272](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/PatchViewDlg.cpp#L242-L272),
  [RC#L1737-L1744](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L1737-L1744)). Inside is a Scintilla view
  with unified-diff colouring (the TortoiseGitUDiff colour and font settings) and a find bar:
  Ctrl+F; F3 and Shift+F3 (also Alt+N and Alt+P); Esc closes the bar; one "match case" option
  ([PatchViewDlg.cpp#L79-L101](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/PatchViewDlg.cpp#L79-L101),
  [PatchViewDlg.cpp#L300-L361](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/PatchViewDlg.cpp#L300-L361),
  [RC#L3492-L3500](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3492-L3500)).
- **Shows** this, refreshed 100 ms after the selection settles
  ([LD#L1061-L1135](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L1061-L1135)):
  - one commit, no file selected: `git diff-tree -r -p [--unified=n] --stat <commit>~1 <commit>`,
    the whole commit against its **first** parent;
  - one commit with files selected: `git diff <commit>^<N>..<commit> -- [<old path>] <path>` for
    each selected file, concatenated, where N is the file's parent group (§5a). This is porcelain
    `git diff`, so renames are paired and the user's diff config applies **(derived)**;
  - more than one commit selected: empty.

This is the nearest TortoiseGit has to an inline "click a file, see its diff" flow.

## 5. The changed-files list

Set up in [LD#L308](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/LogDlg.cpp#L308): columns Path, Extension, Status,
Lines added, Lines deleted. It gets every file-menu item except Restore and Changelists.

### 5a. Merge commits: which parent?

All of them. The list holds one diff per parent (§0), so a merge commit's list is grouped:
"Diff with parent 1: <short hash>", "Diff with parent 2: <short hash>", and so on. A file that
differs from both parents appears in both groups. There is also an always-empty "Merged Files"
group header ([SLC#L3968-L4030](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L3968-L4030),
[RC#L3580](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3580),
[RC#L3811](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseProcENG.rc#L3811)). Every file action uses the parent of
the file's group. A three-way "Three way diff" path for merged files needs a `MERGE_MASK` flag
that nothing sets any more, so it is dead code **(derived)**.

### 5b. Double-click and Enter

- **Double-click** diffs the clicked file. **Enter** diffs every selected file, each in its own
  viewer. With more than `max(3, NumDiffWarning)` files selected (default 10), Enter first asks
  "For every of these items a new instance of the diff viewer will be started. Do you really want
  to show the diff for so many items at once?"
  ([SLC#L2926-L2976](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L2926-L2976),
  [SLC#L3618-L3657](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L3618-L3657),
  [SLC#L4360-L4370](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L4360-L4370)).
- Both call `StartDiff` ([SLC#L3026-L3168](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L3026-L3168)). The
  parent is `<commit>~1` for group 1 and `<commit>^N` for group N:

| File status | Left (old) | Right (new) |
|---|---|---|
| Modified | file at parent | file at commit |
| Renamed or copied | **old path** at parent | new path at commit |
| Added, or any file of a root commit | empty | file at commit |
| Deleted | file at parent | empty |
| Submodule | Submodule Diff dialog (§0A) | |
| Binary | as its status; the viewer refuses it (§6b), except images, which go to TortoiseGitIDiff | |

### 5c. File context menu, diff items

Menu: [SLC#L1739-L1915](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L1739-L1915); handlers
[SLC#L2155-L2407](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L2155-L2407).

| Item | Shown when | What it does |
|---|---|---|
| Compare with b&ase | always; the bold default | Same as double-click, for each selected file |
| Show changes as &unified diff | not a freshly initialised repo | For each selected file, `git diff-tree -r -p --stat <parent> <commit> -- <path>`. All the patches go into one TortoiseGitUDiff window titled with the commit hash ([SLC#L2285-L2355](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L2285-L2355)). Only the new path is passed, so a renamed file shows as wholly added **(derived)** |
| Compare with &working tree; Compare parent with working tree: `<parent>` | a working tree exists | This file at the commit (or its parent) vs the working-tree file. Adjacent, not "what changed" |
| Compare two files | exactly two files selected | `/command:diff` between the two paths at this commit; a deleted one is taken at `~1` ([SLC#L2260-L2283](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L2260-L2283)) |
| Mark for comparison, then Diff with "`<marked>`" | not deleted | Remembers one path@commit, then diffs any other path@commit against it ([SLC#L2155-L2167](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L2155-L2167)) |
| &Blame | one file, not deleted, a working tree exists | TortoiseGitBlame at this commit (§8) |
| View revision in alternative editor; &Open; Open with... | one file, not deleted | Opens the file as it was at the commit |
| Save revision &to...; Export selection to... | not deleted | Writes the file or files as at the commit |
| Show &log; Show log of submodule; Show log &before rename/copy | one file | Opens another log |
| Copy to clipboard ▸ | always | full paths, relative paths, file/foldernames, column '…'; and "Copy all information to clipboard" |

"Revert to this revision" and "Revert to parent revision" also appear. They write to the working
tree and are out of scope here. The manual lists the same menu under slightly different names
("Show as unified diff", "Blame...")
([dug_log.xml#L521-L629](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_log.xml#L521-L629)).

---

## 6. The viewers, read-only use

### 6a. TortoiseGitUDiff (unified patch viewer)

- **Input:** `/patchfile:<file> /title:<text>`, or a patch piped on stdin with `/p`. The window
  title is `<title> - TortoiseGitUDiff`
  ([TortoiseUDiff.cpp#L66-L129](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/TortoiseUDiff.cpp#L66-L129),
  [TortoiseUDiff.rc#L143](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/TortoiseUDiff.rc#L143)).
- **Layout:** one Scintilla editor with the `diff` lexer, a line-number margin (the patch's own
  line numbers **(derived)**) and a find bar at the bottom. There is no toolbar
  ([MainWindow.cpp#L604-L641](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L604-L641),
  [MainWindow.cpp#L783-L791](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L783-L791)).
- **Colours** (light defaults): added lines on `#CCFFCC`, removed lines on `#FFDDDD`, headers on
  `#FFFF80`, `@@` positions in red. Dark-mode variants exist, and high-contrast mode turns
  colouring off ([UDiffColors.h#L21-L50](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/UDiffColors.h#L21-L50),
  [MainWindow.cpp#L777-L782](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L777-L782)).
- **Whitespace is always shown** (`SCI_SETVIEWWS 1`)
  ([MainWindow.cpp#L639-L641](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L639-L641)).
- **Navigation:** none beyond scrolling and find. There is no next/previous file or hunk, no go
  to line and no word wrap. Shift+wheel scrolls sideways
  ([MainWindow.cpp#L119-L129](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L119-L129)) **(derived:
  no such commands exist in the menu or accelerators)**.
- **Find:** Ctrl+F opens a bar with Previous, Next and "Match case". It searches as you type,
  starts from the selected text, and does not wrap round; a miss flashes the window. F3 and
  Shift+F3 repeat ([TortoiseUDiff.rc#L60-L116](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/TortoiseUDiff.rc#L60-L116),
  [MainWindow.cpp#L1123-L1152](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L1123-L1152)).
- **Keys and menu:** Esc closes the find bar, or else quits; Ctrl+W quits; Ctrl+P prints; the
  File menu also has Open, Save, Save as, Page setup, Dark Mode, Settings and "Apply Patch..."
  ([TortoiseUDiff.rc#L60-L98](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/TortoiseUDiff.rc#L60-L98),
  [MainWindow.cpp#L285-L299](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L285-L299)). Copy, select all
  and zoom come from Scintilla's default keys **(derived)**.
- **Not really read-only:** the editor is set read-only at start-up, then made writable before
  loading, and nothing sets it back. Edits can be saved, and closing a changed buffer asks to
  save ([MainWindow.cpp#L629](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L629),
  [MainWindow.cpp#L916](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L916),
  [MainWindow.cpp#L1055-L1071](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L1055-L1071)) **(derived)**.
- **Encoding:** UTF-8 if the first 4 KiB are valid UTF-8 or have a UTF-8 BOM, otherwise the
  system ANSI code page. There is no UTF-16 decoding and no encoding picker
  ([MainWindow.cpp#L811-L926](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L811-L926),
  [MainWindow.cpp#L980-L1053](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L980-L1053)).
- **Huge input:** refused at 250 MiB with "The file is too big"
  ([MainWindow.cpp#L861-L865](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseUDiff/MainWindow.cpp#L861-L865)).
- **Settings:** font Consolas 10, tab size 4, and the six colour pairs, set from TortoiseGit's
  Settings rather than in the viewer
  ([SettingsTUDiff.cpp#L33-L49](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Settings/SettingsTUDiff.cpp#L33-L49),
  [dug_settings_udiff.xml#L3-L45](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_settings_udiff.xml#L3-L45)).

### 6b. TortoiseGitMerge (side-by-side viewer)

Only the viewing side is covered. Editing, merging, patch applying and saving are left out.

- **Input from TortoiseGit:** two temp files plus display names (§0A). No read-only flag is
  passed.
- **Not really read-only:** a read-only `/mine` file only disables "Mark as resolved". In
  two-pane view the right pane is made writable anyway, and "Enable edit" (Ctrl+E) unlocks the
  other ([TortoiseMerge.cpp#L352-L358](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/TortoiseMerge.cpp#L352-L358),
  [MainFrm.cpp#L936-L939](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L936-L939),
  [MainFrm.cpp#L2482-L2503](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L2482-L2503)) **(derived)**.
- **Layout:** two panes by default. Ctrl+D switches to a one-pane view that interleaves removed
  and added lines, and the choice is remembered (`OnePane`, default 0). Ctrl+U swaps the sides.
  Ribbons by default. A **locator bar** on the left gives an overview of the whole file and
  scrolls on click. A **line diff bar** at the bottom shows the hovered line's two versions
  stacked, in two-pane view only
  ([MainFrm.cpp#L212-L221](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L212-L221),
  [MainFrm.cpp#L1276-L1330](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L1276-L1330),
  [LocatorBar.cpp#L219-L281](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/LocatorBar.cpp#L219-L281),
  [LineDiffBar.cpp#L73-L114](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/LineDiffBar.cpp#L73-L114)).
- **Display defaults** ([BaseView.cpp#L74-L77](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/BaseView.cpp#L74-L77),
  [MainFrm.cpp#L212-L221](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L212-L221)):

| Option | Label, key | Default |
|---|---|---|
| Line numbers | "Show line numbers" | on |
| Whitespace markers | "Show Whitespaces", Ctrl+T | on |
| Inline (within-line) diff | "Inline diff", "Inline diff word-wise" | on, word-wise; skipped for lines over 3000 characters |
| Moved blocks | "Moved blocks" ("Detect and highlight moved blocks") | on |
| Collapse unchanged sections | "Collapse", Ctrl+L | off |
| Wrap long lines | "Wrap long lines", Ctrl+P | off |
| Locator bar, line diff bar, status bar | View menu | on |

- **Navigation** ([TortoiseMergeENG.rc#L600-L668](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseMergeENG.rc#L600-L668)):
  next difference Ctrl+Down, Alt+Down, F7 or F11; previous difference Ctrl+Up, Alt+Up, Shift+F7,
  Shift+F11 or Ctrl+F11; next and previous inline difference Ctrl+Alt+Right and Ctrl+Alt+Left;
  switch pane F6. On load it jumps to the first difference ("Jump to first difference when
  loading", on), unless `/line` is given
  ([MainFrm.cpp#L1048-L1113](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L1048-L1113)). There is no
  first/last-difference command and no next/previous *file*: one window shows one file.
- **Find and go to line:** Ctrl+F opens a dialog with "Match case", "Limit search to modified
  lines", "Search up" and "Whole word", plus Find and Count; no regex. F3 and Shift+F3 repeat;
  Ctrl+F3 searches for the selection. Ctrl+G goes to a line
  ([TortoiseMergeENG.rc#L273-L318](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Resources/TortoiseMergeENG.rc#L273-L318),
  [BaseView.cpp#L5888-L5926](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/BaseView.cpp#L5888-L5926)).
- **Diff options:** whitespace mode "Compare whitespaces" (default), "Ignore whitespace changes",
  "Ignore all whitespace changes"; "Ignore line endings" (on by default); ignore case (off, set in
  Settings only); ignore comments (off, only for listed extensions); regex filters. Each change
  re-runs the diff ([DiffData.cpp#L199-L201](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/DiffData.cpp#L199-L201),
  [MainFrm.cpp#L3012-L3058](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L3012-L3058),
  [MainFrm.cpp#L3170-L3294](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L3170-L3294)).
- **Copy:** Ctrl+C or Ctrl+Insert, or "Copy" in the context menu; Ctrl+A selects all
  ([BaseView.cpp#L4481-L4495](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/BaseView.cpp#L4481-L4495)).
- **Encoding and line endings:** detected per file: UTF-32 and UTF-16 by BOM, UTF-16 by a
  null-byte heuristic, UTF-8 with or without BOM, else ANSI. "Default to UTF-8 encoding" is off
  ([FileTextLines.cpp#L63-L200](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/FileTextLines.cpp#L63-L200)). The status
  bar shows each side's encoding, line-ending style and `-n`/`+n` line counts
  ([BaseView.cpp#L269-L337](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/BaseView.cpp#L269-L337)). On a read-only view,
  Ctrl+click on the encoding reloads the file with another encoding
  ([MainFrm.cpp#L3693-L3724](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L3693-L3724)). When only
  whitespace, encoding or line endings differ, it says "The text is identical, but the files do
  not match!" and lists which of them differ
  ([MainFrm.cpp#L950-L987](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L950-L987)).
- **Binary and huge files:** any aligned run of four zero bytes means binary, and the file is
  refused with "The file … is not a valid text file!". Files of 2 GiB or more are refused as "too
  big"; below that the whole file is loaded into memory. Nothing warns earlier, in TortoiseGit or
  the viewer ([FileTextLines.cpp#L70-L77](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/FileTextLines.cpp#L70-L77),
  [FileTextLines.cpp#L236-L284](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/FileTextLines.cpp#L236-L284),
  [MainFrm.cpp#L789-L801](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/MainFrm.cpp#L789-L801)) **(derived: no other
  check found)**.
- **Other:** font Consolas 10, tab size 4. Esc, Ctrl+Q and Ctrl+W quit; F5 reloads. Ctrl+wheel or
  Shift+wheel scrolls sideways; there is no zoom and no print
  ([BaseView.cpp#L2300-L2335](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/BaseView.cpp#L2300-L2335)).

## 7. Defaults, and what users change

Out of the box:

| Setting | Default | Source |
|---|---|---|
| File diff viewer (`Diff`) | empty = TortoiseGitMerge | [SettingsProgsDiff.cpp#L33-L34](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/Settings/SettingsProgsDiff.cpp#L33-L34) |
| Patch viewer (`DiffViewer`) | empty = TortoiseGitUDiff | (same) |
| Per-extension tools (`DiffTools\<ext>`) | none, except image types → TortoiseGitIDiff | [AU#L429-L463](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L429-L463) |
| Double-click / Enter on a commit | does nothing (`DiffByDoubleClickInLog` off) | §2 |
| View Patch pane | closed (`tgit.logshowpatch` unset) | §4 |
| Rename/copy detection threshold | 50 % | §0 |
| Context lines in patches | git's `diff.context`, else git's 3 | [GITDOC#L70-L72](https://github.com/git/git/blob/0f8e75abebff0877cae681a3d5ff31ac47f54220/Documentation/config/diff.adoc#L70-L72) |
| Warn before opening more than N viewers | 10 | [dug_settings_advanced.xml#L376-L384](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_settings_advanced.xml#L376-L384) |
| TortoiseGitMerge: two panes, line numbers, whitespace shown, inline diff, moved blocks, ignore line endings | on | §6b |
| TortoiseGitMerge: ignore whitespace, ignore case, collapse, wrap | off | §6b |

The manual never says which settings users commonly change. Its hints are:
- worked examples for external diff tools (ExamDiff Pro, KDiff3, WinMerge, Araxis) and for an
  external patch viewer ([dug_settings_progs.xml#L115-L185](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_settings_progs.xml#L115-L185));
- the double-click option, "not enabled by default"
  ([dug_settings_general.xml#L288-L301](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_settings_general.xml#L288-L301));
- the Advanced page as "infrequently used settings"
  ([dug_settings_advanced.xml#L9-L15](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_settings_advanced.xml#L9-L15)).

So the likely changes are: a different diff tool, double-click-to-diff, and the View Patch pane
**(derived)**. All three are remembered choices, not per-use toggles.

## 8. Blame from the log (stretch goal)

- **Reached by** the changed-files list's "&Blame" item only: one file, not deleted, and a working
  tree must exist. The commit list has no blame item in the Log dialog (§2)
  ([SLC#L1869-L1870](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L1869-L1870)). The Changed Files dialog
  has "Blame revisions", which blames the newer side
  ([FDD#L587-L593](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/FileDiffDlg.cpp#L587-L593)).
- **Launch:** `TortoiseGitBlame.exe /path:<file> /rev:<commit>`, i.e. the file **as of the
  selected commit**, not its parent. There is no options dialog, although the manual still speaks
  of one ([AU#L712-L729](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L712-L729),
  [SLC#L2403-L2407](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/GitStatusListCtrl.cpp#L2403-L2407),
  [dug_log.xml#L582-L588](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/doc/source/en/TortoiseGit/tgit_dug/dug_log.xml#L582-L588)).
- **git command:** `git blame -p [-M<n> | -C<n> | -C -C<n> | -C -C -C<n>] [-w] [-S <grafts>] <rev> -- <path>`.
  Move/copy detection is off by default, as is ignore-whitespace. "Only consider first parents"
  is done with a grafts file from `git rev-list --first-parent`
  ([TortoiseGitBlameDoc.cpp#L149-L224](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseGitBlame/TortoiseGitBlameDoc.cpp#L149-L224)).
- **Shows:** the file text with a gutter of short hash and author (date, file name and original
  line number can be switched on), coloured by age from white to yellow. There is a log pane of
  the file's history and a commit-info pane
  ([TortoiseGitBlameView.cpp#L127-L141](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseGitBlame/TortoiseGitBlameView.cpp#L127-L141),
  [TortoiseGitBlameView.cpp#L1672-L1685](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseGitBlame/TortoiseGitBlameView.cpp#L1672-L1685)).
- **How it hangs off a diff:** a line's context menu offers "Blame previous revision" (re-blame at
  the line's commit's parent, keeping the line), "Compare with previous revision" (the §0A file
  diff for that line's commit) and "Show log"
  ([TortoiseGitBlameView.cpp#L324-L511](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseGitBlame/TortoiseGitBlameView.cpp#L324-L511)).
  So blame and file diff point at each other: diff → blame at a commit, and blame line → diff of
  its commit.

## 9. Inventory

For a later grilling to give verdicts on. "Default" is what happens out of the box.

| # | Workflow | How it's reached | Default behaviour |
|---|---|---|---|
| 1 | Whole-commit file list, parent vs commit | Commit menu → Compare with previous revision | Modal Changed Files dialog, parent 1; one file per row with status and line counts |
| 2 | Same, against a chosen parent of a merge | Commit menu → Compare with previous revision ▸ Parent N | As 1, for parent N |
| 3 | Compare with previous on double-click / Enter | Setting `DiffByDoubleClickInLog` | Off: double-click and Enter do nothing |
| 4 | Whole-commit unified patch | Commit menu → Show changes as unified diff | TortoiseGitUDiff, `diff-tree -p --stat`, no rename pairing |
| 5 | Merge patch against one parent | Commit menu → Unified diff with ▸ Parent N | As 4, vs parent N |
| 6 | Merge patch against every parent | … ▸ All Parents | `-m`: one patch per parent |
| 7 | Combined diff of a merge | … ▸ Only Merged Files | `-c` |
| 8 | Changes made in the merge itself | … ▸ Show extra changes after merge | `--cc`; a message box if empty |
| 9 | Patch pane next to the log | View button → View Patch | Off; when on, remembered per repo; whole commit vs first parent, or the selected files |
| 10 | Two commits: file list | Select two → Compare revisions | Changed Files dialog, lower row as base |
| 11 | Two commits: unified patch | Select two → Show changes as unified diff | TortoiseGitUDiff |
| 12 | Two commits: diff of their patches | Select two → Compare change sets | Both `format-patch` outputs side by side in TortoiseGitMerge |
| 13 | One file's diff, parent vs commit | File list: double-click, or menu → Compare with base (bold) | TortoiseGitMerge, two panes; old name for renames; empty side for added and deleted |
| 14 | Several files' diffs at once | File list: select, Enter | One viewer per file; asks above 10 |
| 15 | One file's unified patch | File list menu → Show changes as unified diff | Selected files concatenated in one TortoiseGitUDiff; renames look added |
| 16 | Merge commit's files, per parent | Automatic | File list grouped "Diff with parent N"; a file can appear once per parent |
| 17 | Submodule change | Double-click a submodule entry | Submodule Diff dialog: old and new commit, subjects, fast-forward or rewind |
| 18 | Image change | Double-click an image file | TortoiseGitIDiff |
| 19 | Binary (non-image) change | Double-click | TortoiseGitMerge refuses: "not a valid text file" |
| 20 | Compare two files in one commit | File list: select two → Compare two files | Side-by-side diff of the two paths |
| 21 | Compare any two path@commit pairs | File list → Mark for comparison, then Diff with "…" | Side-by-side diff |
| 22 | File at commit vs working tree | File list → Compare with working tree (or parent with working tree) | Side-by-side diff; needs a working tree |
| 23 | Commit vs working tree | Commit menu → Compare with working tree | Changed Files dialog |
| 24 | Open, save or export the file as at the commit | File list menu | Temp file in an editor, or written to a chosen place |
| 25 | Alternative diff tool | Hold Shift while choosing | Swaps built-in and external tool |
| 26 | Viewer: two panes / one pane | TortoiseGitMerge Ctrl+D | Two panes, remembered |
| 27 | Viewer: next / previous change | Ctrl+Down / Ctrl+Up (also F7, F11, Alt) | Starts at the first change |
| 28 | Viewer: inline word diff | View menu | On, word-wise |
| 29 | Viewer: whitespace | Compare / ignore changes / ignore all; show whitespace | Compare all; whitespace shown |
| 30 | Viewer: collapse unchanged, wrap | Ctrl+L, Ctrl+P | Both off |
| 31 | Viewer: find, go to line, copy | Ctrl+F, Ctrl+G, Ctrl+C | Available |
| 32 | Patch viewer: find | Ctrl+F in TortoiseGitUDiff or the View Patch pane | Substring, match case only |
| 33 | Blame the file at this commit | File list menu → Blame | TortoiseGitBlame at the commit; no options dialog |
