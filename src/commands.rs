//! Slash-command palette support: built-in commands live in the view, while
//! user commands are small JavaScript/Python files in the config directory.
//!
//! Contract: the script receives one JSON object on stdin and writes the text
//! to insert to stdout. Non-zero exit statuses surface stderr in the UI.
//!
//! Scripts run with the current user's privileges. This is a local automation
//! hook, not a sandbox; the command picker says so.

use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::Serialize;

use crate::config;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Runtime {
    JavaScript,
    Python,
}

impl Runtime {
    pub fn label(self) -> &'static str {
        match self {
            Runtime::JavaScript => "JS",
            Runtime::Python => "PY",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CustomCommand {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub runtime: Runtime,
}

/// The JSON document sent to a custom command on stdin.
#[derive(Clone, Debug, Serialize)]
pub struct ScriptContext {
    pub document: String,
    pub selection: String,
    /// Byte offset of the caret in `document`.
    pub cursor: usize,
    pub now: String,
    pub date: String,
    pub time: String,
    /// The text typed after `/`, without the slash.
    pub query: String,
}

pub fn command_dir() -> PathBuf {
    config::config_dir().join("commands")
}

pub fn ensure_dir() -> Result<PathBuf, String> {
    let dir = command_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建命令目录失败：{error}"))?;
    Ok(dir)
}

/// Create the example scripts and README once; never overwrite user edits.
pub fn ensure_templates() -> Result<(PathBuf, usize), String> {
    let dir = ensure_dir()?;
    let mut created = 0;
    for (name, contents) in [
        ("example-now.js", EXAMPLE_JS),
        ("example-wordcount.py", EXAMPLE_PY),
        ("README.md", README),
    ] {
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(contents.as_bytes())
                    .map_err(|error| format!("写入 {name} 失败：{error}"))?;
                created += 1;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("创建 {name} 失败：{error}")),
        }
    }
    Ok((dir, created))
}

pub fn load_custom() -> Result<Vec<CustomCommand>, String> {
    let dir = command_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut commands = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // A relative INKDOWN_CONFIG_DIR would otherwise make the script path
        // resolve twice after `current_dir` is applied.
        let path = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&path))
                .unwrap_or(path)
        };
        let Some(runtime) = runtime_for(&path) else {
            continue;
        };
        if entry
            .metadata()
            .map(|metadata| metadata.len() > 1_000_000)
            .unwrap_or(false)
        {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap_or_default();
        let source = source.trim_start_matches('\u{feff}');
        let fallback = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("自定义命令");
        let (name, description) = parse_metadata(&source, fallback);
        commands.push(CustomCommand {
            name,
            description,
            path,
            runtime,
        });
    }
    commands.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(commands)
}

pub fn run(command: &CustomCommand, context: &ScriptContext) -> Result<String, String> {
    let input =
        serde_json::to_vec(context).map_err(|error| format!("序列化上下文失败：{error}"))?;
    let working_dir = command
        .path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    let mut attempts: Vec<Command> = Vec::new();
    match command.runtime {
        Runtime::JavaScript => {
            let mut node = Command::new("node");
            node.arg(&command.path);
            attempts.push(node);
        }
        Runtime::Python => {
            #[cfg(windows)]
            {
                let mut py = Command::new("py");
                py.arg("-3").arg(&command.path);
                attempts.push(py);
                let mut python = Command::new("python");
                python.arg(&command.path);
                attempts.push(python);
            }
            #[cfg(not(windows))]
            {
                let mut python3 = Command::new("python3");
                python3.arg(&command.path);
                attempts.push(python3);
                let mut python = Command::new("python");
                python.arg(&command.path);
                attempts.push(python);
            }
        }
    }

    let mut missing = false;
    for mut process in attempts {
        process.current_dir(&working_dir);
        if command.runtime == Runtime::Python {
            // Make stdin/stdout UTF-8 even when the Windows console code page
            // is GBK; the JSON body contains the whole document.
            process.env("PYTHONIOENCODING", "utf-8");
            process.env("PYTHONUTF8", "1");
        }
        match spawn_and_wait(process, &input) {
            Ok(output) => return finish(output),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                missing = true;
                continue;
            }
            Err(error) => return Err(format!("启动脚本失败：{error}")),
        }
    }

    Err(if missing {
        match command.runtime {
            Runtime::JavaScript => {
                "未找到 Node.js。请安装 Node.js 并把 node 加入 PATH，或改用 .py 命令".to_string()
            }
            Runtime::Python => {
                "未找到 Python。请安装 Python 3 并把 python/py 加入 PATH，或改用 .js 命令"
                    .to_string()
            }
        }
    } else {
        "命令没有可用的运行环境".to_string()
    })
}

