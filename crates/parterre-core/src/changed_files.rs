//! Changed files: what one commit changed compared with its first parent.
//!
//! Git computes them ([`crate::git::Git::changed_files`]); this module holds the result types,
//! the parser for git's output, and the path order the log window shows them in.

use std::cmp::Ordering;
use std::collections::HashMap;

/// How a file changed, from git's `--raw` status letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    /// The kind of entry changed, e.g. a file became a symlink.
    TypeChanged,
    Unmerged,
    /// Any other letter git may print (`X`, or future additions).
    Unknown,
}

impl FileStatus {
    fn from_letter(letter: u8) -> FileStatus {
        match letter {
            b'A' => FileStatus::Added,
            b'M' => FileStatus::Modified,
            b'D' => FileStatus::Deleted,
            b'R' => FileStatus::Renamed,
            b'C' => FileStatus::Copied,
            b'T' => FileStatus::TypeChanged,
            b'U' => FileStatus::Unmerged,
            _ => FileStatus::Unknown,
        }
    }

    /// The word for the status column: "Added", "Modified", ...
    pub fn name(self) -> &'static str {
        match self {
            FileStatus::Added => "Added",
            FileStatus::Modified => "Modified",
            FileStatus::Deleted => "Deleted",
            FileStatus::Renamed => "Renamed",
            FileStatus::Copied => "Copied",
            FileStatus::TypeChanged => "Type changed",
            FileStatus::Unmerged => "Unmerged",
            FileStatus::Unknown => "Unknown",
        }
    }
}

/// One file a commit changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    /// Path in the commit, relative to the repository root with `/` separators. For a deleted
    /// file, the path it had.
    pub path: String,
    /// The path in the parent, for renames and copies.
    pub old_path: Option<String>,
    pub status: FileStatus,
    /// Lines added; `None` for binary files.
    pub added: Option<u32>,
    /// Lines removed; `None` for binary files.
    pub removed: Option<u32>,
}

impl ChangedFile {
    /// True if git treats the file as binary (no line counts).
    pub fn is_binary(&self) -> bool {
        self.added.is_none()
    }

    /// The file name's extension without the dot, or `""` (dotfiles such as `.gitignore`
    /// have none).
    pub fn extension(&self) -> &str {
        let name = self.path.rsplit('/').next().unwrap_or(&self.path);
        match name.rfind('.') {
            Some(i) if i > 0 => &name[i + 1..],
            _ => "",
        }
    }
}

/// Path order for changed files: folder by folder, a folder's files before its subfolders,
/// names compared case-insensitively (ties broken case-sensitively, so the order is total).
///
/// `b.txt` < `a/z.txt`, `A.txt` < `b.txt` < `C.txt`, `src/x.rs` < `src/sub/a.rs`.
pub fn compare_paths(a: &str, b: &str) -> Ordering {
    let mut ia = a.split('/').peekable();
    let mut ib = b.split('/').peekable();
    loop {
        let (Some(ca), Some(cb)) = (ia.next(), ib.next()) else {
            // Only reached for equal paths: a component that ends one path is a file and
            // is decided below before the other path runs on.
            return a.cmp(b);
        };
        let a_file = ia.peek().is_none();
        let b_file = ib.peek().is_none();
        if a_file != b_file {
            return if a_file {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let order = cmp_case_insensitive(ca, cb).then_with(|| ca.cmp(cb));
        if order != Ordering::Equal || a_file {
            return order;
        }
    }
}

/// A column of the changed-files table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FileColumn {
    #[default]
    Path,
    Extension,
    Status,
    Added,
    Removed,
}

impl FileColumn {
    pub const ALL: [FileColumn; 5] = [
        FileColumn::Path,
        FileColumn::Extension,
        FileColumn::Status,
        FileColumn::Added,
        FileColumn::Removed,
    ];

    /// The column's heading.
    pub fn title(self) -> &'static str {
        match self {
            FileColumn::Path => "Path",
            FileColumn::Extension => "Extension",
            FileColumn::Status => "Status",
            FileColumn::Added => "Lines added",
            FileColumn::Removed => "Lines removed",
        }
    }
}

/// How the changed-files table is sorted. The default is the first view: path order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileOrder {
    pub column: FileColumn,
    pub descending: bool,
}

impl FileOrder {
    /// A click on `column`'s heading: sorts by it, or reverses it if it is already the sort.
    pub fn click(&mut self, column: FileColumn) {
        if self.column == column {
            self.descending = !self.descending;
        } else {
            *self = FileOrder {
                column,
                descending: false,
            };
        }
    }

