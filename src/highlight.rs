//! Code-block syntax highlighting for the preview.
//!
//! gpui-kit ships a per-language highlighter behind a crate-private seam, so
//! Inkdown installs its own through `TextViewDefaults`: the same bat grammar
//! set, plus the fence aliases in [`doc::LANG_ALIASES`] — ` ```golang `
//! colours as Go instead of falling back to plain text.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, LazyLock};

use gpui_kit::base::text::CodeBlock;
use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::*;
use syntect::parsing::{Scope, ScopeStack, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::doc;

/// The bat grammar set, built the same way gpui-kit builds its own copy
/// (kit's static is private, so this is a second one-time build).
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// The highlighter `theme::apply` installs as the application default.
pub fn block_highlighter(
    theme: Arc<HighlightTheme>,
) -> impl Fn(&CodeBlock) -> Vec<(Range<usize>, HighlightStyle)> + Send + Sync + 'static {
    move |block| highlight_block(&theme, block)
}

/// Highlights for one code block, as ranges over `block.code()`.
///
/// A block without a language, or one whose token no grammar knows, comes
/// back empty — it renders as plain text on the code background.
pub fn highlight_block(
    theme: &HighlightTheme,
    block: &CodeBlock,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let Some(lang) = block.lang() else {
        return Vec::new();
    };
    let Some(syntax) = find_syntax(&lang) else {
        return Vec::new();
    };
    let code = block.code();
    resolve(&parse(syntax, &code), &(0..code.len()), theme)
}

/// The grammar for a fence token: our alias table first (so `shell` lands on
/// Bash), then the raw token for anything syntect knows that we don't list.
fn find_syntax(language: &str) -> Option<&'static SyntaxReference> {
    let canonical = doc::normalize_code_lang(language).unwrap_or(language);
    match canonical {
        "text" | "plaintext" | "plain" => return None,
        _ => {}
    }
    let mut tokens = vec![canonical];
    if language != canonical {
        tokens.push(language);
    }
    if canonical == "csharp" {
        tokens.push("cs");
    }
    if canonical == "makefile" {
        tokens.push("Makefile");
    }
    tokens
        .into_iter()
        .find_map(|token| SYNTAXES.find_syntax_by_token(token))
        .filter(|syntax| syntax.name != "Plain Text")
}

