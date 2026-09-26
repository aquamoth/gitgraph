//! Which `git` to run.
//!
//! Everywhere but Windows, and on Windows whenever `git.exe` is on PATH, that's plain `git`. On
//! Windows it may not be on PATH yet, e.g. in a terminal opened before Git for Windows was
//! installed, or if its installer was told to leave PATH alone. Then parterre runs the
//! `cmd\git.exe` of a Git for Windows it finds through the registry or in the default folders.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The program to run for git, looked up once.
pub(super) fn git() -> &'static Path {
    static PROGRAM: OnceLock<PathBuf> = OnceLock::new();
    PROGRAM.get_or_init(find)
}

#[cfg(not(windows))]
fn find() -> PathBuf {
    PathBuf::from("git")
}

#[cfg(windows)]
fn find() -> PathBuf {
    locate(
        std::env::var_os("PATH").as_deref(),
        &windows::install_dirs(),
        Path::is_file,
    )
}

/// `git` if `path` (a PATH value) has a `git.exe`, otherwise the first `<dir>\cmd\git.exe` of
/// `install_dirs` that exists. Plain `git` if there is none either, so that spawning it fails
/// with the usual error.
#[cfg(any(windows, test))]
fn locate(
    path: Option<&std::ffi::OsStr>,
    install_dirs: &[PathBuf],
    is_file: impl Fn(&Path) -> bool,
) -> PathBuf {
    let on_path = path
        .into_iter()
        .flat_map(std::env::split_paths)
        .any(|dir| is_file(&dir.join("git.exe")));
    if !on_path {
        let found = install_dirs
            .iter()
            .map(|dir| dir.join("cmd").join("git.exe"))
            .find(|exe| is_file(exe));
        if let Some(exe) = found {
            return exe;
        }
    }
    PathBuf::from("git")
}

#[cfg(windows)]
mod windows {
    use std::path::PathBuf;

    use winreg::RegKey;
    use winreg::enums::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY,
    };

    /// Where Git for Windows may be installed, most likely first: the `InstallPath` its installer
    /// records (per-user, then machine-wide; 64-bit, then 32-bit), then its default folders.
    pub(super) fn install_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
                let path = RegKey::predef(root)
                    .open_subkey_with_flags(r"SOFTWARE\GitForWindows", KEY_READ | view)
                    .and_then(|key| key.get_value::<String, _>("InstallPath"));
                dirs.extend(path.map(PathBuf::from));
            }
        }
        let default = |var, rest: &str| std::env::var_os(var).map(|d| PathBuf::from(d).join(rest));
        dirs.extend(default("ProgramFiles", "Git"));
        dirs.extend(default("LOCALAPPDATA", r"Programs\Git"));
        dirs.extend(default("ProgramFiles(x86)", "Git"));
        dirs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;
    use std::ffi::OsString;

    fn path_var(dirs: &[&str]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    fn locate_with(path: &[&str], install_dirs: &[&str], files: &[PathBuf]) -> PathBuf {
        let files: HashSet<_> = files.iter().collect();
        let install_dirs: Vec<_> = install_dirs.iter().map(PathBuf::from).collect();
        locate(Some(&path_var(path)), &install_dirs, |p| {
            files.contains(&p.to_path_buf())
        })
    }

    fn exe(dir: &str) -> PathBuf {
        Path::new(dir).join("cmd").join("git.exe")
    }

    #[test]
    fn git_on_path_wins() {
        let files = [Path::new("bin").join("git.exe"), exe("installed")];
        assert_eq!(
            locate_with(&["other", "bin"], &["installed"], &files),
            Path::new("git")
        );
    }

    #[test]
    fn otherwise_the_first_install_dir_with_git() {
        let files = [exe("second"), exe("third")];
        assert_eq!(
            locate_with(&["bin"], &["first", "second", "third"], &files),
            exe("second")
        );
    }

    #[test]
    fn plain_git_when_nothing_is_found() {
        assert_eq!(locate_with(&["bin"], &["first"], &[]), Path::new("git"));
        assert_eq!(locate(None, &[], |_| false), Path::new("git"));
    }
}
