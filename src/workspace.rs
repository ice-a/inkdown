//! Folder workspace and the file tree shown in the sidebar.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 12;

#[derive(Clone, Debug, Default)]
pub struct FileTree {
    pub root: PathBuf,
    pub nodes: Vec<FileNode>,
}

#[derive(Clone, Debug)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
}

#[derive(Clone, Debug)]
pub struct FlatNode {
    pub depth: usize,
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub has_children: bool,
    pub expanded: bool,
}

impl FileTree {
    pub fn open(root: PathBuf) -> Self {
        let mut tree = Self {
            root,
            nodes: Vec::new(),
        };
        tree.refresh();
        tree
    }

    pub fn refresh(&mut self) {
        self.nodes = read_dir_sorted(&self.root, 0);
    }

    /// Flatten the tree to the rows that are currently visible.
    pub fn visible(&self, expanded: &HashSet<PathBuf>) -> Vec<FlatNode> {
        let mut out = Vec::new();
        flatten(&self.nodes, 0, expanded, &mut out);
        out
    }
}

fn read_dir_sorted(dir: &Path, depth: usize) -> Vec<FileNode> {
    if depth > MAX_DEPTH {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut nodes = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if skip_name(&name) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            nodes.push(FileNode {
                path: path.clone(),
                name,
                is_dir: true,
                children: read_dir_sorted(&path, depth + 1),
            });
        } else if file_type.is_file() && is_markdown(&path) {
            nodes.push(FileNode {
                path,
                name,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }

    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    nodes
}

fn flatten(nodes: &[FileNode], depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<FlatNode>) {
    for node in nodes {
        let is_expanded = node.is_dir && expanded.contains(&node.path);
        out.push(FlatNode {
            depth,
            path: node.path.clone(),
            name: node.name.clone(),
            is_dir: node.is_dir,
            has_children: !node.children.is_empty(),
            expanded: is_expanded,
        });
        if is_expanded {
            flatten(&node.children, depth + 1, expanded, out);
        }
    }
}

fn skip_name(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".inkdown-backups" | "node_modules" | "target" | ".DS_Store"
    ) || name.starts_with('.')
}

pub fn is_markdown(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "markdown" | "mdx" | "txt")
    )
}

/// The folder new files should be created in for a selected row.
pub fn target_dir(root: &Path, selected: Option<&Path>) -> PathBuf {
    match selected {
        Some(path) if path.is_dir() => path.to_path_buf(),
        Some(path) => path
            .parent()
            .map(|parent| parent.to_path_buf())
            .unwrap_or_else(|| root.to_path_buf()),
        None => root.to_path_buf(),
    }
}

/// Expand every ancestor between the root and `path`.
pub fn expand_ancestors(root: &Path, path: &Path, expanded: &mut HashSet<PathBuf>) {
    let mut current = path.parent();
    while let Some(parent) = current {
        expanded.insert(parent.to_path_buf());
        if parent == root {
            break;
        }
        current = parent.parent();
    }
    if path.is_dir() {
        expanded.insert(path.to_path_buf());
    }
}

/// Validate a file or folder name typed by the user.
pub fn validate_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名称不能为空".to_string());
    }
    if name == "." || name == ".." {
        return Err("这个名称不可用".to_string());
    }
    if name
        .chars()
        .any(|ch| matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
    {
        return Err("名称不能包含 / \\ : * ? \" < > |".to_string());
    }
    Ok(name.to_string())
}

/// Pick a name like `未命名.md`, `未命名 2.md`, ... that does not exist yet.
pub fn unique_markdown_path(dir: &Path, stem: &str) -> PathBuf {
    let mut index = 1;
    loop {
        let name = if index == 1 {
            format!("{stem}.md")
        } else {
            format!("{stem} {index}.md")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
        index += 1;
    }
}

/// Pick a folder name that does not exist yet.
pub fn unique_dir_path(dir: &Path, stem: &str) -> PathBuf {
    let mut index = 1;
    loop {
        let name = if index == 1 {
            stem.to_string()
        } else {
            format!("{stem} {index}")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
        index += 1;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_markdown_and_expands_directories() {
        let root = std::env::temp_dir().join(format!(
            "inkdown-tree-test-{}-{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.md"), "# a").unwrap();
        std::fs::write(root.join("sub/b.md"), "# b").unwrap();
        std::fs::write(root.join("ignore.png"), "x").unwrap();

        let tree = FileTree::open(root.clone());
        let mut expanded = HashSet::new();
        expanded.insert(root.clone());
        let visible = tree.visible(&expanded);
        assert!(visible.iter().any(|node| node.name == "a.md"));
        assert!(visible.iter().any(|node| node.name == "sub" && node.is_dir));
        assert!(!visible.iter().any(|node| node.name == "ignore.png"));

        expanded.insert(root.join("sub"));
        let visible = tree.visible(&expanded);
        assert!(visible.iter().any(|node| node.name == "b.md"));

        std::fs::remove_dir_all(&root).ok();
    }
}
