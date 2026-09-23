# gitgraph – notes for agents

Standalone TortoiseGit-style revision graph viewer. Rust workspace, egui/eframe GUI.

- `crates/gitgraph-core` must stay free of GUI dependencies; put anything testable there.
- `crates/gitgraph` is the app; keep rendering and interaction there.
- Behavioural reference for what TortoiseGit does: `docs/research/tortoisegit-revision-graph.md`.
  When deviating from TortoiseGit on purpose, say so in a comment and in `TODO.md`.
- Open questions for the human go in `TODO.md` under "Open questions (HITL)".

Commands (Rust from `~/.cargo/bin`):

- `cargo test --workspace` – tests; integration tests create throwaway repos with the git CLI.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` before committing.
- `cargo run --release -p gitgraph-core --example stats -- <repo>` – graph sizes and timings.
- `cargo run --release -- <repo> --screenshot out.png` – render one frame to a PNG (for checking
  visuals without a human).