    /// Orders two files: by the column, ties by [`compare_paths`] (always ascending). Binary
    /// files, which have no line counts, sort below 0 lines.
    pub fn compare(self, a: &ChangedFile, b: &ChangedFile) -> Ordering {
        let by_column = match self.column {
            FileColumn::Path => Ordering::Equal,
            FileColumn::Extension => cmp_case_insensitive(a.extension(), b.extension()),
            FileColumn::Status => a.status.name().cmp(b.status.name()),
            FileColumn::Added => a.added.cmp(&b.added),
            FileColumn::Removed => a.removed.cmp(&b.removed),
        };
        let paths = compare_paths(&a.path, &b.path);
        if self.column == FileColumn::Path {
            if self.descending {
                paths.reverse()
            } else {
                paths
            }
        } else if self.descending {
            by_column.reverse().then(paths)
        } else {
            by_column.then(paths)
        }
    }
}

/// The indices of the files whose path (or old path) contains `filter`, ignoring case, in
/// `order`.
pub fn filter_and_sort(files: &[ChangedFile], filter: &str, order: FileOrder) -> Vec<usize> {
    let filter = filter.to_lowercase();
    let matches = |f: &ChangedFile| {
        filter.is_empty()
            || f.path.to_lowercase().contains(&filter)
            || f.old_path
                .as_ref()
                .is_some_and(|p| p.to_lowercase().contains(&filter))
    };
    let mut shown: Vec<usize> = (0..files.len()).filter(|&i| matches(&files[i])).collect();
    shown.sort_by(|&a, &b| order.compare(&files[a], &files[b]));
    shown
}

