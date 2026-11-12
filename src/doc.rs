//! The document model, plus the small markdown utilities the UI needs:
//! an outline, writing statistics and a self-contained HTML export.

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

pub fn outline(source: &str) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut offset = 0usize;
    let mut fence: Option<(char, usize)> = None;

    for line in source.split_inclusive('\n') {
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
    let mut chars = 0usize;
    let mut cjk = 0usize;
    let mut latin_words = 0usize;
    let mut in_word = false;

    for ch in source.chars() {
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

    let parser = Parser::new_ext(source, options);
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
