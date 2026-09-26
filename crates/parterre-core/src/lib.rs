//! Core of parterre: load a repository, reduce it to a TortoiseGit-style revision graph,
//! and lay that graph out. Nothing in this crate depends on a GUI toolkit.

pub mod git;
pub mod glyphs;
pub mod icon;
pub mod layout;
pub mod oid;
pub mod pattern;
pub mod physics;
pub mod recent;
pub mod repo;
pub mod revgraph;
pub mod route;

pub use oid::Oid;
pub use repo::{Commit, CommitIx, GitRef, Head, RefKind, Repo};
