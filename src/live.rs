//! 实时（分块所见即所得）视图的纯逻辑：块的 Enter 分类、段落切分与解析扩展。
//!
//! 渲染与状态流转在 `app.rs`；这里只放可单测的函数。

use std::sync::Arc;

use gpui_kit::base::text::editor_host::{self, BlockNode};
use gpui_kit::component::text::{FrontmatterPlugin, MarkdownExtensions};

/// 智能 Enter 的落点：普通段落切开续写，结构化块内部换行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnterAction {
    /// 光标前后切成两块（当前块提交，光标后的文字成为其后的新段落）。
    Split,
    /// 块内换行——代码、列表、引用里的裸换行会破坏 Markdown 结构。
    Newline,
}

/// 块的 Enter 行为。段落与标题走 [`EnterAction::Split`]；代码块、列表、
/// 引用、表格、frontmatter 等结构化块走 [`EnterAction::Newline`]。
pub fn enter_action(block: &BlockNode) -> EnterAction {
    match block {
        BlockNode::CodeBlock(_)
        | BlockNode::List { .. }
        | BlockNode::Blockquote { .. }
        | BlockNode::Surface { .. }
        | BlockNode::Table(_)
        | BlockNode::Custom { .. }
        | BlockNode::Definition { .. } => EnterAction::Newline,
        _ => EnterAction::Split,
    }
}

/// 在光标处把块源码切成两半：前半留在原块，后半成为其后的新段落。
/// 光标落在 UTF-8 字符中间时回退到字符边界。
pub fn split_for_enter(block_source: &str, caret: usize) -> (String, String) {
    let mut caret = caret.min(block_source.len());
    while caret > 0 && !block_source.is_char_boundary(caret) {
        caret -= 1;
    }
    (
        block_source[..caret].to_string(),
        block_source[caret..].to_string(),
    )
}

/// 与预览完全一致的解析扩展（frontmatter 卡片同款），保证两种模式渲染同构。
pub fn extensions() -> Arc<MarkdownExtensions> {
    Arc::new(
        MarkdownExtensions::default()
            .frontmatter()
            .plugin(FrontmatterPlugin::new()),
    )
}

/// 把整篇源码解析成顶层块树。解析失败（基本不会发生）退化为空文档。
pub fn parse(source: &str) -> Vec<BlockNode> {
    editor_host::parse_markdown(source, extensions()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# 标题\n\n普通段落第一行\n第二行。\n\n- 项目一\n- 项目二\n\n> 引用一段\n\n```rust\nfn main() {}\n```\n\n---\n";

    fn kinds(source: &str) -> Vec<(&'static str, EnterAction)> {
        parse(source)
            .iter()
            .map(|block| {
                let name = match block {
                    BlockNode::Heading { .. } => "heading",
                    BlockNode::Paragraph(_) => "paragraph",
                    BlockNode::List { .. } => "list",
                    BlockNode::Blockquote { .. } => "quote",
                    BlockNode::CodeBlock(_) => "code",
                    BlockNode::HorizontalRule { .. } => "hr",
                    BlockNode::Custom { .. } => "custom",
                    _ => "other",
                };
                (name, enter_action(block))
            })
            .collect()
    }

    #[test]
    fn enter_action_classifies_structured_blocks() {
        let blocks = kinds(SAMPLE);
        assert!(blocks.contains(&("heading", EnterAction::Split)));
        assert!(blocks.contains(&("paragraph", EnterAction::Split)));
        assert!(blocks.contains(&("list", EnterAction::Newline)));
        assert!(blocks.contains(&("quote", EnterAction::Newline)));
        assert!(blocks.contains(&("code", EnterAction::Newline)));
        assert!(blocks.contains(&("hr", EnterAction::Split)));
        // 两行的段落是一个块，不拆开。
        assert_eq!(
            blocks
                .iter()
                .filter(|(name, _)| *name == "paragraph")
                .count(),
            1
        );
    }

    #[test]
    fn enter_action_frontmatter_is_newline() {
        let source = "---\ntitle: 测试\n---\n\n正文\n";
        let blocks = kinds(source);
        assert!(
            blocks.contains(&("custom", EnterAction::Newline)),
            "frontmatter 应是 Custom 块且 Enter 换行，实际 {blocks:?}"
        );
    }

    #[test]
    fn split_at_caret_keeps_both_halves() {
        let (stay, rest) = split_for_enter("Hello world", 5);
        assert_eq!(stay, "Hello");
        assert_eq!(rest, " world");
    }

    #[test]
    fn split_at_end_leaves_empty_next_block() {
        // “## 标题” 共 9 字节（中文各 3），末尾切开 → 新段落为空。
        let source = "## 标题";
        let (stay, rest) = split_for_enter(source, source.len());
        assert_eq!(stay, "## 标题");
        assert!(rest.is_empty());
    }

    #[test]
    fn split_at_start_moves_everything_down() {
        let (stay, rest) = split_for_enter("内容", 0);
        assert!(stay.is_empty());
        assert_eq!(rest, "内容");
    }

    #[test]
    fn split_clamps_to_utf8_boundary() {
        // “中”占3字节，字节1落在字符中间。
        let (stay, rest) = split_for_enter("中文", 1);
        assert_eq!(stay, "");
        assert_eq!(rest, "中文");
        // 越界光标按末尾处理。
        let (stay, rest) = split_for_enter("中文", 99);
        assert_eq!(stay, "中文");
        assert!(rest.is_empty());
    }

    #[test]
    fn parse_spans_are_monotonic_byte_ranges() {
        let source = SAMPLE;
        let mut last_end = 0;
        let mut saw_span = 0;
        for block in parse(source) {
            let Some(span) = block.span() else {
                continue;
            };
            assert!(span.start >= last_end, "块 span 重叠: {span:?}");
            assert!(span.end <= source.len(), "块 span 越界: {span:?}");
            last_end = span.end;
            saw_span += 1;
        }
        assert!(saw_span >= 6, "样例至少解析出 6 个带 span 的块");
    }
}
