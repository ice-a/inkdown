//! Git integration through the system `git` executable. Credentials stay in
//! the user's normal credential manager; Inkdown never sees them.

use std::path::Path;
use std::process::Command;

use crate::config::GitConfig;

fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(root);
    command.env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
}

pub fn version() -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("--version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .output()
        .map_err(|error| format!("找不到 git，请先安装 Git for Windows（{error}）"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn is_repo(root: &Path) -> bool {
    root.join(".git").exists()
}

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = command(root)
        .args(args)
        .output()
        .map_err(|error| format!("执行 git 失败：{error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else if stderr.is_empty() {
        Err(stdout)
    } else {
        Err(stderr)
    }
}

fn configure_user(root: &Path, config: &GitConfig) -> Result<(), String> {
    if !config.user_name.trim().is_empty() {
        run(root, &["config", "user.name", config.user_name.trim()])?;
    }
    if !config.user_email.trim().is_empty() {
        run(root, &["config", "user.email", config.user_email.trim()])?;
    }
    Ok(())
}

fn configure_remote(root: &Path, config: &GitConfig) -> Result<(), String> {
    let remote = config.remote.trim();
    if remote.is_empty() {
        return Ok(());
    }
    match run(root, &["remote", "get-url", "origin"]) {
        Ok(current) if current.trim() == remote => Ok(()),
        Ok(_) => {
            run(root, &["remote", "set-url", "origin", remote])?;
            Ok(())
        }
        Err(_) => {
            run(root, &["remote", "add", "origin", remote])?;
            Ok(())
        }
    }
}

pub fn init(root: &Path, config: &GitConfig) -> Result<String, String> {
    let mut log = String::new();
    if !is_repo(root) {
        log.push_str(&run(root, &["init"])?);
        log.push('\n');
    }
    configure_user(root, config)?;
    configure_remote(root, config)?;
    let branch = if config.branch.trim().is_empty() {
        "main"
    } else {
        config.branch.trim()
    };
    log.push_str(&run(root, &["checkout", "-B", branch])?);
    log.push('\n');
    Ok(log.trim().to_string())
}

pub fn status(root: &Path) -> Result<String, String> {
    if !is_repo(root) {
        return Ok("未初始化 Git 仓库".to_string());
    }
    let branch = run(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "（还没有提交）".to_string());
    let porcelain = run(root, &["status", "--porcelain"])?;
    let changes = porcelain
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    Ok(if changes == 0 {
        format!("{branch} · 工作区干净")
    } else {
        format!("{branch} · {changes} 处改动待提交")
    })
}

/// Stage everything, commit if needed, pull with rebase and push.
pub fn sync(root: &Path, config: &GitConfig) -> Result<String, String> {
    if !is_repo(root) {
        return Err("当前文件夹还不是 Git 仓库，请先在设置里点击“初始化仓库”".to_string());
    }
    configure_user(root, config)?;
    configure_remote(root, config)?;

    let branch = if config.branch.trim().is_empty() {
        "main"
    } else {
        config.branch.trim()
    };
    let mut log = Vec::new();

    run(root, &["add", "-A"])?;
    let changes = run(root, &["status", "--porcelain"])?;
    if !changes.trim().is_empty() {
        let message = format!(
            "Inkdown sync: {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        );
        run(root, &["commit", "-m", &message])?;
        log.push(format!("已提交 {message}"));
    } else {
        log.push("没有新的改动需要提交".to_string());
    }

    // Make sure the branch exists even in a repository with no commits yet.
    if run(root, &["rev-parse", "--verify", branch]).is_err() {
        run(root, &["checkout", "-B", branch])?;
    }

    let has_remote =
        !config.remote.trim().is_empty() || run(root, &["remote", "get-url", "origin"]).is_ok();
    if has_remote {
        match run(root, &["pull", "--rebase", "origin", branch]) {
            Ok(_) => log.push("已拉取远端更新".to_string()),
            Err(error) => log.push(format!("拉取跳过：{error}")),
        }
        run(root, &["push", "-u", "origin", branch])?;
        log.push(format!("已推送到 origin/{branch}"));
    } else {
        log.push("未配置远端，只完成了本地提交".to_string());
    }

    Ok(log.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    fn temp_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "inkdown-git-test-{name}-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn init_commit_and_push_to_local_remote() {
        let root = temp_dir("repo");
        let remote = temp_dir("remote");
        let init_status = Command::new("git")
            .args(["init", "--bare", remote.to_str().unwrap()])
            .status()
            .expect("git init --bare");
        assert!(init_status.success());

        let config = GitConfig {
            remote: remote.to_str().unwrap().to_string(),
            branch: "main".to_string(),
            user_name: "Inkdown Test".to_string(),
            user_email: "test@example.com".to_string(),
            auto_sync: false,
        };

        init(&root, &config).unwrap();
        fs::write(root.join("note.md"), "# hello\n").unwrap();
        let log = sync(&root, &config).unwrap();
        assert!(log.contains("已推送") || log.contains("已提交"), "{log}");

        let output = Command::new("git")
            .args([
                "--git-dir",
                remote.to_str().unwrap(),
                "rev-list",
                "--count",
                "main",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1");

        let status = status(&root).unwrap();
        assert!(!status.is_empty());

        fs::remove_dir_all(root).ok();
        fs::remove_dir_all(remote).ok();
    }
}
