# parterre – notes for agents

Standalone TortoiseGit-style revision graph viewer. Rust workspace, egui/eframe GUI.

- `crates/parterre-core` must stay free of GUI dependencies; put anything testable there.
- `crates/parterre` is the app; keep rendering and interaction there.
- Behavioural reference for what TortoiseGit does: `docs/research/tortoisegit-revision-graph.md`.
  When deviating from TortoiseGit on purpose, say so in a comment and in `TODO.md`.
- Open questions for the human go in `TODO.md` under "Open questions (HITL)".
- A `TODO.md` item keeps its number for good, so that "question 12" never comes to mean
  something else: never renumber an item, and never reuse a number, even one whose item was
  deleted. A new item takes the next number noted in `TODO.md`; bump that note in the same
  change. If two branches took the same number, the item merged later takes the next free one.

Commands (Rust from `~/.cargo/bin`):

- `cargo test --workspace` – tests; integration tests create throwaway repos with the git CLI.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` before committing.
- `cargo run --release -p parterre-core --example stats -- <repo>` – graph sizes and timings.
- `cargo run --release -- <repo> --screenshot out.png` – render one frame to a PNG (for checking
  visuals without a human).