/// Runs syntect over the block and keeps the spans that map to a theme name.
fn parse(syntax: &SyntaxReference, text: &str) -> Vec<(Range<usize>, &'static str)> {
    let mut parser = syntect::parsing::ParseState::new(syntax);
    let mut scopes = ScopeStack::new();
    let mut names = HashMap::<Scope, Option<&'static str>>::new();
    let mut highlights: Vec<(Range<usize>, &'static str)> = Vec::new();
    let mut offset = 0;

    // The innermost scope that names something wins, so the quotes of a
    // string colour as the string rather than as punctuation.
    let mut push = |range: Range<usize>,
                    scopes: &ScopeStack,
                    highlights: &mut Vec<(Range<usize>, &'static str)>| {
        if range.is_empty() {
            return;
        }
        let name = scopes
            .scopes
            .iter()
            .rev()
            .find_map(|scope| *names.entry(*scope).or_insert_with(|| semantic_name(*scope)));
        if let Some(name) = name {
            match highlights.last_mut() {
                Some((last, last_name)) if last.end == range.start && *last_name == name => {
                    last.end = range.end;
                }
                _ => highlights.push((range, name)),
            }
        }
    };

    for line in LinesWithEndings::from(text) {
        // A line syntect cannot parse keeps the scopes it started with; the
        // next line starts over from them rather than losing the document.
        if let Ok(operations) = parser.parse_line(line, &SYNTAXES) {
            let mut cursor = 0;
            for (index, operation) in operations {
                push(offset + cursor..offset + index, &scopes, &mut highlights);
                let _ = scopes.apply(&operation);
                cursor = index;
            }
            push(
                offset + cursor..offset + line.len(),
                &scopes,
                &mut highlights,
            );
        }
        offset += line.len();
    }
    highlights
}

/// Gap-free style runs for `range`: spans get their theme colour, everything
/// between them the default (code foreground).
fn resolve(
    highlights: &[(Range<usize>, &'static str)],
    range: &Range<usize>,
    theme: &HighlightTheme,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut runs = Vec::new();
    let mut cursor = range.start;
    for (highlight, name) in highlights {
        if highlight.start >= range.end {
            break;
        }
        let start = highlight.start.max(range.start);
        let end = highlight.end.min(range.end);
        if start >= end || end <= cursor {
            continue;
        }
        if cursor < start {
            runs.push((cursor..start, HighlightStyle::default()));
        }
        runs.push((start..end, theme.style(name).unwrap_or_default()));
        cursor = end;
    }
    if cursor < range.end {
        runs.push((cursor..range.end, HighlightStyle::default()));
    }
    runs
}

/// The theme name for a TextMate scope, or `None` to let an enclosing scope
/// decide. Standard syntect scope taxonomy mapped onto the Zed-style names
/// `SyntaxColors` carries (`comment`, `string`, `keyword`, …).
fn semantic_name(scope: Scope) -> Option<&'static str> {
    let scope = scope.build_string();
    let is = |prefix: &str| {
        scope == prefix
            || scope
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
    };

    // Delimiters belong to what they delimit; containers carry no colour
    // of their own.
    if is("punctuation.definition.string")
        || is("punctuation.definition.comment")
        || is("invalid")
        || is("meta")
        || is("source")
        || is("text")
    {
        return None;
    }

    Some(
        if is("comment.block.documentation") || is("comment.line.documentation") {
            "comment.doc"
        } else if is("comment") {
            "comment"
        } else if is("constant.character.escape") {
            "string.escape"
        } else if is("string.regexp") {
            "string.regex"
        } else if is("string") {
            "string"
        } else if is("constant.numeric") {
            "number"
        } else if is("constant.language.boolean") {
            "boolean"
        } else if is("constant") || is("support.constant") {
            "constant"
        } else if is("keyword.operator") {
            "operator"
        } else if is("keyword.control.import") || is("meta.preprocessor") {
            "preproc"
        } else if is("keyword") || is("storage.modifier") {
            "keyword"
        } else if is("storage.type") {
            // `fn`, `class` and `let` declare; `int` and `bool` are types. Match
            // whole segments after the prefix — a bare `contains("type")` would
            // hit the `storage.type` path itself and mislabel every primitive.
            let declares = [
                "class",
                "enum",
                "function",
                "impl",
                "interface",
                "module",
                "namespace",
                "struct",
                "trait",
                "let",
                "var",
                "const",
            ];
            if scope
                .trim_start_matches("storage.type.")
                .split('.')
                .any(|segment| declares.contains(&segment))
            {
                "keyword"
            } else {
                "type"
            }
        } else if is("entity.name.function")
            || is("support.function")
            || is("variable.function")
            || is("meta.function-call")
        {
            "function"
        } else if is("entity.name.type")
            || is("entity.name.class")
            || is("entity.name.struct")
            || is("entity.name.enum")
            || is("entity.name.trait")
            || is("entity.name.interface")
            || is("entity.other.inherited-class")
            || is("support.type")
            || is("support.class")
        {
            "type"
        } else if is("entity.name.tag") {
            "tag"
        } else if is("entity.other.attribute-name") {
            "attribute"
        } else if is("entity.name.label") {
            "label"
        } else if is("entity.name.section") || is("markup.heading") {
            "title"
        } else if is("variable.language") {
            "variable.special"
        } else if is("variable.other.member")
            || is("variable.other.property")
            || is("support.variable.property")
            || is("meta.mapping.key")
        {
            "property"
        } else if is("variable") {
            "variable"
        } else if is("punctuation.section") || is("punctuation.definition.block") {
            "punctuation.bracket"
        } else if is("punctuation.separator") || is("punctuation.terminator") {
            "punctuation.delimiter"
        } else if is("punctuation") {
            "punctuation"
        } else {
            return None;
        },
    )
}

#[cfg(test)]
mod tests {
    use syntect::parsing::Scope;

    use super::{find_syntax, parse, resolve, semantic_name};
    use crate::doc;
    use gpui_kit::component::highlighter::HighlightTheme;

    #[test]
    fn resolves_picker_and_alias_tokens() {
        // Every picker token the app claims resolves.
        for (token, _) in doc::CODE_LANGUAGES {
            assert!(
                find_syntax(token).is_some(),
                "picker token {token} has no grammar"
            );
        }
        // Aliases land on their target's grammar.
        assert!(find_syntax("golang").is_some());
        assert_eq!(
            find_syntax("golang").map(|s| s.name.as_str()),
            find_syntax("go").map(|s| s.name.as_str())
        );
        // Explicit plain-text spellings stay unhighlighted.
        assert!(find_syntax("text").is_none());
        assert!(find_syntax("plaintext").is_none());
        // Unknown tokens fall through to syntect's own knowledge or None.
        assert!(find_syntax("no-such-language-xyz").is_none());
    }

    #[test]
    fn scope_taxonomy_hits_the_common_families() {
        let scope = |s: &str| Scope::new(s).unwrap();
        assert_eq!(
            semantic_name(scope("comment.line.double-slash")),
            Some("comment")
        );
        assert_eq!(
            semantic_name(scope("comment.line.documentation.python")),
            Some("comment.doc")
        );
        assert_eq!(semantic_name(scope("string.quoted.double")), Some("string"));
        assert_eq!(
            semantic_name(scope("punctuation.definition.string.begin")),
            None
        );
        assert_eq!(
            semantic_name(scope("keyword.control.flow")),
            Some("keyword")
        );
        assert_eq!(semantic_name(scope("storage.type.primitive")), Some("type"));
        assert_eq!(
            semantic_name(scope("storage.type.function.rust")),
            Some("keyword")
        );
        assert_eq!(
            semantic_name(scope("entity.name.function")),
            Some("function")
        );
        assert_eq!(semantic_name(scope("constant.numeric")), Some("number"));
        // Containers (`meta.*`) carry no colour of their own by design.
        assert_eq!(semantic_name(scope("meta.function-call")), None);
        assert_eq!(
            semantic_name(scope("support.function.builtin.python")),
            Some("function")
        );
    }

    #[test]
    fn highlights_coloured_runs_for_a_known_language() {
        let theme = HighlightTheme::default_light();
        let syntax = find_syntax("python").unwrap();
        let code = "def add(a, b):
    return a + b  # sum
";
        let spans = parse(syntax, code);
        assert!(!spans.is_empty(), "python snippet produced no spans");
        let resolved = resolve(&spans, &(0..code.len()), &theme);
        // Gaps are filled, runs are ordered and inside the block.
        assert!(resolved.windows(2).all(|w| w[0].0.end <= w[1].0.start));
        assert_eq!(resolved.last().map(|(r, _)| r.end), Some(code.len()));
        // At least one span carries a real colour (not all default).
        assert!(resolved.iter().any(|(_, style)| style.color.is_some()));
    }
}
