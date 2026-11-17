//! Folder hierarchy and folder-scoped queries.
//!
//! Ported from the reference implementation (`src/shared/folders.ts`). Two
//! things here are worth keeping exactly as they are:
//!
//! * **Folder filtering is a string range, not `LIKE`.** `path >= base\ AND
//!   path < base]` (where `]` is the character after `\`) uses the primary key
//!   index and cannot be fooled by a sibling folder named `Foley2` when the
//!   filter is `Foley`. `LIKE 'Foley\%'` would need escaping and a scan.
//! * **The tree is built from paths, not from `readdir`.** The index already
//!   knows every file; asking the filesystem again would show empty folders and
//!   folders the index has not visited.

use std::collections::HashMap;
use std::path::Path;

/// A folder in the library, with the number of sounds at or below it.
#[derive(Debug, Clone, PartialEq)]
pub struct FolderNode {
    pub name: String,
    /// Absolute folder path in native separators, as stored in the index.
    pub path: String,
    /// Sounds in this folder and everything under it.
    pub count: u64,
    pub children: Vec<FolderNode>,
}

/// Which separator a path uses. Windows libraries are all backslash; a mixed
/// path is treated as forward-slash, matching the reference.
pub fn separator_of(path: &str) -> char {
    if path.contains('\\') && !path.contains('/') {
        '\\'
    } else {
        '/'
    }
}

/// Force a root path onto native separators.
///
/// The indexer compares paths as strings (unique key, folder ranges, prune), so
/// every path it stores must use one separator. `walkdir` returns paths that
/// inherit the separator of the root it was given, which means a root passed as
/// `D:/lib` produces `D:/lib\sub\file.wav` — a hybrid that matches nothing,
/// including the rows a previous run wrote. Normalising the root once, at the
/// entry point, is what keeps those comparisons honest.
///
/// On Windows the native separator is `\`; elsewhere it is `/` and this is a
/// no-op.
pub fn normalize_root(root: &str) -> String {
    if cfg!(windows) {
        root.replace('/', "\\")
    } else {
        root.to_string()
    }
}

/// Convert a walked path to the stored form. Native separators, no trailing
/// separator.
pub fn store_path(path: &Path) -> String {
    let text = path.to_string_lossy().to_string();
    if cfg!(windows) {
        text.replace('/', "\\")
    } else {
        text
    }
}

fn trim_sep(path: &str, sep: char) -> &str {
    if path.len() > 1 && path.ends_with(sep) {
        &path[..path.len() - 1]
    } else {
        path
    }
}

/// Half-open range `[lo, hi)` that contains every path inside `folder`.
///
/// The upper bound is the character after the separator in code-point order, so
/// `Foley` matches `Foley\...` and `Foley2` sorts above the range.
///
/// The folder is normalised to native separators first, because the stored paths
/// are: a folder picked up as `D:/lib/Foley` from a path built elsewhere must
/// still match rows stored as `D:\lib\Foley\...`.
pub fn folder_range(folder: &str) -> (String, String) {
    let folder = normalize_root(folder);
    let folder = folder.as_str();
    let sep = separator_of(folder);
    let base = trim_sep(folder, sep);
    let hi = char::from_u32(sep as u32 + 1).unwrap_or('\u{7f}');
    (format!("{base}{sep}"), format!("{base}{hi}"))
}

/// Folder of `file_path` relative to `root`, with `""` for files directly in the
/// root. This is what the `folder` column stores.
pub fn relative_folder(root: &str, file_path: &str) -> String {
    let sep = separator_of(root);
    let base = trim_sep(root, sep);
    let dir = match file_path.rfind(['\\', '/']) {
        Some(idx) => &file_path[..idx],
        None => return String::new(),
    };
    if dir.len() >= base.len() && dir[..base.len()].eq_ignore_ascii_case(base) {
        let rest = &dir[base.len()..];
        rest.trim_start_matches(['\\', '/']).to_string()
    } else {
        String::new()
    }
}

