//! Periodic snapshots of open documents, stored in one local git repository
//! under the data directory. Separate from the workspace's own repo so the
//! version history never mixes with the author's commits or remotes.

use std::path::{Path, PathBuf};

use crate::config;
use crate::git;

/// One saved version of a document.
#[derive(Clone, Debug)]
pub struct Version {
    pub rev: String,
    /// Commit subject: the local time the snapshot was taken.
    pub subject: String,
}

/// The single repository holding every document's snapshot history.
pub fn repo_dir() -> PathBuf {
    config::data_dir()
        .unwrap_or_else(config::config_dir)
        .join("history")
}

/// Stable per-document file name: a sanitized path stem plus an FNV-1a hash
/// of the full path, so long or colliding paths stay unique and readable.
pub fn key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut stem: String = raw
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if stem.len() > 80 {
        stem.truncate(80);
    }
    format!("{stem}-{:016x}", fnv1a(raw.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Cheap content fingerprint: the timer skips documents whose text has not
/// changed since their last snapshot.
pub fn content_hash(text: &str) -> u64 {
    fnv1a(text.as_bytes())
}

fn doc_rel(path: &Path) -> String {
    format!("docs/{}.md", key(path))
}

/// Create the repository on first use and pin the settings that keep
/// snapshots byte-exact (autocrlf would otherwise invent versions) and
/// committable without touching the user's global git identity.
pub fn ensure_repo(root: &Path) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    if !root.join(".git").exists() {
        git::run(root, &["init"])?;
        git::run(root, &["config", "core.autocrlf", "false"])?;
        git::run(root, &["config", "user.name", "Inkdown"])?;
        git::run(root, &["config", "user.email", "inkdown@localhost"])?;
    }
    Ok(())
}

/// Store `content` as the next version of `doc`.
///
/// Returns `Ok(true)` when a commit was created and `Ok(false)` when the
/// content is identical to the last snapshot — git itself decides, so an
/// idle timer never fills the log with no-op versions.
pub fn snapshot(root: &Path, doc: &Path, content: &str) -> Result<bool, String> {
    ensure_repo(root)?;
    let rel = doc_rel(doc);
    let file = root.join(&rel);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&file, content).map_err(|error| error.to_string())?;

    git::run(root, &["add", "--", &rel])?;
    let status = git::run(root, &["status", "--porcelain", "--", &rel])?;
    if status.trim().is_empty() {
        return Ok(false);
    }
    let message = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    git::run(root, &["commit", "-m", &message, "--", &rel])?;
    Ok(true)
}

/// Versions of `doc`, newest first. Empty when the repository or the
/// document has no commits yet.
pub fn versions(root: &Path, doc: &Path) -> Result<Vec<Version>, String> {
    if !root.join(".git").exists() {
        return Ok(Vec::new());
    }
    // `git log` fails on an empty repository; that simply means no versions.
    if git::run(root, &["rev-parse", "--verify", "HEAD"]).is_err() {
        return Ok(Vec::new());
    }
    let rel = doc_rel(doc);
    let output = git::run(root, &["log", "--format=%H%x09%s", "--", &rel])?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let (rev, subject) = line.split_once('\t')?;
            Some(Version {
                rev: rev.trim().to_string(),
                subject: subject.trim().to_string(),
            })
        })
        .collect())
}

/// Read a stored version's exact content (trailing newlines included).
pub fn read(root: &Path, doc: &Path, rev: &str) -> Result<String, String> {
    let rel = doc_rel(doc);
    git::run_raw(root, &["show", &format!("{rev}:{rel}")])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "inkdown-history-test-{name}-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn keys_are_stable_and_distinct() {
        let a = key(Path::new("C:/notes/第一篇.md"));
        let b = key(Path::new("C:/notes/第二篇.md"));
        assert_eq!(a, key(Path::new("C:/notes/第一篇.md")));
        assert_ne!(a, b);
        // ASCII-only file name part, hash suffix for uniqueness.
        assert!(a.starts_with("C__notes_____"));
        assert!(a.len() <= 80 + 1 + 16);
    }

    #[test]
    fn snapshots_deduplicate_and_roundtrip() {
        let root = temp_dir("repo");
        let doc = PathBuf::from("C:/notes/sample.md");

        assert!(snapshot(&root, &doc, "# 第一版\n").unwrap());
        // Same content: git sees no change, no empty version is created.
        assert!(!snapshot(&root, &doc, "# 第一版\n").unwrap());
        assert!(snapshot(&root, &doc, "# 第二版\n").unwrap());

        let versions = versions(&root, &doc).unwrap();
        assert_eq!(versions.len(), 2);

        // Newest first, subject is the snapshot time.
        let newest = read(&root, &doc, &versions[0].rev).unwrap();
        assert_eq!(newest, "# 第二版\n");
        let older = read(&root, &doc, &versions[1].rev).unwrap();
        assert_eq!(older, "# 第一版\n");
        assert!(versions[0].subject.contains(':'), "{}", versions[0].subject);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn versions_of_unknown_document_are_empty() {
        let root = temp_dir("empty");
        assert!(
            versions(&root, Path::new("C:/never/saved.md"))
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(&root).ok();
    }
}
