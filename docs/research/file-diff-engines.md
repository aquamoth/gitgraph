# File diffs: where they come from and how they are drawn

Research note for issue #42: *what are the realistic options for producing the diff of one changed
file (a commit against its first parent) and drawing it in parterre's own read-only egui window,
and what does each cost?* parterre will not write its own diff algorithm. Every engine, renderer
or crate named here needs a discussion before it is used; this note is the input to that
discussion.

Sources are official docs and source code at pinned versions, crates.io data, and experiments I
ran. A statement that is my own conclusion is marked **(derived)**. A statement I could not check
is marked **(unverified)**. A statement I checked by running something is marked **(tested)**;
the probes are described in §10. The probes ran on Linux x86_64 with git 2.43.0 and rustc
1.98.1 on 2026-09-26.

## Sources (pinned)

| Short name | What | Link base |
|---|---|---|
| GITDOC | git documentation, tag `v2.55.0` | https://github.com/git/git/blob/v2.55.0/Documentation/ |
| GITSRC | git source, tag `v2.55.0` | https://github.com/git/git/blob/v2.55.0/ |
| TG | TortoiseGit `master` @ `acc10fc2` (as in `tortoisegit-revision-graph.md`) | https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/ |
| GITUI | gitui `master` @ `2fa693cb` (2026-07-31) | https://github.com/gitui-org/gitui/blob/2fa693cb6ed431b21ebc300dd02e83c2476699ce/ |
| GB | GitButler `master` @ `bfc4c3d4` (2026-09-26) | https://github.com/gitbutlerapp/gitbutler/blob/bfc4c3d4ece16c68edfe42a9049813bc4b921dc5/ |
| LG | lazygit `master` @ `1b39e38a` | https://github.com/jesseduffield/lazygit/blob/1b39e38ae66996a9d15c14f41dc1cd1ae5b74327/ |
| DFT | difftastic 0.71.0 manual and `difft --help` | https://difftastic.wilfred.me.uk/ |
| DELTA | delta 0.19.2 manual and `delta --help` | https://dandavison.github.io/delta/ |
| CRATES | crates.io API (downloads, owners, dependencies), read 2026-09-26 | https://crates.io/api/v1/crates/ |
| Crate source | the published crates as downloaded by cargo: similar 3.2.0, imara-diff 0.2.0, syntect 5.3.0, egui_extras 0.36.2, tree-sitter 0.27.0, chardetng 1.0.0 | https://docs.rs/ |

## TL;DR

- **git can produce everything a first version needs.** `git diff <parent> <commit> -- <old>
  <new>` gives a unified patch that honours `diff.algorithm`, the `diff` attribute (`-diff`,
  `binary`, drivers, per-path algorithm), textconv, git's binary detection and rename
  detection. Plumbing (`diff-tree -p`) ignores `diff.algorithm` and textconv, so the porcelain
  `git diff` is the one to call, with a few flags pinned (§1.3) **(tested)**.
- **A unified patch is enough for both views.** Unified is direct. Side by side pairs each run of
  `-` lines with the following run of `+` lines, row by row, and pads the shorter side. What is
  lost: git doesn't say which old line became which new line inside a changed block, so pairing
  is by position. TortoiseGitMerge, difftastic and delta do better alignment with their own diff
  (§1.4).
- **Intraline highlighting doesn't come cleanly from git.** `--word-diff=porcelain` can't be laid
  over the line patch: its `~` newline marker doesn't say which side it belongs to, and
  whitespace-only changes vanish **(tested)**. Word-level highlights would come from a crate run
  on each paired `-`/`+` line (§4).
- **Rust diff crates are small.** `imara-diff` adds 36 KiB and 3 crates; `similar` adds 127 KiB
  (269 KiB with its inline word diff) and 1 crate **(tested)**. Diffing blobs in-process instead
  of git loses git's config unless parterre re-reads it; for display alone it gains little (§2).
- **External engines are poor fits.** difftastic has the best alignment and a JSON output, but the
  JSON is marked unstable, the binary is 113 MiB, and users would have to install it. delta only
  writes ANSI terminal text for a fixed width (§3).
- **Syntax highlighting is the expensive part.** syntect adds 2.2 MiB and 11 crates, including
  `bincode` 1.3.3, which RustSec lists as unmaintained. tree-sitter adds about 2.7 MiB with one
  grammar and needs a C compiler. egui_extras' built-in fallback adds 42 KiB but knows only
  C/C++, Python, Rust and TOML keywords (§5).
- **Prior art:** gitui renders libgit2's patch without syntax colour; GitButler diffs in-process
  with gitoxide (imara-diff), then does word diffs and syntax colour in the web front end.
  TortoiseGit extracts both files and hands them to TortoiseGitMerge, which diffs them itself
  with libsvn_diff (§7).
- **Recommendation (§11):** first version = git's patch, parsed as bytes in `parterre-core`,
  unified and side-by-side views, no new crate. Later, if wanted: `similar` for intraline
  highlights. Syntax colour later still, and only after its own discussion.

---

## 1. git's own output