/// Build the tree under `root` from indexed file paths. Files outside the root
/// are ignored.
pub fn build_folder_tree<'a, I>(file_paths: I, root: &str) -> FolderNode
where
    I: IntoIterator<Item = &'a str>,
{
    let sep = separator_of(root);
    let base = trim_sep(root, sep);
    let mut root_node = FolderNode {
        name: base
            .rsplit(['\\', '/'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(base)
            .to_string(),
        path: base.to_string(),
        count: 0,
        children: Vec::new(),
    };

    // Index into the tree by absolute path so each node is created once. The
    // children Vec is built separately from the lookup map to keep the borrow
    // checker happy without unsafe.
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut edges: HashMap<String, Vec<(String, String)>> = HashMap::new();
    let mut counts: HashMap<String, u64> = HashMap::new();

    let prefix = format!("{base}{sep}");
    for file in file_paths {
        let Some(rest) = file.strip_prefix(prefix.as_str()) else {
            continue;
        };
        *counts.entry(base.to_string()).or_insert(0) += 1;
        let parts: Vec<&str> = rest.split(sep).collect();
        let mut parent = base.to_string();
        // The last component is the file name, not a folder.
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            if part.is_empty() {
                continue;
            }
            let child_path = format!("{parent}{sep}{part}");
            *counts.entry(child_path.clone()).or_insert(0) += 1;
            let entry = edges.entry(parent.clone()).or_default();
            if !entry.iter().any(|(p, _)| p == &child_path) {
                entry.push((child_path.clone(), (*part).to_string()));
                index.insert(child_path.clone(), 0);
            }
            parent = child_path;
        }
    }

    fn assemble(
        path: &str,
        name: &str,
        edges: &HashMap<String, Vec<(String, String)>>,
        counts: &HashMap<String, u64>,
    ) -> FolderNode {
        let mut children: Vec<FolderNode> = edges
            .get(path)
            .map(|kids| {
                kids.iter()
                    .map(|(child_path, child_name)| assemble(child_path, child_name, edges, counts))
                    .collect()
            })
            .unwrap_or_default();
        children.sort_by(|a, b| natural_cmp(&a.name, &b.name));
        FolderNode {
            name: name.to_string(),
            path: path.to_string(),
            count: counts.get(path).copied().unwrap_or(0),
            children,
        }
    }

    root_node.count = counts.get(base).copied().unwrap_or(0);
    root_node.children = edges
        .get(base)
        .map(|kids| {
            kids.iter()
                .map(|(child_path, child_name)| assemble(child_path, child_name, &edges, &counts))
                .collect()
        })
        .unwrap_or_default();
    root_node
        .children
        .sort_by(|a, b| natural_cmp(&a.name, &b.name));
    root_node
}

/// Case-insensitive, digit-aware ordering: `Take 2` before `Take 10`, as the UI
/// lists folders. `localeCompare(..., {numeric:true})` in the reference.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x.is_ascii_digit() && y.is_ascii_digit() {
                    let mut xn = String::new();
                    while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                        xn.push(c);
                        ai.next();
                    }
                    let mut yn = String::new();
                    while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                        yn.push(c);
                        bi.next();
                    }
                    let xv: u128 = xn.parse().unwrap_or(0);
                    let yv: u128 = yn.parse().unwrap_or(0);
                    match xv.cmp(&yv) {
                        Ordering::Equal => continue,
                        other => return other,
                    }
                }
                let xl = x.to_lowercase().next().unwrap_or(x);
                let yl = y.to_lowercase().next().unwrap_or(y);
                match xl.cmp(&yl) {
                    Ordering::Equal => {
                        ai.next();
                        bi.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_range_excludes_a_sibling_with_the_same_prefix() {
        let (lo, hi) = folder_range(r"D:\lib\Foley");
        assert_eq!(lo, r"D:\lib\Foley\");
        assert_eq!(hi, r"D:\lib\Foley]");
        assert!(r"D:\lib\Foley\door.wav" >= lo.as_str());
        assert!(r"D:\lib\Foley\door.wav" < hi.as_str());

        // `Foley2` is excluded by the *lower* bound: at the position after
        // "Foley" the sibling has `2` (0x32) where the range starts with the
        // separator (0x5C), so it sorts below the range rather than above it.
        // Verified against the reference `folderRange` before writing this.
        let sibling = r"D:\lib\Foley2\door.wav";
        assert!(!(sibling >= lo.as_str() && sibling < hi.as_str()));

        // The range is exact for a backslash separator: nothing sorts between
        // `\` (0x5C) and `]` (0x5D), so a folder named `Foleya` falls outside on
        // the upper bound. Checked rather than assumed — an earlier version of
        // this test claimed the opposite.
        let edge = r"D:\lib\Foleya\door.wav";
        assert!(edge >= hi.as_str());
    }

    #[test]
    fn folder_range_handles_a_trailing_separator() {
        let (lo, _) = folder_range(r"D:\lib\Foley\");
        assert_eq!(lo, r"D:\lib\Foley\");
    }

    #[test]
    fn relative_folder_is_empty_at_the_root() {
        assert_eq!(relative_folder(r"D:\lib", r"D:\lib\kick.wav"), "");
        assert_eq!(
            relative_folder(r"D:\lib", r"D:\lib\Foley\Cloth\x.wav"),
            r"Foley\Cloth"
        );
    }

    #[test]
    fn store_path_normalises_the_separator_the_root_was_given_with() {
        // `walkdir` returns paths that inherit the root's separator, so a root
        // passed as `D:/lib` yields `D:/lib\sub\a.wav`. Storing that hybrid
        // broke the unique key and made a full pass prune every row a previous
        // run had written.
        let mixed = Path::new(r"D:/lib\Foley\a.wav");
        let stored = store_path(mixed);
        if cfg!(windows) {
            assert_eq!(stored, r"D:\lib\Foley\a.wav");
            assert_eq!(normalize_root("D:/lib"), r"D:\lib");
        }
        // Both sides normalise to the same form, so the folder is not empty.
        let root = normalize_root("D:/lib");
        assert_eq!(relative_folder(&root, &stored), r"Foley");
    }

    #[test]
    fn folder_tree_is_not_empty_when_the_root_was_given_with_forward_slashes() {
        // The indexer stores native separators, so the tree builder has to
        // normalise the root before prefix-matching. Without that the tree comes
        // back with zero children on every real library opened as `D:/lib`.
        let paths = [r"D:\lib\Foley\door.wav", r"D:\lib\Foley\Cloth\a.wav"];
        let tree = build_folder_tree(paths.iter().copied(), &normalize_root("D:/lib"));
        assert_eq!(tree.count, 2);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].name, "Foley");
    }

    #[test]
    fn folder_range_accepts_either_separator_style() {
        let (lo, hi) = folder_range("D:/lib/Foley");
        assert_eq!(lo, folder_range(r"D:\lib\Foley").0);
        assert_eq!(hi, folder_range(r"D:\lib\Foley").1);
        if cfg!(windows) {
            assert_eq!(lo, r"D:\lib\Foley\");
        }
    }

    #[test]
    fn tree_counts_are_recursive_and_children_are_natural_sorted() {
        let paths = [
            r"D:\lib\Foley\Cloth\a.wav",
            r"D:\lib\Foley\door.wav",
            r"D:\lib\Take 10\x.wav",
            r"D:\lib\Take 2\x.wav",
            r"D:\lib\root.wav",
        ];
        let tree = build_folder_tree(paths.iter().copied(), r"D:\lib");
        assert_eq!(tree.count, 5);
        assert_eq!(tree.name, "lib");
        let names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Foley", "Take 2", "Take 10"]);
        assert_eq!(tree.children[0].count, 2);
        assert_eq!(tree.children[0].children[0].name, "Cloth");
    }

    #[test]
    fn files_outside_the_root_are_ignored() {
        let tree = build_folder_tree([r"D:\other\a.wav"].iter().copied(), r"D:\lib");
        assert_eq!(tree.count, 0);
        assert!(tree.children.is_empty());
    }
}
