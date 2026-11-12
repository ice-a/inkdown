//! Local zip backups of the current folder, or of the open documents when no
//! folder is open.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target", ".inkdown-backups"];
const MAX_FILES: usize = 5000;

pub fn default_stem() -> String {
    format!(
        "inkdown-backup-{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    )
}

fn unique_archive_path(dir: &Path, stem: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}.zip"));
    let mut index = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{index}.zip"));
        index += 1;
    }
    path
}

pub fn backup_workspace(root: &Path, backup_dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(backup_dir).map_err(|error| error.to_string())?;
    let folder_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace");
    let stem = format!("{}-{}", folder_name, default_stem());
    let path = unique_archive_path(backup_dir, &stem);
    let file = File::create(&path).map_err(|error| format!("创建备份失败：{error}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut count = 0;
    add_dir(&mut zip, root, root, options, &mut count)?;
    zip.finish().map_err(|error| format!("写入备份失败：{error}"))?;
    Ok(path)
}

pub fn backup_documents(
    documents: &[(String, String)],
    backup_dir: &Path,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(backup_dir).map_err(|error| error.to_string())?;
    let path = unique_archive_path(backup_dir, &default_stem());
    let file = File::create(&path).map_err(|error| format!("创建备份失败：{error}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut used = std::collections::HashSet::new();
    for (index, (name, content)) in documents.iter().enumerate() {
        let mut name = sanitize(name);
        if !used.insert(name.clone()) {
            name = format!(
                "{}-{}.md",
                name.trim_end_matches(".md"),
                index + 1
            );
            used.insert(name.clone());
        }
        zip.start_file(name, options)
            .map_err(|error| format!("写入备份失败：{error}"))?;
        zip.write_all(content.as_bytes())
            .map_err(|error| format!("写入备份失败：{error}"))?;
    }
    zip.finish().map_err(|error| format!("写入备份失败：{error}"))?;
    Ok(path)
}

fn add_dir(
    zip: &mut zip::ZipWriter<File>,
    root: &Path,
    dir: &Path,
    options: SimpleFileOptions,
    count: &mut usize,
) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        if *count >= MAX_FILES {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".env" {
            continue;
        }
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            add_dir(zip, root, &path, options, count)?;
        } else if file_type.is_file() {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let entry_name = relative.to_string_lossy().replace('\\', "/");
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            zip.start_file(entry_name, options)
                .map_err(|error| format!("写入备份失败：{error}"))?;
            zip.write_all(&bytes)
                .map_err(|error| format!("写入备份失败：{error}"))?;
            *count += 1;
        }
    }
    Ok(())
}

fn sanitize(name: &str) -> String {
    let name = name.replace(['/', '\\', ':'], "-");
    if name.ends_with(".md") {
        name
    } else {
        format!("{name}.md")
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn packs_workspace_files() {
        let root = std::env::temp_dir().join(format!(
            "inkdown-backup-test-{}-{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let out = root.join("out");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("a.md"), "# a\n").unwrap();
        std::fs::write(root.join("sub/b.md"), "# b\n").unwrap();

        let archive = backup_workspace(&root, &out).unwrap();
        let file = File::open(&archive).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut names = Vec::new();
        for index in 0..zip.len() {
            names.push(zip.by_index(index).unwrap().name().to_string());
        }
        assert!(names.iter().any(|name| name == "a.md"));
        assert!(names.iter().any(|name| name == "sub/b.md"));

        let mut entry = zip.by_name("a.md").unwrap();
        let mut text = String::new();
        entry.read_to_string(&mut text).unwrap();
        assert_eq!(text, "# a\n");

        std::fs::remove_dir_all(&root).ok();
    }
}