### 1.1 Which command

parterre already asks git for a commit's changed files with `git diff-tree -r -M --root
--diff-merges=first-parent ... --no-textconv -z --raw --numstat <commit>`
(`crates/parterre-core/src/git.rs`, `Git::changed_files`). For the diff of one of those files
there are three git commands to choose from:

| Command | `diff.algorithm` | textconv | `diff` attribute | Notes |
|---|---|---|---|---|
| `git diff-tree -p <commit> -- <path>` | ignored **(tested)** | off by default | yes | plumbing |
| `git diff <parent> <commit> -- <old> <new>` | honoured **(tested)** | on by default | yes | porcelain; rename needs both paths in the pathspec **(tested)** |
| `git diff <parent>:<old> <commit>:<new>` | honoured | on **(tested)** | yes, by path **(tested)** | the blob form; no rename logic needed |

Evidence:
- On a file where Myers and histogram differ, `git -c diff.algorithm=histogram diff-tree -p`
  printed the Myers result; `git -c diff.algorithm=histogram diff` printed the histogram result
  **(tested)**.
- "textconv filters are enabled by default only for git-diff and git-log, but not for
  git-format-patch or diff plumbing commands"
  ([GITDOC diff-options.adoc#L838-L847](https://github.com/git/git/blob/v2.55.0/Documentation/diff-options.adoc#L838-L847)).
- Several `diff.*` settings "affect only `git diff` Porcelain, and not lower level `diff`
  commands", e.g. `diff.renames`
  ([GITDOC config/diff.adoc#L159-L164](https://github.com/git/git/blob/v2.55.0/Documentation/config/diff.adoc#L159-L164)).
- The blob form: "view the differences between the raw contents of two blob objects"
  ([GITDOC git-diff.adoc#L122-L125](https://github.com/git/git/blob/v2.55.0/Documentation/git-diff.adoc#L122-L125)).
  Written as `<rev>:<path>`, git still knows the paths: `diff=upper` textconv and `-diff` were
  both applied **(tested)**.
- With only the new path in the pathspec, a renamed file shows as added; with both paths it
  shows as a rename with a small patch **(tested)**.

A root commit (or a shallow boundary) is compared with the empty tree, as `changed_files` already
does **(derived)**. For a merge, `<parent>` is the first parent, matching the changed-files list
**(derived)**.

### 1.2 What git's patch honours

| Feature | Where it is configured | In `git diff`'s patch |
|---|---|---|
| Algorithm | `diff.algorithm` (`myers` default, `minimal`, `patience`, `histogram`) ([config/diff.adoc#L225-L240](https://github.com/git/git/blob/v2.55.0/Documentation/config/diff.adoc#L225-L240)) | yes **(tested)** |
| Per-path algorithm | `diff=<name>` attribute plus `diff.<name>.algorithm` ([gitattributes.adoc#L792-L822](https://github.com/git/git/blob/v2.55.0/Documentation/gitattributes.adoc#L792-L822)) | yes **(tested)** |
| Binary | `-diff` / `binary` attribute, else NUL in the first 8000 bytes ([gitattributes.adoc#L744-L756](https://github.com/git/git/blob/v2.55.0/Documentation/gitattributes.adoc#L744-L756), [xdiff-interface.c#L197-L201](https://github.com/git/git/blob/v2.55.0/xdiff-interface.c#L197-L201)) | "Binary files a/x and b/x differ" **(tested)** |
| Big files | `core.bigFileThreshold`, default 512 MiB: treated as binary, no diff ([config/core.adoc#L460-L480](https://github.com/git/git/blob/v2.55.0/Documentation/config/core.adoc#L460-L480)) | yes |
| textconv | `diff=<driver>` + `diff.<driver>.textconv` | yes **(tested)** |
| Renames | `-M` / `diff.renames` | yes, if both paths are given **(tested)** |
| Moved lines | `--color-moved` | only as colours in ANSI output (§1.5) |
| Word diff | `--word-diff` | separate output, see §4 |

Two caveats:
- **Attributes come from the working tree, not from the commit.** `--attr-source=<tree-ish>`
  reads them from a commit instead
  ([GITDOC git.adoc#L229-L232](https://github.com/git/git/blob/v2.55.0/Documentation/git.adoc#L229-L232);
  git 2.43 accepted it **(tested)**). Which one TortoiseGit's behaviour matches is an open
  question **(derived)**.
- **Counts can disagree.** The changed-files list asks for `--no-textconv`, so its line counts
  are for the raw blobs. A textconv'd patch can show different lines (or text where the list says
  binary) **(derived)**.

### 1.3 Flags to pin

The porcelain reads the user's config, and some settings change the output format. Probes on
git 2.43 **(tested)**:

| Setting | Effect on a parser | Pin |
|---|---|---|
| `color.diff=always` | ANSI codes, even with parterre's `-c color.ui=false` | `--no-color` |
| `diff.external`, `diff.<driver>.command` | git runs another program; with `/bin/false` git died with "external diff died" | `--no-ext-diff` |
| `diff.noprefix`, `diff.mnemonicPrefix`, `diff.srcPrefix` | `---`/`+++` headers change | `--default-prefix` (accepted by 2.43), or ignore headers |
| `diff.suppressBlankEmpty` | an empty context line is printed as `""`, not `" "` | parser treats an empty line as context |
| `diff.context`, `diff.interHunkContext` | hunk sizes | `-U<n>` explicitly |
| `core.quotepath` | octal-escaped paths in headers | already `-c core.quotepath=off` |

Output must be read as **bytes**. Git copies file bytes into the patch unchanged: Latin-1 `é`
arrived as `0xE9`, and CRLF lines kept their `\r` **(tested)**. parterre's `Git::run` turns stdout
into a `String` with `from_utf8_lossy`, which would already replace such bytes. A diff reader
needs its own byte-level path **(derived)**.

### 1.4 Is a unified patch enough for a side-by-side view?

Yes, with one loss **(derived)**:
- Each hunk header gives old and new start lines. Context lines go on both sides. A run of `-`
  lines followed by a run of `+` lines is laid out row by row, with blank filler on the shorter
  side. That is what delta and GitButler do with the same input.
- Whole-file side by side: ask for `-U<very large>` (a 10.8 MB file gave an 11 MB patch in
  117 ms **(tested)**), or fetch both blobs with `git cat-file` and fill the gaps between hunks
  from them. The hunk line numbers say exactly which lines are unchanged.
- **What is lost:** inside a changed block, git doesn't say which old line corresponds to which
  new line. If one line is inserted in the middle of an edited block, position pairing
  mismatches the lines below it. Tools that align properly compute their own diff: TortoiseGitMerge
  (libsvn_diff, §7), difftastic (`aligned_lines` in its JSON, §3.1). delta pairs lines whose edit
  distance is under `--max-line-distance` (default 0.6) (DELTA `--help`).
- Moved blocks show as a deletion in one place and an insertion in another.

### 1.5 Moved lines

`--color-moved` exists only as colouring
([GITDOC diff-options.adoc#L360-L368](https://github.com/git/git/blob/v2.55.0/Documentation/diff-options.adoc#L360-L368)).
Without `--color` the patch is unchanged **(tested)**. Recovering moves would mean asking for
colour, pinning each `color.diff.*Moved*` slot to a distinct code with `-c`, and decoding ANSI.
Possible, but fragile **(derived)**. Not worth it for a first version.

### 1.6 Speed

On a 10.8 MB, 300,000-line file with 300 changed lines **(tested)**:

| Command | Time | Output |
|---|---|---|
| `git diff` (Myers) | 97–110 ms | 101 KB, 2,701 lines |
| `git diff --diff-algorithm=histogram` | 287–298 ms | same |
| `git diff --diff-algorithm=patience` | 115–122 ms | same |
| `git diff -U2147483647` (whole file) | 117 ms | 11.1 MB |
| `git diff --word-diff=porcelain` | 96 ms | 103 KB |
| `git cat-file blob` (old side) | 15 ms | 10.8 MB |
| one changed 350 KB line: `git diff` | 4 ms | 698 KB |

Fast enough to run on the log window's worker thread for each file click **(derived)**.

## 2. Rust diff crates (diff in-process)

### 2.1 Candidates

Numbers from crates.io on 2026-09-26; sizes from §10.

| Crate | Version | Owners | Downloads (all / 90 days) | Reverse deps | Licence | New crates in parterre | Binary added |
|---|---|---|---|---|---|---|---|
| [`similar`](https://github.com/mitsuhiko/similar) | 3.2.0 (2026-08-17) | mitsuhiko (Armin Ronacher), who wrote nearly all of it (249 commits) | 204 M / 52.7 M | 1,123 | Apache-2.0 | 1 | 127 KiB; 269 KiB with `inline` |
| [`imara-diff`](https://github.com/pascalkuthe/imara-diff) | 0.2.0 (2025-06-14) | pascalkuthe (Helix maintainer); Byron contributes | 32.7 M / 9.2 M | 63 | Apache-2.0 | 3 (`hashbrown` 0.15 as a second version, `foldhash`) | 36 KiB |
| [`gix-imara-diff`](https://github.com/GitoxideLabs/gitoxide) | 0.3.0 (2026-09-25) | Byron | 3.2 M / 2.5 M | – | Apache-2.0 | gitoxide's fork, used by `gix-diff` | not measured |
| [`diffy`](https://github.com/bmwill/diffy) | 0.5.2 | bmwill | 16.3 M / 4.0 M | – | MIT/Apache-2.0 | – | not measured |

Apache-2.0 is compatible with parterre's GPL-3.0 **(derived)**.

Algorithms and features, from the crate sources:
- `similar`: Myers (with heuristics), raw Myers, patience, LCS, Hunt, histogram
  (`src/types.rs`). Deadlines bound the work. The `inline` feature refines a replaced block into
  emphasised words or characters, with a minimum similarity ratio and optional semantic
  clean-up (`src/text/inline.rs`). No required dependencies.
- `imara-diff`: Myers and histogram, git's slider/indent heuristics (`postprocess_lines`), and a
  unified-diff printer. It claims up to 30× faster than `similar` on Linux kernel diffs
  (`src/lib.rs` docs). Measured here: Myers 25 ms and histogram 85 ms on the 10.8 MB file,
  against `similar`'s 150 ms Myers and 190–210 ms patience **(tested)**.

### 2.2 What diffing in-process would mean

parterre would fetch both sides with `git cat-file` (15 ms for 10.8 MB **(tested)**) and diff
them with the crate. Compared with git's patch **(derived)**:

- **Lost unless re-implemented:** `diff.algorithm` and the per-path `diff.<name>.algorithm` (the
  crate would need parterre to read the config and map it; `minimal` has no match), the indent
  heuristic defaults, binary detection (NUL in the first 8000 bytes, `core.bigFileThreshold`),
  the `-diff`/`binary` attributes (`git check-attr diff -- <path>` works **(tested)**), textconv
  (`git cat-file --textconv <rev>:<path>` works for one object **(tested)**; the `--batch` form
  failed on the second object in my probe).
- **Gained:** full control of alignment and of the blobs (for whole-file side by side), and one
  diff engine for lines and words.
- GitButler does exactly this, but through gitoxide's `diff_resource_cache`, which handles
  attributes and textconv for it, and hands back the algorithm to use
  (GB `crates/but-core/src/unified_diff.rs`, §7). That gitoxide reads that algorithm from
  `diff.algorithm` is **(unverified)**.
  parterre would have to glue those pieces together by hand.
- It also conflicts with the reason for using the git CLI: "honours every repo configuration"
  (`docs/architecture.md`, "Why these choices").

For line diffs, git already does the job. Where a crate earns its place is **intraline**
highlighting on lines git has already paired (§4).

## 3. External engines and renderers

### 3.1 difftastic

- **What it is:** a structural diff that parses both files with vendored tree-sitter grammars
  (130 languages in 0.71.0 **(tested)**). MIT licence, 25.9k stars, active (pushed 2026-09-22).
- **Output:** side-by-side and inline text with ANSI colour, and `--display=json`. The JSON gives
  `aligned_lines` (old/new line pairs, i.e. real side-by-side alignment) and per-line change
  spans **(tested)**. But without `DFT_UNSTABLE=yes` it refuses: "JSON output is an unstable
  feature and its format may change in future" **(tested)**.
- **Git integration:** as `diff.external`/`GIT_EXTERNAL_DIFF` or as a difftool; the manual
  prefers `diff.external` because git passes more information that way
  ([DFT git.html](https://difftastic.wilfred.me.uk/git.html)).
- **Limits:** files over `--byte-limit` (default 1,000,000 bytes) fall back to a line diff
  (DFT `--help`). The 10.8 MB file took about 1 s and produced 4.7 MB of JSON **(tested)**.
- **Encodings:** it guesses per file. With a Latin-1 file where only one line changed, it read
  the old side as UTF-8 (showing `�`) and the new side as Latin-1, so every line showed as changed
  **(tested)**.
- **Availability:** prebuilt for Linux, Windows and macOS; also Homebrew, WinGet, Scoop,
  Chocolatey and Linux distributions
  ([DFT installation](https://difftastic.wilfred.me.uk/installation.html)). The Linux x86_64
  binary is 118.8 MB uncompressed (11.7 MB `.tar.gz`) **(tested)**. Bundling it would make
  parterre ten times larger; not bundling it means users install it themselves **(derived)**.

### 3.2 delta

- **What it is:** a pager that reads git's patch and rewrites it with syntax colour (syntect and
  bat's syntaxes), side-by-side layout and intraline highlights. MIT licence, 32.3k stars.
- **Output:** ANSI terminal text only; the only JSON in `--help` is for `rg --json` input
  **(tested)**. Side by side is laid out for a fixed `--width` with box-drawing characters
  **(tested)**. Drawing it in egui would mean a terminal-cell emulator, and the layout would not
  follow the window width **(derived)**.
- **Availability:** Linux, Windows and macOS through release binaries and package managers
  ([DELTA installation](https://dandavison.github.io/delta/installation.html)); the 0.19.2 assets
  lack an x86_64 macOS build (release asset list, **tested**). Binary 7.2 MB **(tested)**.
- What is useful from delta is its method, not its output: pair `-`/`+` lines by similarity, then
  diff words inside each pair (`--max-line-distance`, `--word-diff-regex`, `--max-line-length`
  3000 by default; DELTA `--help`).

### 3.3 `git difftool`

`git difftool --extcmd=<cmd>` writes the two sides to temporary files and starts a program with
them. There is no output format to draw from. parterre can get the same two files itself with
`git cat-file` **(derived)**. An "Open in external diff tool" command (TortoiseGit has one) is a
separate feature, not a diff engine.

## 4. Intraline and word-level highlighting

### 4.1 From git

`git diff --word-diff=porcelain` prints runs starting with ` `, `-` or `+`, and a `~` line for
each newline
([GITDOC diff-options.adoc#L435-L456](https://github.com/git/git/blob/v2.55.0/Documentation/diff-options.adoc#L435-L456)).
Its hunks have the same headers as the line patch **(tested)**. But laying it over the line
patch fails **(tested, 8 files, Myers and histogram)**:
- `~` is emitted once for both sides. Where one side has lines the other lacks (a moved function
  in `code.c`), rebuilding old and new lines from the runs gives extra empty lines.
- Whitespace-only changes vanish: `"  trailing   "` → `"  trailing"` shows no change.
- A missing newline at end of file adds an extra line.
- With `--word-diff-regex`, text between matches is dropped from the output, so lines can't be
  rebuilt at all.

It was fine on simple one-line edits, CRLF, Latin-1 and the 350 KB line. A reader could use it as
a second git call and fall back when the rebuilt lines don't match **(derived)**. That is fragile
and costs a second diff per file.

### 4.2 Computed on the changed lines

Take each `-` run and the `+` run after it, pair lines, and diff each pair by words or
characters **(derived)**. This is what delta and GitButler do. It needs no change to the line
diff, so git stays the source of truth.
- GitButler pairs only when both runs have the same number of lines, skips lines over 300
  characters, and uses `diff-match-patch` with semantic clean-up
  ([GB diffParsing.ts#L295-L300](https://github.com/gitbutlerapp/gitbutler/blob/bfc4c3d4ece16c68edfe42a9049813bc4b921dc5/packages/ui-svelte/src/lib/utils/diffParsing.ts#L295-L300),
  [#L655-L690](https://github.com/gitbutlerapp/gitbutler/blob/bfc4c3d4ece16c68edfe42a9049813bc4b921dc5/packages/ui-svelte/src/lib/utils/diffParsing.ts#L655-L690)).
- `similar`'s `iter_inline_changes` does the pairing and the refinement in one call, with a
  minimum ratio below which it leaves the lines unemphasised (`src/text/inline.rs`). On the
  350 KB one-line change it took 11 ms and marked 2 spans **(tested)**. Cost: 269 KiB, 1 crate.
- `imara-diff` can diff any token sequence (`Diff::compute_with`). parterre would write the
  tokeniser and the pairing, not a diff algorithm. Cost: 36 KiB, 3 crates.

## 5. Syntax highlighting

### 5.1 Options

Sizes and crate counts from §10; crates.io data 2026-09-26.

| Option | Binary added | New crates | C compiler | Languages | Supply-chain notes |
|---|---|---|---|---|---|
| egui_extras fallback (`syntax_highlighting`, no `syntect`) | 42 KiB | 3 | no | keywords, comments and strings for C/C++, Python, Rust, TOML only ([source](https://github.com/emilk/egui/blob/0.36.2/crates/egui_extras/src/syntax_highlighting.rs)) | owned by emilk/rerun, the egui owners |
| syntect, minimal (`default-syntaxes`, `default-themes`, `regex-fancy`) | 2.20 MiB | 11 | no | 75 syntaxes **(tested)** | owners trishume, robinst, Enselic, keith-hall; 28.8 M downloads; pulls `bincode` 1.3.3 |
| syntect `default-fancy` (what egui_extras' `syntect` feature uses) | 2.21 MiB | 26 (29 via egui_extras) | no | same | also `yaml-rust`, `plist`, `serde_json`, `time` |
| syntect default (Oniguruma) | 1.05 MiB | 23 | **yes** (`onig_sys`) | same | C library |
| two-face (bat's syntaxes) + syntect minimal | 2.77 MiB | 12 | no | bat's set | one owner (CosmicHorrorDev); syntaxes under their own licences |
| tree-sitter + `tree-sitter-highlight` + Rust grammar | 2.71 MiB | 9 | **yes** (grammar `parser.c`, runtime) | 1 | tree-sitter org; each grammar is a separate crate |
| tree-sitter + 4 grammars (Rust, C, Python, JavaScript) | 4.15 MiB | 12 | **yes** | 4 | about 0.5 MiB per extra grammar |

Notes:
- `bincode` is marked unmaintained: "Due to a doxxing and harassment incident, the bincode team
  has taken the decision to cease development permanently. The team considers version 1.3.3 a
  complete version" ([RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html)).
  syntect needs it to load its bundled dumps.
- `yaml-rust` (in `default-fancy`) is also unmaintained
  ([RUSTSEC-2024-0320](https://rustsec.org/advisories/RUSTSEC-2024-0320.html)).
- tree-sitter and Oniguruma compile C sources in build scripts (`cargo tree -e build -i cc`
  shows `tree-sitter`, `tree-sitter-rust` and `onig_sys` **(tested)**). That breaks "keeps the
  build free of C dependencies" (`docs/architecture.md`).
- The bundled syntax definitions come from Sublime Text and bat packages with their own licences;
  two-face lists them in an `acknowledgement` module. The third-party notices
  (`packaging/about.toml`) cover crates, not assets inside them **(derived)**.

### 5.2 Speed

On `crates/parterre-core/src/physics.rs` (2,403 lines) **(tested)**: syntect 180 ms to
highlight (loading the syntax set is lazy: 0.6 ms); tree-sitter 12 ms to build the Rust
configuration and 13 ms to highlight.

### 5.3 Highlighting a diff correctly

Highlighting hunk lines alone loses state: a hunk that starts inside a block comment or a string
is coloured wrongly. The fix is to highlight both whole blobs and pick the lines, which needs both
blobs from `git cat-file` **(derived)**. GitButler highlights line by line and accepts the errors
(GB `diffParsing.ts`, `toTokens`).

### 5.4 Worth it for a first version?

**(derived)** No. TortoiseGit's own diff viewer (TortoiseGitMerge) shows no syntax colour by
default; TortoiseGitUDiff colours only `+`/`-` lines **(unverified)**. gitui shows diffs without
syntax colour (§7). The cheapest option costs 2.2 MiB (about 18 % of today's 12.8 MB binary) and
brings an unmaintained dependency. It can be added later without changing the diff model.

## 6. Large and awkward inputs

| Input | git patch | In-process crate | difftastic | Drawing in egui |
|---|---|---|---|---|
| Huge file (10.8 MB) | 0.1 s Myers, 0.3 s histogram; only hunks sent **(tested)** | imara 25–85 ms, similar 150–210 ms, plus 15 ms per blob fetch **(tested)** | falls back to a line diff above 1 MB; ~1 s, 4.7 MB JSON **(tested)** | `ScrollArea::show_rows` draws only visible rows ([egui scroll_area.rs](https://github.com/emilk/egui/blob/0.36.2/crates/egui/src/containers/scroll_area.rs)); the log window already uses it |
| Over `core.bigFileThreshold` (512 MiB) or ~1 GiB | treated as binary; xdiff refuses above `MAX_XDIFF_SIZE` (1023 MiB) ([xdiff-interface.h#L14](https://github.com/git/git/blob/v2.55.0/xdiff-interface.h#L14)) | parterre must set its own limit | byte limit | show "too large" |
| Very long line (350 KB) | 4 ms; 698 KB patch **(tested)** | similar inline 11 ms **(tested)** | – | laying out a 350 KB line each frame is slow; cut lines (delta cuts at 3000 chars, GitButler skips word diff over 300) **(derived)** |
| Binary | "Binary files … differ" **(tested)**; parterre's numstat already says binary | parterre must detect | `--override-binary` | show sizes (`git cat-file -s`) instead |
| CRLF | `\r` kept at line end **(tested)**; `--ignore-cr-at-eol` available | bytes as given | – | strip one trailing `\r` when drawing, maybe show a marker |
| Latin-1 / Windows-1252 | bytes passed through **(tested)** | same | guesses per side; spurious changes **(tested)** | needs decoding (§6.1) |
| UTF-16 | binary to git (NULs) **(tested)**; readable with textconv (`iconv`) or `working-tree-encoding`, which stores UTF-8 in the repo ([gitattributes.adoc#L305-L312](https://github.com/git/git/blob/v2.55.0/Documentation/gitattributes.adoc#L305-L312)) | same as git | – | – |
| No newline at end | `\ No newline at end of file` line **(tested)** | crate-specific | – | show a marker |

### 6.1 Decoding non-UTF-8 text

- The `encoding` attribute exists for exactly this: "the character encoding that should be used by
  GUI tools (e.g. gitk and git-gui) to display the contents"
  ([gitattributes.adoc#L1267-L1274](https://github.com/git/git/blob/v2.55.0/Documentation/gitattributes.adoc#L1267-L1274)).
  `git check-attr encoding -- <path>` reads it **(tested)**.
- `encoding_rs` (Firefox's decoder; owner hsivonen; 542 M downloads) adds 191 KiB and 4 crates.
  `chardetng` (same owner) adds guessing, 276 KiB and 5 crates in total **(tested)**. GitButler
  uses `chardetng` and keeps the bytes when the guess is not confident
  ([GB unified_diff.rs#L261-L276](https://github.com/gitbutlerapp/gitbutler/blob/bfc4c3d4ece16c68edfe42a9049813bc4b921dc5/crates/but-core/src/unified_diff.rs#L261-L276)).
- Without a crate: decode as UTF-8, and show invalid bytes as `\xNN` or as Latin-1 (a byte-to-char
  mapping, not an algorithm) **(derived)**. Guessing per side is what went wrong in difftastic;
  whatever is chosen should decode both sides the same way **(derived)**.

## 7. Prior art

| Tool | Diff source | Views | Intraline | Syntax colour | Encoding |
|---|---|---|---|---|---|
| **TortoiseGit** (Windows, C++) | Side by side: both files extracted (`CGit::GetOneFile`, libgit2 `git_blob_filter` or `git cat-file -p`), then TortoiseGitMerge diffs them with libsvn_diff (`svn_diff_file_diff_2`). Unified: `git diff-tree -r -p --stat` shown in TortoiseGitUDiff | both | TortoiseGitMerge: yes | TortoiseGitUDiff: `+`/`-` colours | TortoiseGitMerge detects |
| **gitui** (Rust, TUI) | libgit2 (`git2` 0.21) patch, lines through `from_utf8_lossy` with `\r\n` trimmed | unified | no | not in diffs; syntect + two-face only in the file viewer | lossy UTF-8 |
| **GitButler** (Rust + Svelte) | gitoxide in-process (`gix` diff resource cache: attributes, textconv, algorithm; `gix-imara-diff`); binary and too-large cases returned as such | unified (split rows in UI) | `diff-match-patch`, only for equal-sized runs, lines ≤ 300 chars | Shiki (JS), line by line | `chardetng` + `encoding_rs` |
| **lazygit** (Go, TUI) | `git show … --color=… -p` / `git diff --color=…`, drawn as ANSI; optional pager such as delta | unified, or the pager's | only via the pager | only via the pager | as git |

Citations:
- TortoiseGit: [GitDiff.cpp#L406-L474](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/GitDiff.cpp#L406-L474),
  [Git.cpp#L2777-L2869](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L2777-L2869),
  [Git.cpp#L3090-L3109](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/Git/Git.cpp#L3090-L3109),
  [TortoiseMerge/DiffData.cpp#L364-L370](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseMerge/DiffData.cpp#L364-L370),
  [AppUtils.cpp#L466-L553](https://github.com/TortoiseGit/TortoiseGit/blob/acc10fc20afe36aabc4afbc5f1af33f31acae32e/src/TortoiseProc/AppUtils.cpp#L466-L553).
  TortoiseGitMerge's intraline and encoding detection are from memory of the product
  **(unverified)**.
- gitui: [asyncgit/Cargo.toml#L20](https://github.com/gitui-org/gitui/blob/2fa693cb6ed431b21ebc300dd02e83c2476699ce/asyncgit/Cargo.toml#L20),
  [asyncgit/src/sync/diff.rs#L257-L400](https://github.com/gitui-org/gitui/blob/2fa693cb6ed431b21ebc300dd02e83c2476699ce/asyncgit/src/sync/diff.rs#L257-L400)
  (`from_utf8_lossy` at L307),
  [Cargo.toml#L61-L62](https://github.com/gitui-org/gitui/blob/2fa693cb6ed431b21ebc300dd02e83c2476699ce/Cargo.toml#L61-L62),
  [src/ui/syntax_text.rs#L37](https://github.com/gitui-org/gitui/blob/2fa693cb6ed431b21ebc300dd02e83c2476699ce/src/ui/syntax_text.rs#L37),
  used by [src/components/revision_files.rs#L52](https://github.com/gitui-org/gitui/blob/2fa693cb6ed431b21ebc300dd02e83c2476699ce/src/components/revision_files.rs#L52).
- GitButler: [crates/but-core/src/unified_diff.rs#L173-L258](https://github.com/gitbutlerapp/gitbutler/blob/bfc4c3d4ece16c68edfe42a9049813bc4b921dc5/crates/but-core/src/unified_diff.rs#L173-L258),
  [packages/ui-svelte/package.json#L64](https://github.com/gitbutlerapp/gitbutler/blob/bfc4c3d4ece16c68edfe42a9049813bc4b921dc5/packages/ui-svelte/package.json#L64) (Shiki),
  `diffParsing.ts` as in §4.2.
- lazygit: [pkg/commands/git_commands/commit.go#L243-L259](https://github.com/jesseduffield/lazygit/blob/1b39e38ae66996a9d15c14f41dc1cd1ae5b74327/pkg/commands/git_commands/commit.go#L243-L259).

**(derived)** The closest match to parterre's constraints (git CLI, no git library) is lazygit's:
ask git for the patch. The difference is that parterre must parse it rather than pass ANSI to a
terminal. TortoiseGit is the reference for behaviour, and its side-by-side viewer uses its own
diff engine; parterre can't copy that part without an engine.

## 8. Drawing in egui

Nothing here needs a new crate **(derived)**:
- One row per diff line, fixed height, through `ScrollArea::show_rows`, as the log window does.
- Colours per span with `LayoutJob` and `TextFormat::background`
  ([epaint text_layout_types.rs](https://github.com/emilk/egui/blob/0.36.2/crates/epaint/src/text/text_layout_types.rs)).
- Side by side: two columns sharing one vertical scroll offset; line numbers in a gutter.
- Keep the parser, the pairing and any intraline work in `parterre-core` (GUI-free, testable);
  the widget stays in `crates/parterre`.

## 9. Options side by side

| | A. git patch | B. git patch + intraline crate | C. blobs + diff crate | D. difftastic JSON | E. delta |
|---|---|---|---|---|---|
| Honours git config | yes | yes | only what parterre re-reads | no (own engine) | yes (input is git's patch) |
| Unified view | yes | yes | yes | yes | ANSI only |
| Side by side | position pairing | position pairing | own alignment possible | real alignment | ANSI, fixed width |
| Intraline | no | yes | yes | yes (tokens) | ANSI only |
| New crates | 0 | 1 (`similar`) or 3 (`imara-diff`) | 1–3 | 0 (but JSON parser: `serde_json` not in the tree today) | 0 |
| Binary added | 0 | 269 KiB / 36 KiB | 36–269 KiB | 0 (external 113 MiB) | 0 (external 6.8 MiB) |
| User installs something | no | no | no | yes | yes |
| Stability of the interface | git's patch format | same | crate API | "unstable", env var gate | not an interface |

## 10. Probes

All throwaway, in `/tmp`, not committed.

**Binary size.** A copy of parterre `origin/main` @ `aac2b84`, release profile as in the repo
(thin LTO, `codegen-units = 1`, `strip = "symbols"`). Each candidate is an optional dependency
behind a cargo feature; a `probe` module calls it for real (diff, inline changes, highlight,
decode) when an environment variable is set, so LTO keeps it. Size = stripped `parterre` binary;
new crates = `cargo tree -e normal --no-dedupe` minus the baseline. Baseline with the empty probe
module: 12,822,584 bytes.

| Feature | Bytes added | New crates |
|---|---|---|
| `similar` (TextDiff, unified printer) | 129,592 | 1 |
| `similar` + `inline` | 275,648 | 1 |
| `imara-diff` (histogram, Myers, unified printer) | 37,248 | 3 |
| `encoding_rs` | 195,136 | 4 |
| `encoding_rs` + `chardetng` | 282,840 | 5 |
| `egui_extras` fallback highlighter | 43,472 | 3 |
| `egui_extras` + `syntect` feature | 2,322,064 | 29 |
| syntect minimal, fancy-regex | 2,304,080 | 11 |
| syntect `default-fancy` | 2,318,192 | 26 |
| syntect default (Oniguruma) | 1,102,224 | 23 |
| two-face + syntect minimal | 2,907,592 | 12 |
| tree-sitter + highlight + Rust | 2,837,560 | 9 |
| tree-sitter + highlight + Rust, C, Python, JavaScript | 4,353,464 | 12 |

External programs, Linux x86_64 release assets: `difft` 0.71.0 118,816,568 bytes (11.7 MB
`.tar.gz`); `delta` 0.19.2 7,151,152 bytes (3.3 MB `.tar.gz`).

**git.** A throwaway repository with two commits changing: a C file where Myers and histogram
differ, a CRLF file, a Latin-1 file, a UTF-16LE file, a random binary (`binary` attribute), a text
file marked `-diff`, a file with a `diff=upper` textconv driver (`tr a-z A-Z`), a 350 KB one-line
file, a 10.8 MB 300,000-line file with 300 changed lines, a rename with an edit, a file without a
final newline, a word-level edit and a whitespace-only edit. Commands and results are quoted in
§1, §4 and §6. Timings are from a Python script calling git with `subprocess`.

**Word diff over line diff.** A script rebuilt each hunk's old and new lines from
`--word-diff=porcelain` and compared them with the context/`-`/`+` lines of the plain patch:
equal for 4 of 8 files, different for the moved function, the whitespace-only edit, the missing
final newline and the 300-hunk file; different for all 8 with a `--word-diff-regex`.

**Speed of crates.** A separate release binary with `similar`, `imara-diff`, syntect (minimal) and
tree-sitter + Rust, run twice on the 10.8 MB file pair and on `physics.rs`.

**difftastic and delta.** Release binaries run on the extracted blobs and on a saved patch.

## 11. Recommendation

**(derived)** In order, each step usable on its own:

1. **First version: git's patch, no new crate.** In `parterre-core`, run `git diff --no-color
   --no-ext-diff --default-prefix -M -U<n> <parent> <commit> -- <old> <new>` (or the
   `<rev>:<path>` blob form), read stdout as bytes, and parse hunks into typed lines. Draw a
   unified view and a side-by-side view (position pairing) with `show_rows`. Binary files show
   their sizes. Decode as UTF-8 and show invalid bytes visibly. Cut very long lines. This keeps
   git's config, adds nothing to the binary and fits the "git CLI" choice.
2. **If intraline highlights are wanted: `similar` with `inline`.** 269 KiB, one crate with no
   dependencies, one well-known owner, widely used. It does the pairing and word refinement in
   one call. `imara-diff` is the smaller alternative (36 KiB, 3 crates) but leaves more glue code
   to parterre.
3. **Syntax colour: not now.** If it is wanted later, syntect with minimal features is the only
   pure-Rust candidate with broad language coverage; weigh its 2.2 MiB and the unmaintained
   `bincode` first. tree-sitter would need a C compiler and grows per language.
4. **External engines: no.** Neither difftastic nor delta gives egui something stable to draw.
   An "Open in external diff tool" command is a separate, optional feature.

Open questions for the discussion:
- Attributes from the working tree (git's default) or from the commit (`--attr-source`)?
- Honour textconv in the viewer, while the changed-files list uses `--no-textconv`?
- Is position pairing in side by side good enough, given that TortoiseGitMerge aligns better?
- Show non-UTF-8 bytes raw, as Latin-1, or via the `encoding` attribute with `encoding_rs`?