fn spawn_and_wait(mut process: Command, input: &[u8]) -> std::io::Result<Output> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a script must not flash a cmd window.
        process.creation_flags(0x0800_0000);
    }
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = process.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        // Feed stdin on its own thread: a script that never reads stdin must
        // not block the command (large documents exceed the pipe buffer).
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    child.wait_with_output()
}

fn finish(output: Output) -> Result<String, String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            format!(
                "命令执行失败，退出码 {}",
                output.status.code().unwrap_or(-1)
            )
        } else {
            format!("命令执行失败：{}", truncate(stderr, 1500))
        });
    }
    let text = String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n");
    const MAX_CHARS: usize = 200_000;
    if text.chars().count() > MAX_CHARS {
        let mut clipped: String = text.chars().take(MAX_CHARS).collect();
        clipped.push_str("\n\n…（输出过长，已截断）");
        Ok(clipped)
    } else {
        Ok(text)
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(max_chars).collect();
    clipped.push('…');
    clipped
}

fn runtime_for(path: &Path) -> Option<Runtime> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("js" | "mjs" | "cjs") => Some(Runtime::JavaScript),
        Some("py") => Some(Runtime::Python),
        _ => None,
    }
}

fn parse_metadata(source: &str, fallback: &str) -> (String, String) {
    let mut name = fallback.to_string();
    let mut description = "自定义命令".to_string();
    for line in source.lines().take(40) {
        let line = line.trim();
        let comment = if let Some(rest) = line.strip_prefix("//") {
            rest
        } else if let Some(rest) = line.strip_prefix('#') {
            rest
        } else {
            continue;
        };
        let comment = comment.trim();
        if let Some(value) = meta_value(comment, "@name") {
            if !value.is_empty() {
                name = value.to_string();
            }
        } else if let Some(value) =
            meta_value(comment, "@description").or_else(|| meta_value(comment, "@desc"))
        {
            if !value.is_empty() {
                description = value.to_string();
            }
        }
    }
    (name, description)
}

fn meta_value<'a>(comment: &'a str, key: &str) -> Option<&'a str> {
    let rest = comment.strip_prefix(key)?;
    if !rest.is_empty()
        && !rest.starts_with(':')
        && !rest.chars().next().is_some_and(char::is_whitespace)
    {
        return None;
    }
    Some(rest.trim_start_matches(':').trim())
}

const EXAMPLE_JS: &str = r#"// @name 示例：当前时间
// @description 读取 stdin 的 JSON，把时间以引用形式插入文稿
const chunks = [];
process.stdin.on("data", (chunk) => chunks.push(chunk));
process.stdin.on("end", () => {
  const ctx = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  process.stdout.write(`> ${ctx.now}\n`);
});
"#;

const EXAMPLE_PY: &str = r#"# @name 示例：文档统计
# @description 统计当前文档的字符数并插入一行
import json
import sys

ctx = json.load(sys.stdin)
text = ctx.get("document", "")
print(f"> 当前文档共 {len(text)} 个字符")
"#;

const README: &str = r#"# Inkdown 快捷指令

在编辑器里输入 `/` 打开命令面板，输入名称筛选；↑↓ 选择，Enter 或 Tab 执行，Esc 关闭。

这个目录里的 `.js` / `.mjs` / `.cjs` 和 `.py` 文件会自动出现在命令面板里。
文件开头的注释可以写名称和说明：

```js
// @name 当前时间
// @description 插入当前日期和时间
```

```python
# @name 当前时间
# @description 插入当前日期和时间
```

## 脚本约定

- 从 stdin 读取一个 JSON 对象：

