//! The document model, plus the small markdown utilities the UI needs:
//! an outline, writing statistics and a self-contained HTML export.

use std::ops::Range;
use std::path::PathBuf;

use gpui_kit::SharedString;
use pulldown_cmark::{Options, Parser, html};

#[derive(Clone)]
pub struct Document {
    pub id: u64,
    pub title: SharedString,
    pub path: Option<PathBuf>,
    pub content: String,
    /// The content as it was last written to disk, for the dirty indicator.
    pub saved_content: String,
}

impl Document {
    pub fn sample(id: u64, title: &str, content: &str) -> Self {
        Self {
            id,
            title: title.into(),
            path: None,
            content: content.to_string(),
            saved_content: content.to_string(),
        }
    }

    pub fn untitled(id: u64, number: usize) -> Self {
        Self {
            id,
            title: format!("未命名 {number}.md").into(),
            path: None,
            content: String::new(),
            saved_content: String::new(),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.content != self.saved_content
    }

    pub fn file_stem(&self) -> String {
        self.title
            .strip_suffix(".md")
            .unwrap_or(self.title.as_ref())
            .to_string()
    }
}

#[derive(Clone, Debug)]
pub struct Heading {
    pub level: usize,
    pub title: String,
    /// Byte offset of the heading's `#` in the source.
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub chars: usize,
    pub lines: usize,
    pub minutes: usize,
}

/// Split a leading YAML frontmatter block (`---` … `---`) from the body.
///
/// Returns the frontmatter block including both delimiter lines when the
/// document starts with a closed `---` block, and the remaining document.
/// An unterminated opening `---` is not frontmatter and passes through.
pub fn split_frontmatter(source: &str) -> (Option<&str>, &str) {
    let mut offset = 0usize;
    let mut seen_opening = false;
    for line in source.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        if !seen_opening {
            if text.trim_end() != "---" {
                return (None, source);
            }
            seen_opening = true;
        } else if text.trim_end() == "---" {
            let end = offset + line.len();
            return (Some(&source[..end]), &source[end..]);
        }
        offset += line.len();
    }
    (None, source)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontmatterField {
    pub key: String,
    pub value: String,
}

/// Simple `key: value` entries from the frontmatter block. Nested YAML and
/// block scalars are left to the raw text and ignored here.
pub fn frontmatter_fields(source: &str) -> Vec<FrontmatterField> {
    let (Some(front), _) = split_frontmatter(source) else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    for line in front.lines().skip(1) {
        if line.trim_end() == "---" {
            break;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            if !key.is_empty() {
                fields.push(FrontmatterField {
                    key: key.to_string(),
                    value: value.trim().to_string(),
                });
            }
        }
    }
    fields
}

/// Rewrite `source` with a frontmatter block built from `fields`.
pub fn set_frontmatter_fields(source: &str, fields: &[(String, String)]) -> String {
    let (_, body) = split_frontmatter(source);
    if fields.iter().all(|(key, _)| key.trim().is_empty()) {
        return body.to_string();
    }
    let mut out = String::from("---\n");
    for (key, value) in fields {
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        out.push_str(key);
        out.push(':');
        if !value.trim().is_empty() {
            out.push(' ');
            out.push_str(value.trim());
        }
        out.push('\n');
    }
    out.push_str("---\n");
    out.push_str(body);
    out
}

pub fn outline(source: &str) -> Vec<Heading> {
    let (frontmatter, body) = split_frontmatter(source);
    let base = frontmatter.map(str::len).unwrap_or(0);
    let mut headings = Vec::new();
    let mut offset = base;
    let mut fence: Option<(char, usize)> = None;

    for line in body.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        let trimmed = text.trim_start();

        if let Some((marker, len)) = fence {
            let run = trimmed.chars().take_while(|c| *c == marker).count();
            if run >= len && trimmed.chars().skip(run).all(|c| c.is_whitespace()) {
                fence = None;
            }
        } else if let Some((marker, len)) = fence_start(trimmed) {
            fence = Some((marker, len));
        } else if let Some((level, title)) = parse_heading(trimmed) {
            let indent = text.len() - trimmed.len();
            headings.push(Heading {
                level,
                title,
                offset: offset + indent,
            });
        }

        offset += line.len();
    }

