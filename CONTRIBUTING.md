# Contributing

Bug reports and ideas are welcome: open an issue for either.

Code is another matter. parterre is a small personal project, and pull requests are unlikely to
be merged unless the change was discussed in an issue first.

## If you still want to open a pull request

- Open an issue about it first, and wait for an answer.
- Keep it small: one change, no unrelated fixes or reformatting.
- Explain what changed and why.
- For anything visible in the window, include before and after screenshots.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace`.

Opening a pull request creates no obligation to review or merge it. It may be closed, left
alone, or the idea done differently later.

## License of contributions

parterre is released under the GNU General Public License, version 3 only, with the additional
terms in [NOTICE](NOTICE).

By submitting a contribution you license it under the
[MIT No Attribution license](https://spdx.org/licenses/MIT-0.html) (MIT-0), and confirm that you
have the right to do so. MIT-0 lets anyone use, modify, relicense and distribute your
contribution for any purpose, without conditions. In parterre it is distributed under the
project's license, like the rest of the code.

This applies only to contributions from people other than the copyright holder named in
[NOTICE](NOTICE). The rest of parterre, including that copyright holder's own changes, is
released only under the license above.