```json
{
  "document": "整篇 Markdown",
  "selection": "当前选中的文字",
  "cursor": 123,
  "now": "2026-11-13 11:20:00",
  "date": "2026-11-13",
  "time": "11:20:00",
  "query": "typed-after-slash"
}
```

- 把要插入到光标位置的 Markdown 写到 stdout。
- 非零退出码会把 stderr 显示在应用状态栏。
- JS 使用 `node`；Python 优先用 `py -3`（Windows）或 `python3`。
- 脚本以当前用户权限运行，可以访问网络和文件。只运行你信任的脚本，
  不要把来路不明的 `.js` / `.py` 放进这个目录。
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_javascript_and_python_metadata() {
        let js = "// @name 当前时间\n// @description 插入时间\nconsole.log(1);";
        let (name, description) = parse_metadata(js, "example");
        assert_eq!(name, "当前时间");
        assert_eq!(description, "插入时间");

        let py = "#!/usr/bin/env python3\n# @name: 文档统计\n# @desc: 统计字符\nprint(1)";
        let (name, description) = parse_metadata(py, "example");
        assert_eq!(name, "文档统计");
        assert_eq!(description, "统计字符");

        let plain = "console.log(1);";
        let (name, description) = parse_metadata(plain, "fallback");
        assert_eq!(name, "fallback");
        assert_eq!(description, "自定义命令");
    }

    #[test]
    fn picks_runtime_from_extension() {
        assert_eq!(runtime_for(Path::new("a.js")), Some(Runtime::JavaScript));
        assert_eq!(runtime_for(Path::new("a.py")), Some(Runtime::Python));
        assert_eq!(runtime_for(Path::new("a.txt")), None);
    }

    #[test]
    fn truncates_long_text_on_char_boundaries() {
        let text = "中文".repeat(10);
        let clipped = truncate(&text, 5);
        assert_eq!(clipped.chars().count(), 6);
        assert!(clipped.ends_with('…'));
    }

    #[test]
    fn runs_javascript_command_when_available() {
        let dir = std::env::temp_dir().join(format!(
            "inkdown-command-js-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.js");
        std::fs::write(
            &path,
            r#"const chunks = [];
process.stdin.on("data", (chunk) => chunks.push(chunk));
process.stdin.on("end", () => {
  const ctx = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  console.log(`js=${ctx.query}`);
});
"#,
        )
        .unwrap();
        let command = CustomCommand {
            name: "test".to_string(),
            description: String::new(),
            path,
            runtime: Runtime::JavaScript,
        };
        let context = ScriptContext {
            document: String::new(),
            selection: String::new(),
            cursor: 0,
            now: String::new(),
            date: String::new(),
            time: String::new(),
            query: "hello".to_string(),
        };
        let result = run(&command, &context);
        std::fs::remove_dir_all(&dir).ok();
        match result {
            Ok(output) => assert!(output.contains("js=hello"), "unexpected output: {output}"),
            Err(error) if error.contains("未找到 Node.js") => {}
            Err(error) => panic!("javascript command failed: {error}"),
        }
    }

    #[test]
    fn runs_python_command_when_available() {
        let dir = std::env::temp_dir().join(format!(
            "inkdown-command-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.py");
        std::fs::write(
            &path,
            r#"import json
import sys
ctx = json.load(sys.stdin)
print(f"doc={len(ctx['document'])} q={ctx['query']}")
"#,
        )
        .unwrap();
        let command = CustomCommand {
            name: "test".to_string(),
            description: String::new(),
            path,
            runtime: Runtime::Python,
        };
        let context = ScriptContext {
            document: "中文abc".to_string(),
            selection: String::new(),
            cursor: 0,
            now: "2026-11-13 11:30:00".to_string(),
            date: "2026-11-13".to_string(),
            time: "11:30:00".to_string(),
            query: "test".to_string(),
        };
        let result = run(&command, &context);
        std::fs::remove_dir_all(&dir).ok();
        match result {
            Ok(output) => {
                assert!(output.contains("doc=5"), "unexpected output: {output}");
                assert!(output.contains("q=test"), "unexpected output: {output}");
            }
            Err(error) if error.contains("未找到 Python") => {}
            Err(error) => panic!("python command failed: {error}"),
        }
    }
}