    headings
}

fn fence_start(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|c| *c == marker).count();
    (len >= 3).then_some((marker, len))
}

/// Languages the code-block language picker offers, as the token written
/// into the fence paired with its display name. Every token resolves in the
/// grammar set the preview highlights with (syntect's syntaxes plus bat's
/// extras), so picking one turns syntax highlighting on.
pub const CODE_LANGUAGES: &[(&str, &str)] = &[
    ("ada", "Ada"),
    ("asm", "Assembly"),
    ("awk", "AWK"),
    ("bash", "Bash / Shell"),
    ("c", "C"),
    ("cmd", "Batch / CMD"),
    ("cmake", "CMake"),
    ("cpp", "C++"),
    ("csharp", "C#"),
    ("css", "CSS"),
    ("csv", "CSV"),
    ("dart", "Dart"),
    ("diff", "Diff"),
    ("dockerfile", "Dockerfile"),
    ("elixir", "Elixir"),
    ("erb", "ERB"),
    ("erlang", "Erlang"),
    ("f#", "F#"),
    ("fish", "Fish"),
    ("go", "Go"),
    ("gradle", "Gradle"),
    ("graphql", "GraphQL"),
    ("groovy", "Groovy"),
    ("haskell", "Haskell"),
    ("html", "HTML"),
    ("ini", "INI"),
    ("java", "Java"),
    ("javascript", "JavaScript"),
    ("jinja", "Jinja"),
    ("json", "JSON"),
    ("julia", "Julia"),
    ("kotlin", "Kotlin"),
    ("latex", "LaTeX"),
    ("less", "Less"),
    ("lisp", "Lisp"),
    ("lua", "Lua"),
    ("makefile", "Makefile"),
    ("markdown", "Markdown"),
    ("matlab", "MATLAB"),
    ("nim", "Nim"),
    ("nix", "Nix"),
    ("objective-c", "Objective-C"),
    ("ocaml", "OCaml"),
    ("perl", "Perl"),
    ("php", "PHP"),
    ("proto", "Protocol Buffers"),
    ("python", "Python"),
    ("r", "R"),
    ("ruby", "Ruby"),
    ("rust", "Rust"),
    ("sass", "Sass"),
    ("scala", "Scala"),
    ("scss", "SCSS"),
    ("svelte", "Svelte"),
    ("swift", "Swift"),
    ("tcl", "Tcl"),
    ("terraform", "Terraform"),
    ("toml", "TOML"),
    ("ts", "TypeScript"),
    ("tsx", "TSX / React"),
    ("twig", "Twig"),
    ("vim", "Vim script"),
    ("vue", "Vue"),
    ("xml", "XML"),
    ("yaml", "YAML"),
    ("zig", "Zig"),
    ("zsh", "Zsh"),
];

/// The picker's display name for a fence language token, when it knows it.
pub fn code_language_display(token: &str) -> Option<&'static str> {
    CODE_LANGUAGES
        .iter()
        .find(|(known, _)| *known == token)
        .map(|(_, display)| *display)
}

/// The byte range of the language word on the opening fence line of the code
/// block starting at `block_start`, or `None` when that offset does not sit
/// on a fence. An empty range means the fence carries no language yet, so
/// inserting at it adds one.
///
/// The range covers only the first word; a fence meta suffix such as
/// ` ```rust ignore ` keeps its rest untouched.
pub fn fence_lang_range(source: &str, block_start: usize) -> Option<Range<usize>> {
    if block_start > source.len() {
        return None;
    }
    // The span can start at the fence itself rather than at the line start,
    // so anchor on the line that contains it.
    let line_start = source[..block_start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |i| line_start + i);
    let line = &source[line_start..line_end];
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let info_start = line_start + indent;
    let info = &source[info_start..line_end];
    let marker = info.as_bytes().first().copied()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let fence_len = info.len() - info.trim_start_matches(marker as char).len();
    if fence_len < 3 {
        return None;
    }
    let rest = &info[fence_len..];
    let word = rest.trim_start_matches([' ', '\t']);
    if word.is_empty() {
        return Some((info_start + fence_len)..(info_start + info.len()));
    }
    let lead = rest.len() - word.len();
    let word_len = word.find(char::is_whitespace).unwrap_or(word.len());
    Some((info_start + fence_len + lead)..(info_start + fence_len + lead + word_len))
}

