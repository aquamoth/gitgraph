# parterre

A standalone, fast, native re-creation of TortoiseGit's **Revision Graph**: a compact,
tree-like picture of how the branches and tags of a git repository relate, in a resizable window
that runs on Linux and Windows. On top of the TortoiseGit look you can rearrange the graph by
hand.

![parterre showing a demo repository](https://raw.githubusercontent.com/aquamoth/parterre/main/docs/images/demo.png)

```sh
cargo install --locked parterre    # build from source
cargo binstall parterre            # or download the release binary
parterre [PATH]                    # show the repository containing PATH (default: .)
```

parterre needs `git` on `PATH` at runtime; it reads the repository with `git log` and
`git for-each-ref` and never writes to it. `cargo install` installs only the binary, without a
desktop entry or icon; installers and packages are on the
[releases page](https://github.com/aquamoth/parterre/releases).

Usage, keys and options are in the [README on GitHub](https://github.com/aquamoth/parterre#readme).

## License

parterre is free software under the GNU General Public License, version 3 only, with two
additional terms in [NOTICE](https://github.com/aquamoth/parterre/blob/main/NOTICE): works
based on parterre keep its copyright notice and say that they are based on it, and modified
versions are marked as modified.
