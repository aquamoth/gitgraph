//! Open pull requests from GitHub, loaded on a worker thread while they are shown. Not in
//! TortoiseGit.
//!
//! Whether `origin` is on GitHub is asked of git alone, when a repository is opened. GitHub
//! itself is asked only while pull requests are shown: when they are turned on, when a
//! repository is opened with them on, and on F5. Reloads by themselves (when the refs change)
//! keep the list they have: the unauthenticated limit is 60 requests an hour.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui;
use parterre_core::forge::github::{self, GithubRepo};
use parterre_core::forge::{ForgeError, PullRequests};
use parterre_core::git::Git;

/// What a finished load brought.
#[derive(Debug)]
pub enum Loaded {
    /// A list of this many pull requests.
    Found(usize),
    Failed(String),
}

#[derive(Debug, Default)]
pub struct PullRequestLoader {
    /// The repository everything here is about.
    path: Option<PathBuf>,
    /// The GitHub repository its `origin` points at, if any.
    origin: Option<GithubRepo>,
    list: Option<Arc<PullRequests>>,
    job: Option<Receiver<Result<PullRequests, ForgeError>>>,
    /// The last load failed; don't try again by itself.
    failed: bool,
}

impl PullRequestLoader {
    /// Follows the repository shown (`None` for none), forgetting everything about the one
    /// before when it changes.
    pub fn follow(&mut self, path: Option<&Path>) {
        if self.path.as_deref() == path {
            return;
        }
        *self = PullRequestLoader {
            path: path.map(Path::to_owned),
            origin: path.and_then(|p| github::origin(&Git::new(p))),
            ..PullRequestLoader::default()
        };
    }

    /// The GitHub repository `origin` points at: pull requests can be shown only if there is
    /// one.
    pub fn origin(&self) -> Option<&GithubRepo> {
        self.origin.as_ref()
    }

    pub fn list(&self) -> Option<&Arc<PullRequests>> {
        self.list.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.job.is_some()
    }

    /// Loads the list if pull requests are `shown` and it hasn't been loaded (or failed to);
    /// returns what a load that has finished brought.
    pub fn update(&mut self, shown: bool, ctx: &egui::Context) -> Option<Loaded> {
        if shown && self.list.is_none() && !self.failed && self.job.is_none() {
            self.load(ctx);
        }
        let job = self.job.as_ref()?;
        let result = match job.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => {
                self.job = None;
                self.failed = true;
                return Some(Loaded::Failed("loading them stopped unexpectedly".into()));
            }
        };
        self.job = None;
        Some(match result {
            Ok(list) => {
                let count = list.list.len();
                self.list = Some(Arc::new(list));
                self.failed = false;
                Loaded::Found(count)
            }
            Err(e) => {
                self.failed = true;
                Loaded::Failed(e.to_string())
            }
        })
    }

    /// Loads the list again (F5). The list shown stays until the new one is in.
    pub fn reload(&mut self, ctx: &egui::Context) {
        self.failed = false;
        self.load(ctx);
    }

    /// Pull requests were turned on: try again if the last load failed.
    pub fn turned_on(&mut self) {
        self.failed = false;
    }

    /// Forgets the list (F5 while pull requests are hidden), so that showing them loads it
    /// afresh.
    pub fn forget(&mut self) {
        self.list = None;
        self.failed = false;
    }

    fn load(&mut self, ctx: &egui::Context) {
        let (Some(path), Some(_)) = (&self.path, &self.origin) else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let git = Git::new(path);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // The receiver is gone if another repository was opened meanwhile.
            let _ = tx.send(github::load(&git));
            ctx.request_repaint();
        });
        self.job = Some(rx);
    }
}

/// What the status bar says once `count` open pull requests have loaded, of which `here` have
/// their head in the repository.
pub fn loaded_status(count: usize, here: usize) -> String {
    let open = match count {
        0 => return "No open pull requests on GitHub".to_owned(),
        1 => "1 open pull request on GitHub".to_owned(),
        n => format!("{n} open pull requests on GitHub"),
    };
    match (count, count.saturating_sub(here)) {
        (_, 0) => open,
        (1, _) => format!("{open}, on a commit not fetched here"),
        (_, 1) => format!("{open}, 1 on a commit not fetched here"),
        (_, missing) => format!("{open}, {missing} on commits not fetched here"),
    }
}

#[cfg(test)]
mod tests {
    use super::loaded_status;

    #[test]
    fn status_counts_what_can_be_shown() {
        assert_eq!(loaded_status(0, 0), "No open pull requests on GitHub");
        assert_eq!(loaded_status(1, 1), "1 open pull request on GitHub");
        assert_eq!(
            loaded_status(1, 0),
            "1 open pull request on GitHub, on a commit not fetched here"
        );
        assert_eq!(loaded_status(5, 5), "5 open pull requests on GitHub");
        assert_eq!(
            loaded_status(5, 4),
            "5 open pull requests on GitHub, 1 on a commit not fetched here"
        );
        assert_eq!(
            loaded_status(63, 25),
            "63 open pull requests on GitHub, 38 on commits not fetched here"
        );
    }
}