fn parse_heading(trimmed: &str) -> Option<(usize, String)> {
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim().to_string();
    (!title.is_empty()).then_some((level, title))
}

pub fn stats(source: &str) -> Stats {
    let (_, body) = split_frontmatter(source);
    let mut chars = 0usize;
    let mut cjk = 0usize;
    let mut latin_words = 0usize;
    let mut in_word = false;

    for ch in body.chars() {
        if !ch.is_whitespace() {
            chars += 1;
        }
        if is_cjk(ch) {
            cjk += 1;
            in_word = false;
        } else if ch.is_alphanumeric() {
            if !in_word {
                latin_words += 1;
                in_word = true;
            }
        } else {
            in_word = false;
        }
    }

    let lines = if source.is_empty() {
        1
    } else {
        source.lines().count().max(1)
    };
    // Chinese reads at roughly 300 characters a minute, Latin prose at about
    // 220 words; weighting a Latin word as 1.5 CJK characters meets in between.
    let load = cjk as f32 + latin_words as f32 * 1.5;
    let minutes = if load <= 0.0 {
        0
    } else {
        (load / 300.0).ceil().max(1.0) as usize
    };

    Stats {
        chars,
        lines,
        minutes,
    }
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF        // kana
        | 0x3400..=0x4DBF      // CJK extension A
        | 0x4E00..=0x9FFF      // CJK unified ideographs
        | 0xF900..=0xFAFF      // compatibility ideographs
        | 0xAC00..=0xD7AF      // hangul
    )
}

/// Render the document as a standalone, self-contained HTML page.
pub fn export_html(title: &str, source: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let (_, body) = split_frontmatter(source);
    let parser = Parser::new_ext(body, options);
    let mut body = String::new();
    html::push_html(&mut body, parser);

    let safe_title = title
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{safe_title}</title>
<style>
  :root {{
    color-scheme: light dark;
    --paper: #f6f2ea;
    --ink: #25221c;
    --muted: #857b6b;
    --line: #e3dacb;
    --seal: #b4552d;
    --code: #efe9de;
  }}
  @media (prefers-color-scheme: dark) {{
    :root {{
      --paper: #14161b;
      --ink: #e9e2d6;
      --muted: #8e8a80;
      --line: #2a2c33;
      --seal: #e8a87c;
      --code: #1c1f25;
    }}
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0;
    padding: 72px 24px 120px;
    background: var(--paper);
    color: var(--ink);
    font-family: "Segoe UI", "PingFang SC", "Microsoft YaHei UI", system-ui, sans-serif;
    font-size: 17px;
    line-height: 1.85;
    text-rendering: optimizeLegibility;
  }}
  main {{ max-width: 720px; margin: 0 auto; }}
  h1, h2, h3, h4 {{ line-height: 1.3; letter-spacing: -0.01em; margin: 2em 0 0.7em; }}
  h1 {{ font-size: 2.1em; margin-top: 0; }}
  h2 {{ font-size: 1.5em; padding-bottom: 0.35em; border-bottom: 1px solid var(--line); }}
  h3 {{ font-size: 1.2em; }}
  p {{ margin: 0.9em 0; }}
  a {{ color: var(--seal); text-underline-offset: 3px; }}
  blockquote {{
    margin: 1.4em 0; padding: 0.2em 1.2em;
    border-left: 3px solid var(--seal);
    color: var(--muted);
  }}
  code {{
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    font-size: 0.88em; background: var(--code);
    padding: 0.15em 0.4em; border-radius: 5px;
  }}
  pre {{
    background: var(--code); border: 1px solid var(--line);
    border-radius: 10px; padding: 18px 20px; overflow-x: auto; line-height: 1.6;
  }}
  pre code {{ background: none; padding: 0; font-size: 0.86em; }}
  table {{ border-collapse: collapse; width: 100%; margin: 1.4em 0; font-size: 0.94em; }}
  th, td {{ border: 1px solid var(--line); padding: 9px 14px; text-align: left; }}
  th {{ background: var(--code); }}
  hr {{ border: 0; border-top: 1px solid var(--line); margin: 2.4em 0; }}
  img {{ max-width: 100%; border-radius: 8px; }}
  li {{ margin: 0.35em 0; }}
  input[type="checkbox"] {{ margin-right: 0.5em; accent-color: var(--seal); }}
</style>
</head>
<body>
<main>
{body}
</main>
</body>
</html>
"#
    )
}

