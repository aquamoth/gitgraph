//! Reloading by itself when the repository's refs change: a commit, checkout or fetch made
//! outside parterre shows up without pressing F5.
//!
//! Deliberate deviation from TortoiseGit, whose revision graph reloads only on F5.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use eframe::egui;
use parterre_core::Repo;
use parterre_core::watch::RefStorage;

/// How often the ref files are looked at.
const INTERVAL: Duration = Duration::from_secs(1);
/// How long they must stay unchanged before loading, so that a rebase or a fetch of many
/// branches is loaded once, when it is done, rather than halfway.
const SETTLE: Duration = Duration::from_millis(300);

/// Watches one repository on a worker thread, and loads it again when its refs change.
#[derive(Debug)]
pub struct Watcher {
    path: PathBuf,
    loaded: Receiver<Repo>,
    /// Dropping it stops the thread.
    _stop: Sender<()>,
}

impl Watcher {
    pub fn start(path: &Path, ctx: &egui::Context) -> Watcher {
        let (stop, stopped) = mpsc::channel();
        let (send, loaded) = mpsc::channel();
        let (dir, ctx) = (path.to_owned(), ctx.clone());
        std::thread::spawn(move || watch(&dir, &stopped, &send, &ctx));
        Watcher {
            path: path.to_owned(),
            loaded,
            _stop: stop,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The newest snapshot loaded since the last call, if any. It may have the same refs as
    /// the one shown ([`Repo::same_refs`]).
    pub fn take(&self) -> Option<Repo> {
        self.loaded.try_iter().last()
    }
}

fn watch(dir: &Path, stopped: &Receiver<()>, send: &Sender<Repo>, ctx: &egui::Context) {
    // Waits `d`; false once the watcher has been dropped.
    let wait = |d| matches!(stopped.recv_timeout(d), Err(RecvTimeoutError::Timeout));
    let Ok(storage) = RefStorage::locate(dir) else {
        return;
    };
    let mut seen = storage.fingerprint();
    while wait(INTERVAL) {
        let mut now = storage.fingerprint();
        if now == seen {
            continue;
        }
        loop {
            if !wait(SETTLE) {
                return;
            }
            let later = storage.fingerprint();
            if later == now {
                break;
            }
            now = later;
        }
        seen = now;
        // A failure (say, halfway through a git command) is retried at the next change.
        if let Ok(repo) = parterre_core::git::load_repo(dir) {
            if send.send(repo).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}