fn cmp_case_insensitive(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

/// Parses `git diff-tree -z --raw --numstat` output (all raw records, then all numstat
/// records) into changed files sorted by [`compare_paths`].
pub(crate) fn parse_diff_tree(out: &str) -> Result<Vec<ChangedFile>, String> {
    let mut tokens = out.split('\0').peekable();
    let mut files = Vec::new();
    // Raw records: `:<modes> <oids> <status>` then one path, or two for renames and copies.
    while let Some(header) = tokens.next_if(|t| t.starts_with(':')) {
        let status_field = header
            .rsplit(' ')
            .next()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("raw record without status: {header:?}"))?;
        let status = FileStatus::from_letter(status_field.as_bytes()[0]);
        let mut path = || {
            tokens
                .next()
                .map(str::to_owned)
                .ok_or_else(|| format!("raw record without path: {header:?}"))
        };
        let (path, old_path) = match status {
            FileStatus::Renamed | FileStatus::Copied => {
                let old = path()?;
                (path()?, Some(old))
            }
            _ => (path()?, None),
        };
        files.push(ChangedFile {
            path,
            old_path,
            status,
            added: None,
            removed: None,
        });
    }

    // Numstat records: `<added>\t<removed>\t<path>`, or with an empty path followed by the old
    // and new paths as two more tokens. Binary files show `-` for both counts.
    let mut counts: HashMap<&str, (Option<u32>, Option<u32>)> = HashMap::new();
    while let Some(record) = tokens.next() {
        if record.is_empty() && tokens.peek().is_none() {
            break; // the final terminator
        }
        let mut fields = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return Err(format!("bad numstat record: {record:?}"));
        };
        let path = if path.is_empty() {
            tokens.next(); // old path
            tokens
                .next()
                .ok_or_else(|| format!("numstat rename without paths: {record:?}"))?
        } else {
            path
        };
        counts.insert(path, (added.parse().ok(), removed.parse().ok()));
    }
    for f in &mut files {
        if let Some(&(added, removed)) = counts.get(f.path.as_str()) {
            f.added = added;
            f.removed = removed;
        }
    }
    files.sort_by(|a, b| compare_paths(&a.path, &b.path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(paths: &[&str]) -> Vec<String> {
        let mut v: Vec<String> = paths.iter().map(|&p| p.to_owned()).collect();
        v.sort_by(|a, b| compare_paths(a, b));
        v
    }

    #[test]
    fn files_come_before_subfolders_case_insensitively() {
        assert_eq!(
            sorted(&[
                "src/sub/a.rs",
                "b.txt",
                "Src2/x",
                "a/z.txt",
                "C.txt",
                "src/X.rs",
                "A.txt",
                "src/b.rs",
                "Z/y",
            ]),
            [
                "A.txt",
                "b.txt",
                "C.txt",
                "a/z.txt",
                "src/b.rs",
                "src/X.rs",
                "src/sub/a.rs",
                "Src2/x",
                "Z/y",
            ]
        );
    }

    #[test]
    fn case_only_differences_are_ordered_and_kept_together() {
        assert_eq!(
            sorted(&["a/2", "B/1", "b/2", "A/1", "a", "A"]),
            ["A", "a", "A/1", "a/2", "B/1", "b/2"]
        );
        assert_eq!(compare_paths("x/y", "x/y"), Ordering::Equal);
    }

    #[test]
    fn a_file_sorts_before_a_folder_of_the_same_name() {
        assert_eq!(compare_paths("a", "a/b"), Ordering::Less);
        assert_eq!(compare_paths("a/b", "a"), Ordering::Greater);
    }

    #[test]
    fn extensions() {
        let f = |path: &str| ChangedFile {
            path: path.into(),
            old_path: None,
            status: FileStatus::Added,
            added: Some(0),
            removed: Some(0),
        };
        assert_eq!(f("src/main.rs").extension(), "rs");
        assert_eq!(f("a.tar.gz").extension(), "gz");
        assert_eq!(f("dir.d/.gitignore").extension(), "");
        assert_eq!(f("Makefile").extension(), "");
    }

    #[test]
    fn parses_raw_and_numstat_records() {
        let z = "0".repeat(40);
        let o = "1".repeat(40);
        let out = format!(
            ":100644 100644 {o} {o} M\0bin.dat\0\
             :000000 100644 {z} {o} A\0new\nline\0\
             :100644 100644 {o} {o} R085\0old name\0new ☃\0\
             :100644 000000 {o} {z} D\0:gone\0\
             -\t-\tbin.dat\0\
             1\t0\tnew\nline\0\
             2\t3\t\0old name\0new ☃\0\
             0\t4\t:gone\0"
        );
        let files = parse_diff_tree(&out).unwrap();
        let summary: Vec<_> = files
            .iter()
            .map(|f| {
                (
                    f.path.as_str(),
                    f.old_path.as_deref(),
                    f.status,
                    f.added,
                    f.removed,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (":gone", None, FileStatus::Deleted, Some(0), Some(4)),
                ("bin.dat", None, FileStatus::Modified, None, None),
                ("new\nline", None, FileStatus::Added, Some(1), Some(0)),
                (
                    "new ☃",
                    Some("old name"),
                    FileStatus::Renamed,
                    Some(2),
                    Some(3)
                ),
            ]
        );
        assert!(files[1].is_binary());
    }

    fn file(path: &str, status: FileStatus, lines: Option<(u32, u32)>) -> ChangedFile {
        ChangedFile {
            path: path.into(),
            old_path: None,
            status,
            added: lines.map(|l| l.0),
            removed: lines.map(|l| l.1),
        }
    }

    fn table() -> Vec<ChangedFile> {
        let mut renamed = file("src/New.rs", FileStatus::Renamed, Some((1, 1)));
        renamed.old_path = Some("lib/old.rs".into());
        vec![
            file("README.md", FileStatus::Modified, Some((3, 1))),
            file("img/logo.PNG", FileStatus::Added, None),
            file("src/main.rs", FileStatus::Modified, Some((10, 0))),
            renamed,
            file("src/sub/a.md", FileStatus::Deleted, Some((0, 7))),
        ]
    }

    fn paths(files: &[ChangedFile], shown: &[usize]) -> Vec<String> {
        shown.iter().map(|&i| files[i].path.clone()).collect()
    }

    #[test]
    fn sorting_by_a_column_breaks_ties_by_path_and_clicking_again_reverses() {
        let files = table();
        let mut order = FileOrder::default();
        let sorted = |order| paths(&files, &filter_and_sort(&files, "", order));
        assert_eq!(
            sorted(order),
            [
                "README.md",
                "img/logo.PNG",
                "src/main.rs",
                "src/New.rs",
                "src/sub/a.md"
            ]
        );
        order.click(FileColumn::Path);
        assert_eq!(sorted(order)[0], "src/sub/a.md");

        order.click(FileColumn::Extension);
        assert!(!order.descending);
        // "md" twice (tie by path), then "png" ignoring case, then "rs" twice.
        assert_eq!(
            sorted(order),
            [
                "README.md",
                "src/sub/a.md",
                "img/logo.PNG",
                "src/main.rs",
                "src/New.rs"
            ]
        );
        order.click(FileColumn::Extension);
        // Reversed by extension; ties still in path order.
        assert_eq!(
            sorted(order),
            [
                "src/main.rs",
                "src/New.rs",
                "img/logo.PNG",
                "README.md",
                "src/sub/a.md"
            ]
        );

        order.click(FileColumn::Added);
        // The binary file has no count and sorts lowest.
        assert_eq!(
            sorted(order),
            [
                "img/logo.PNG",
                "src/sub/a.md",
                "src/New.rs",
                "README.md",
                "src/main.rs"
            ]
        );
        order.click(FileColumn::Status);
        assert_eq!(
            sorted(order),
            [
                "img/logo.PNG",
                "src/sub/a.md",
                "README.md",
                "src/main.rs",
                "src/New.rs"
            ]
        );
    }

    #[test]
    fn the_filter_matches_path_or_old_path_ignoring_case() {
        let files = table();
        let order = FileOrder::default();
        let shown = |filter| paths(&files, &filter_and_sort(&files, filter, order));
        assert_eq!(shown("SRC/"), ["src/main.rs", "src/New.rs", "src/sub/a.md"]);
        assert_eq!(shown("old"), ["src/New.rs"]);
        assert_eq!(shown("png"), ["img/logo.PNG"]);
        assert!(shown("nothing").is_empty());
        assert_eq!(shown("").len(), files.len());
    }

    #[test]
    fn parses_empty_output() {
        assert!(parse_diff_tree("").unwrap().is_empty());
    }
}