#[derive(Clone, Copy, Debug)]
pub struct DocumentTemplate {
    /// Short label in the template picker.
    pub name: &'static str,
    pub description: &'static str,
    /// Suggested file name without the extension.
    pub file_stem: &'static str,
    pub content: &'static str,
}

/// A small, opinionated set of starting points. They are plain Markdown with
/// a frontmatter block, so every existing feature (metadata panel, outline,
/// stats, publish) works on them unchanged.
pub const TEMPLATES: &[DocumentTemplate] = &[
    DocumentTemplate {
        name: "普通文章",
        description: "标题、摘要、正文与总结，适合公众号或博客",
        file_stem: "新文章",
        content: r#"---
title: 未命名文章
author:
tags:
---

# 未命名文章

> 用一句话说清楚这篇文章要讲什么。

## 背景

为什么写这件事？读者现在处在什么情境里？

## 正文

### 第一个要点

展开你的观点，给一个具体的例子或数据。

### 第二个要点

补充论据，或者换一个角度看同一个问题。

## 总结

回到开头的问题，给读者一个可以带走的结论。

- 要点一
- 要点二
- 要点三
"#,
    },
    DocumentTemplate {
        name: "技术笔记",
        description: "问题、结论、过程与参考，适合记录一次排查",
        file_stem: "技术笔记",
        content: r#"---
title: 技术笔记
tags:
created:
---

# 技术笔记

## 结论先行

一句话说明问题是怎么解决的。

## 问题

- 现象：
- 影响范围：
- 复现步骤：

## 排查过程

```text
在这里贴关键日志或命令
```

1. 第一步观察到了什么
2. 第二步排除了什么
3. 最后定位到哪里

## 方案

说明改动、取舍和风险。

## 参考

- [链接标题](https://example.com)
"#,
    },
    DocumentTemplate {
        name: "周报",
        description: "本周进展、风险与下周计划",
        file_stem: "周报",
        content: r#"---
title: 本周周报
week:
author:
tags: 周报
---

# 本周周报

## 本周进展

- [x] 完成事项一
- [x] 完成事项二
- [ ] 顺延事项

## 关键结果

| 事项 | 状态 | 说明 |
| --- | --- | --- |
| 事项一 | 已完成 | 结果或数据 |
| 事项二 | 进行中 | 当前进度 |

## 风险与阻塞

- 风险：
- 需要谁支持：

## 下周计划

1. 填写下周第一件事
2. 填写下周第二件事
3. 填写下周第三件事
"#,
    },
    DocumentTemplate {
        name: "会议纪要",
        description: "议题、结论与待办，会后直接发出去",
        file_stem: "会议纪要",
        content: r#"---
title: 会议纪要
date:
attendees:
tags: 会议
---

# 会议纪要

- **时间**：
- **参会人**：
- **主持人**：
- **记录人**：

## 议题

1. 议题一
2. 议题二

## 讨论要点

- 记录讨论中的关键判断

## 结论

- 结论一

## 待办

- [ ] 事项 — 负责人 — 截止时间
- [ ] 事项 — 负责人 — 截止时间
"#,
    },
];

impl DocumentTemplate {
    pub fn document(&self, id: u64) -> Document {
        Document {
            id,
            title: format!("{}.md", self.file_stem).into(),
            path: None,
            content: self.content.to_string(),
            // Templates start unsaved, so the title bar shows the dirty hint
            // until the user picks a real file.
            saved_content: String::new(),
        }
    }
}

pub fn sample_documents() -> Vec<Document> {
    vec![
        Document::sample(1, "欢迎使用 Inkdown.md", WELCOME),
        Document::sample(2, "Markdown 语法速查.md", CHEATSHEET),
        Document::sample(3, "给未来的自己.md", LETTER),
    ]
}

const WELCOME: &str = r#"# 欢迎使用 Inkdown

> 一款用 Rust 与 GPUI Kit 打磨的 Markdown 编辑器。
> 左边是文字，右边是它长成的样子。

## 为什么是 Inkdown？

Inkdown 把**写作**放回第一位。没有复杂的配置，没有打断思路的弹窗，
只有一张安静的稿纸和恰到好处的工具。

- **实时预览**：每一笔修改，右侧都会同步呈现。
- **专注排版**：标题、引用、代码块与表格都有细腻的样式。
- **原生性能**：Rust 编译，毫秒级响应，长文也不卡顿。
- **随心切换**：写作 / 分栏 / 预览三种视图，配合明暗主题。

## 开始写第一行

把光标移到下面任意位置，试着输入。你也可以选中一段文字，
再点击工具栏上的 **B** 把它加粗。

### 一个小例子

```rust
fn main() {
    let mut editor = Inkdown::new();
    editor.write("Hello, Markdown!");
    editor.save("welcome.md");
}
```

> “写作是用文字思考。工具应该消失，只留下想法。”
> —— 某个安静的下午

## 快捷键

| 操作 | 快捷键 |
| --- | --- |
| 保存文档 | `Ctrl + S` |
| 新建文档 | `Ctrl + N` |
| 加粗 | `Ctrl + B` |
| 斜体 | `Ctrl + I` |
| 行内代码 | `Ctrl + E` |
| 切换主题 | `Ctrl + Shift + T` |

## 今日清单

- [x] 打开 Inkdown
- [x] 读到这里
- [ ] 写下第一段属于自己的文字
- [ ] 保存一份 `.md` 文件

---

祝你写得愉快。写完后，按 **Ctrl + S** 保存到磁盘。
"#;

const CHEATSHEET: &str = r#"# Markdown 语法速查

一份随手可查的小抄。下面每一种写法，右侧都能看到实际效果。

## 标题

```markdown
# 一级标题
## 二级标题
### 三级标题
```

## 强调

```markdown
**加粗文字**
*斜体文字*
~~删除线~~
`行内代码`
```

效果：**加粗文字**、*斜体文字*、~~删除线~~、`行内代码`。

## 列表

```markdown
- 无序列表
- 第二项
  - 嵌套项

1. 有序列表
2. 第二项

- [ ] 待办事项
- [x] 已完成
```

- 无序列表
- 第二项
  - 嵌套项

1. 有序列表
2. 第二项

- [ ] 待办事项
- [x] 已完成

## 引用与分隔线

```markdown
> 引用一段话。
> 可以写很多行。

---
```

> 引用一段话。
> 可以写很多行。

---

## 链接与图片

```markdown
[链接文字](https://example.com)
![图片描述](https://example.com/image.png)
```

## 代码

```python
def greet(name: str) -> str:
    return f"Hello, {name}!"
```

## 表格

| 左对齐 | 居中 | 右对齐 |
| :--- | :---: | ---: |
| 苹果 | 3.20 | 12 |
| 香蕉 | 1.50 | 8 |

## 提示

- 标题、列表符号后面都要留一个空格。
- 空行分隔段落，阅读更舒服。
- 把长文档拆成多个 `##` 小节，结构自然清晰。
"#;

const LETTER: &str = r#"# 给未来的自己

写给一年后的你。写这封信的时候，窗外正下着雨，桌上的茶已经凉了一半。

## 此刻在做什么

我在学着把想法写下来，而不是让它们在脑海里打转。Inkdown 的空白页有一种奇妙的安定感：
没有多余的按钮，没有催促，只有光标在安静地闪。

> 慢一点。把一句话写清楚，比写十句漂亮话更难，也更值得。

## 想记住的三件事

1. **保持好奇。** 遇到不懂的事，先查一查，再下结论。
2. **保持笨拙。** 允许自己写得不好，初稿本来就不该完美。
3. **保持手感。** 每天写一点，哪怕只有三行。

## 一年后要回答的问题

- [ ] 还在写东西吗？
- [ ] 有没有完成那个一直在拖延的项目？
- [ ] 读过的书里，哪一本真的改变了你？
- [ ] 今天的心情，比去年更从容了吗？

## 最后

如果这一年过得很快，没关系。翻到这里的时候，至少证明你又认真活过一段时间。

*—— 写于一个普通的下午*
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_yaml_frontmatter() {
        let source = "---\ntitle: 逆向\nauthor: \naccount: 未知公众号\n---\n# 正文\n\n内容";
        let (front, body) = split_frontmatter(source);
        let front = front.expect("frontmatter");
        assert!(front.starts_with("---\n"));
        assert!(front.ends_with("---\n"));
        assert_eq!(body, "# 正文\n\n内容");
        let headings = outline(source);
        assert_eq!(headings[0].title, "正文");
        assert_eq!(headings[0].offset, front.len());
        assert_eq!(stats(source).lines, source.lines().count());
    }

    #[test]
    fn edits_frontmatter_fields() {
        let source = "---\ntitle: 逆向\nauthor: \n---\n# 正文\n";
        let fields = frontmatter_fields(source);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].key, "title");
        assert_eq!(fields[0].value, "逆向");
        let updated = set_frontmatter_fields(
            source,
            &[
                ("title".to_string(), "新标题".to_string()),
                ("url".to_string(), "https://example.com".to_string()),
            ],
        );
        assert!(updated.starts_with("---\ntitle: 新标题\nurl: https://example.com\n---\n"));
        assert!(updated.ends_with("# 正文\n"));
        assert_eq!(set_frontmatter_fields(source, &[]), "# 正文\n");
    }

    #[test]
    fn unterminated_frontmatter_passes_through() {
        let source = "---\n# 标题\n";
        assert_eq!(split_frontmatter(source), (None, source));
        let plain = "# 标题";
        assert_eq!(split_frontmatter(plain), (None, plain));
    }

    #[test]
    fn templates_are_publishable_documents() {
        assert!(TEMPLATES.len() >= 4);
        for template in TEMPLATES {
            assert!(!template.name.is_empty());
            assert!(!template.file_stem.is_empty());
            assert!(!template.content.trim().is_empty());
            assert!(template.content.lines().any(|line| line.starts_with("# ")));
            assert!(!frontmatter_fields(template.content).is_empty());
            let document = template.document(1);
            assert!(document.is_dirty());
            assert!(document.title.ends_with(".md"));
            assert!(split_frontmatter(&document.content).0.is_some());
        }
    }

    #[test]
    fn finds_language_word_on_fence_lines() {
        let source = "# t\n\n```rust\nfn main() {}\n```\n";
        let fence = source.find("```rust").unwrap();
        assert_eq!(fence_lang_range(source, fence), Some(8..12));

        // The span may point a few bytes into the fence line itself.
        assert_eq!(fence_lang_range(source, fence + 3), Some(8..12));
        // A line that is not a fence yields nothing.
        assert_eq!(
            fence_lang_range(source, source.find("fn main").unwrap()),
            None
        );

        // No language: an empty range right after the fence marks the spot.
        let bare = "``` \n\ncode\n```";
        assert_eq!(fence_lang_range(bare, 0), Some(3..4));

        // Meta after the language is left alone; the word itself is replaced.
        let meta = "```js title=demo\ncode\n```";
        assert_eq!(fence_lang_range(meta, 0), Some(3..5));

        // Four backtick fences, tilde fences and indented fences all work.
        let long = "````python\nx\n````";
        assert_eq!(fence_lang_range(long, 0), Some(4..10));
        let tilde = "~~~go\nx\n~~~";
        assert_eq!(fence_lang_range(tilde, 0), Some(3..5));
        let indented = "   ```ts\nx\n```";
        assert_eq!(fence_lang_range(indented, 3), Some(6..8));

        // Not a fence, too deep an indent, past the end: no range.
        assert_eq!(fence_lang_range("# heading", 0), None);
        assert_eq!(fence_lang_range("    ```rust\nx", 4), None);
        assert_eq!(fence_lang_range("```", 99), None);
    }

    #[test]
    fn code_language_picker_tokens_all_have_display_names() {
        for (token, display) in CODE_LANGUAGES {
            assert!(!token.is_empty());
            assert!(!display.is_empty());
            assert_eq!(code_language_display(token), Some(*display));
        }
        assert_eq!(code_language_display("no-such-language"), None);
    }
}
