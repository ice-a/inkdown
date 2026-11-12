//! Publishing pipeline.
//!
//! * Images: local images are uploaded to the configured image host and the
//!   Markdown source is rewritten.
//! * HTML: a dedicated inline-style renderer that survives rich-text editors
//!   such as WeChat's, Zhihu's and Toutiao's. WeChat also supports a reference
//!   list for external links, because its editor strips them from bodies.
//! * APIs: WeChat draft box (official account) and cnblogs MetaWeblog.
//! * Other platforms: rich HTML goes to the clipboard and the editor page is
//!   opened in the browser.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use pulldown_cmark::{
    Alignment, Event, Options, Parser, Tag, TagEnd,
};
use serde_json::{Value, json};

use crate::config::{CnblogsConfig, ImageHostConfig, ImageHostKind, WechatConfig};
use crate::doc::split_frontmatter;
use crate::imagebed;

// ---------------------------------------------------------------------------
// Platforms

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Wechat,
    Zhihu,
    Toutiao,
    Csdn,
    Cnblogs,
    Jianshu,
    Juejin,
}

impl Platform {
    pub const ALL: [Platform; 7] = [
        Platform::Wechat,
        Platform::Zhihu,
        Platform::Toutiao,
        Platform::Csdn,
        Platform::Cnblogs,
        Platform::Jianshu,
        Platform::Juejin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Platform::Wechat => "微信公众号",
            Platform::Zhihu => "知乎",
            Platform::Toutiao => "今日头条",
            Platform::Csdn => "CSDN",
            Platform::Cnblogs => "博客园",
            Platform::Jianshu => "简书",
            Platform::Juejin => "掘金",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Platform::Wechat => "wechat",
            Platform::Zhihu => "zhihu",
            Platform::Toutiao => "toutiao",
            Platform::Csdn => "csdn",
            Platform::Cnblogs => "cnblogs",
            Platform::Jianshu => "jianshu",
            Platform::Juejin => "juejin",
        }
    }

    pub fn editor_url(self) -> &'static str {
        match self {
            Platform::Wechat => "https://mp.weixin.qq.com/",
            Platform::Zhihu => "https://zhuanlan.zhihu.com/write",
            Platform::Toutiao => "https://mp.toutiao.com/profile_v4/graphic/publish",
            Platform::Csdn => "https://mp.csdn.net/mp_blog/creation/editor",
            Platform::Cnblogs => "https://i.cnblogs.com/posts/edit",
            Platform::Jianshu => "https://www.jianshu.com/writer",
            Platform::Juejin => "https://juejin.cn/editor/drafts/new",
        }
    }

    pub fn uses_api(self) -> bool {
        matches!(self, Platform::Wechat | Platform::Cnblogs)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Targets {
    pub wechat: bool,
    pub zhihu: bool,
    pub toutiao: bool,
    pub csdn: bool,
    pub cnblogs: bool,
    pub jianshu: bool,
    pub juejin: bool,
}

impl Targets {
    pub fn selected(&self) -> Vec<Platform> {
        Platform::ALL
            .into_iter()
            .filter(|platform| self.is_selected(*platform))
            .collect()
    }

    pub fn is_selected(&self, platform: Platform) -> bool {
        match platform {
            Platform::Wechat => self.wechat,
            Platform::Zhihu => self.zhihu,
            Platform::Toutiao => self.toutiao,
            Platform::Csdn => self.csdn,
            Platform::Cnblogs => self.cnblogs,
            Platform::Jianshu => self.jianshu,
            Platform::Juejin => self.juejin,
        }
    }

    pub fn set(&mut self, platform: Platform, value: bool) {
        match platform {
            Platform::Wechat => self.wechat = value,
            Platform::Zhihu => self.zhihu = value,
            Platform::Toutiao => self.toutiao = value,
            Platform::Csdn => self.csdn = value,
            Platform::Cnblogs => self.cnblogs = value,
            Platform::Jianshu => self.jianshu = value,
            Platform::Juejin => self.juejin = value,
        }
    }

    pub fn any(&self) -> bool {
        !self.selected().is_empty()
    }
}

// ---------------------------------------------------------------------------
// Images

pub fn image_sources(markdown: &str) -> Vec<String> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    let mut rest = markdown;
    while let Some(start) = rest.find("![") {
        let after_alt = &rest[start + 2..];
        let Some(alt_end) = after_alt.find(']') else {
            break;
        };
        let after = &after_alt[alt_end + 1..];
        if !after.starts_with('(') {
            rest = after;
            continue;
        }
        let inside = &after[1..];
        let Some(end) = inside.find(')') else {
            break;
        };
        let mut source = inside[..end].trim();
        if source.starts_with('<') {
            if let Some(close) = source.find('>') {
                source = &source[1..close];
            }
        } else {
            source = source.split_whitespace().next().unwrap_or(source);
        }
        push_source(&mut sources, &mut seen, source);
        rest = &inside[end + 1..];
    }

    let lower = markdown.to_ascii_lowercase();
    let mut index = 0;
    while let Some(relative) = lower[index..].find("<img") {
        let start = index + relative;
        let tag = &markdown[start..];
        let tag_lower = tag.to_ascii_lowercase();
        if let Some(src_at) = tag_lower.find("src=") {
            let after = &tag[src_at + 4..];
            let quote = after.chars().next().unwrap_or('"');
            if quote == '"' || quote == '\'' {
                let after = &after[quote.len_utf8()..];
                if let Some(end) = after.find(quote) {
                    push_source(&mut sources, &mut seen, after[..end].trim());
                }
            } else {
                let end = after
                    .find(|ch: char| ch.is_whitespace() || ch == '>')
                    .unwrap_or(after.len());
                push_source(&mut sources, &mut seen, after[..end].trim());
            }
        }
        index = start + 4;
        if index >= markdown.len() {
            break;
        }
    }

    sources
}

fn push_source(sources: &mut Vec<String>, seen: &mut HashSet<String>, source: &str) {
    if source.is_empty() {
        return;
    }
    if seen.insert(source.to_string()) {
        sources.push(source.to_string());
    }
}

pub fn is_remote(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://") || source.starts_with("//")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        if bytes[ix] == b'%' && ix + 2 < bytes.len() {
            let hex = &value[ix + 1..ix + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                ix += 3;
                continue;
            }
        }
        out.push(bytes[ix]);
        ix += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn local_path(source: &str, doc_dir: &Path) -> Option<PathBuf> {
    if is_remote(source) || source.starts_with("data:") {
        return None;
    }
    let clean = source.split(['?', '#']).next().unwrap_or(source);
    let decoded = percent_decode(clean);
    if decoded.is_empty() {
        return None;
    }
    let path = PathBuf::from(decoded);
    Some(if path.is_absolute() {
        path
    } else {
        doc_dir.join(path)
    })
}

pub struct UploadResult {
    pub markdown: String,
    pub uploaded: usize,
    pub warnings: Vec<String>,
}

/// Upload every local image referenced by the Markdown and rewrite the source.
pub fn upload_local_images(
    markdown: &str,
    doc_dir: &Path,
    host: &ImageHostConfig,
) -> UploadResult {
    let mut result = UploadResult {
        markdown: markdown.to_string(),
        uploaded: 0,
        warnings: Vec::new(),
    };

    let sources = image_sources(markdown);
    if sources.is_empty() {
        return result;
    }

    for source in sources {
        let Some(path) = local_path(&source, doc_dir) else {
            continue;
        };
        if host.kind == ImageHostKind::None {
            result.warnings.push(format!(
                "未配置图床，本地图片 {} 不会被上传",
                path.display()
            ));
            continue;
        }
        match imagebed::upload_file(host, &path) {
            Ok(url) => {
                result.markdown = result.markdown.replace(&source, &url);
                result.uploaded += 1;
            }
            Err(error) => {
                result
                    .warnings
                    .push(format!("图片 {} 上传失败：{error}", path.display()));
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// HTML conversion

const P: &str = "margin:0 0 20px;font-size:16px;line-height:1.85;color:#3f3f3f;letter-spacing:0.5px;";
const P_IN_LIST: &str = "margin:0 0 6px;font-size:16px;line-height:1.8;color:#3f3f3f;";
const LINK: &str = "color:#576b95;text-decoration:none;";
const INLINE_CODE: &str = "padding:2px 6px;border-radius:4px;background:#f2f3f5;color:#c7254e;font-family:Consolas,'Courier New',monospace;font-size:14px;";
const PRE: &str = "margin:22px 0;padding:18px 20px;background:#1f2328;color:#e6e1d8;border-radius:10px;font-size:13px;line-height:1.65;font-family:Consolas,'Courier New',monospace;white-space:pre-wrap;word-break:break-word;";
const LI: &str = "margin:6px 0;font-size:16px;line-height:1.8;color:#3f3f3f;";

fn heading_style(level: usize) -> String {
    match level {
        1 => "margin:10px 0 22px;font-size:26px;line-height:1.35;font-weight:700;color:#1f2328;".to_string(),
        2 => "margin:36px 0 18px;padding-left:12px;border-left:4px solid #b4552d;font-size:21px;line-height:1.4;font-weight:700;color:#1f2328;".to_string(),
        3 => "margin:28px 0 14px;font-size:18px;line-height:1.45;font-weight:600;color:#1f2328;".to_string(),
        4 => "margin:24px 0 12px;font-size:16px;line-height:1.5;font-weight:600;color:#1f2328;".to_string(),
        _ => "margin:20px 0 10px;font-size:15px;line-height:1.5;font-weight:600;color:#1f2328;".to_string(),
    }
}

pub fn render_inline_html(
    markdown: &str,
    image_map: &HashMap<String, String>,
    footnote_links: bool,
) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let mut out = String::with_capacity(markdown.len() * 2);
    let mut links: Vec<(String, String)> = Vec::new();
    let mut link_stack: Vec<(String, String)> = Vec::new();
    let mut in_image = false;
    let mut in_code_block = false;
    let mut code_text = String::new();
    let mut in_li = false;
    let mut in_thead = false;
    let mut table_aligns: Option<Vec<Alignment>> = None;
    let mut cell_index = 0usize;

    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    if in_li {
                        out.push_str(&format!("<p style=\"{P_IN_LIST}\">"));
                    } else {
                        out.push_str(&format!("<p style=\"{P}\">"));
                    }
                }
                Tag::Heading { level, .. } => {
                    let level = level as usize;
                    out.push_str(&format!("<h{level} style=\"{}\">", heading_style(level)));
                }
                Tag::BlockQuote(_) => out.push_str(
                    "<section style=\"margin:22px 0;padding:14px 18px;background:#f8f4ee;border-left:4px solid #c47a52;border-radius:0 8px 8px 0;color:#6b6257;font-size:15px;line-height:1.8;\">",
                ),
                Tag::CodeBlock(kind) => {
                    in_code_block = true;
                    code_text.clear();
                    let _ = kind;
                }
                Tag::List(start) => {
                    if start.is_some() {
                        out.push_str("<ol style=\"margin:0 0 20px;padding-left:24px;\">");
                    } else {
                        out.push_str("<ul style=\"margin:0 0 20px;padding-left:24px;list-style-type:disc;\">");
                    }
                }
                Tag::Item => {
                    in_li = true;
                    out.push_str(&format!("<li style=\"{LI}\">"));
                }
                Tag::Emphasis => out.push_str("<em>"),
                Tag::Strong => out.push_str("<strong style=\"font-weight:600;color:#1f2328;\">"),
                Tag::Strikethrough => out.push_str("<del>"),
                Tag::Link { dest_url, .. } => {
                    let url = dest_url.to_string();
                    out.push_str(&format!(
                        "<a href=\"{}\" style=\"{LINK}\">",
                        escape_attr(&url)
                    ));
                    link_stack.push((url, String::new()));
                }
                Tag::Image { dest_url, .. } => {
                    let source = dest_url.to_string();
                    let source = image_map.get(&source).cloned().unwrap_or(source);
                    out.push_str(&format!(
                        "<section style=\"text-align:center;margin:26px 0;\"><img src=\"{}\" alt=\"\" style=\"max-width:100%;height:auto;border-radius:8px;display:block;margin:0 auto;\" /></section>",
                        escape_attr(&source)
                    ));
                    in_image = true;
                }
                Tag::Table(aligns) => {
                    table_aligns = Some(aligns);
                    out.push_str("<table style=\"width:100%;border-collapse:collapse;margin:22px 0;font-size:14px;line-height:1.6;\">");
                }
                Tag::TableHead => {
                    in_thead = true;
                    cell_index = 0;
                    out.push_str("<thead><tr>");
                }
                Tag::TableRow => {
                    cell_index = 0;
                    out.push_str("<tr>");
                }
                Tag::TableCell => {
                    let align = table_aligns
                        .as_ref()
                        .and_then(|aligns| aligns.get(cell_index))
                        .copied()
                        .unwrap_or(Alignment::None);
                    let text_align = match align {
                        Alignment::Left => "left",
                        Alignment::Center => "center",
                        Alignment::Right => "right",
                        Alignment::None => "left",
                    };
                    cell_index += 1;
                    let tag = if in_thead { "th" } else { "td" };
                    let extra = if in_thead {
                        "background:#f2ece2;font-weight:600;color:#3a342b;"
                    } else {
                        "background:#fffdf9;color:#3f3f3f;"
                    };
                    out.push_str(&format!(
                        "<{tag} style=\"border:1px solid #e2d9cb;padding:9px 12px;text-align:{text_align};{extra}\">"
                    ));
                }
                Tag::FootnoteDefinition(name) => {
                    out.push_str(&format!(
                        "<p style=\"font-size:13px;color:#8b8378;\">[^{}]</p>",
                        escape_html(&name)
                    ));
                }
                Tag::Superscript => out.push_str("<sup>"),
                Tag::Subscript => out.push_str("<sub>"),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => out.push_str("</p>"),
                TagEnd::Heading(level) => {
                    out.push_str(&format!("</h{}>", level as usize));
                }
                TagEnd::BlockQuote(_) => out.push_str("</section>"),
                TagEnd::CodeBlock => {
                    out.push_str(&format!(
                        "<pre style=\"{PRE}\"><code>{}</code></pre>",
                        escape_html(&code_text)
                    ));
                    in_code_block = false;
                    code_text.clear();
                }
                TagEnd::List(ordered) => {
                    out.push_str(if ordered { "</ol>" } else { "</ul>" });
                }
                TagEnd::Item => {
                    in_li = false;
                    out.push_str("</li>");
                }
                TagEnd::Emphasis => out.push_str("</em>"),
                TagEnd::Strong => out.push_str("</strong>"),
                TagEnd::Strikethrough => out.push_str("</del>"),
                TagEnd::Link => {
                    out.push_str("</a>");
                    if let Some((url, text)) = link_stack.pop() {
                        links.push((text, url));
                    }
                }
                TagEnd::Image => {
                    in_image = false;
                }
                TagEnd::Table => {
                    out.push_str("</tbody></table>");
                    table_aligns = None;
                }
                TagEnd::TableHead => {
                    out.push_str("</tr></thead><tbody>");
                    in_thead = false;
                }
                TagEnd::TableRow => out.push_str("</tr>"),
                TagEnd::TableCell => {
                    out.push_str(if in_thead { "</th>" } else { "</td>" });
                }
                TagEnd::FootnoteDefinition => {}
                TagEnd::Superscript => out.push_str("</sup>"),
                TagEnd::Subscript => out.push_str("</sub>"),
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    code_text.push_str(&text);
                } else if in_image {
                    // Alt text is not rendered: the image is the content.
                } else {
                    if let Some((_, link_text)) = link_stack.last_mut() {
                        link_text.push_str(&text);
                    }
                    out.push_str(&escape_html(&text));
                }
            }
            Event::Code(code) => {
                if let Some((_, link_text)) = link_stack.last_mut() {
                    link_text.push_str(&code);
                }
                out.push_str(&format!(
                    "<code style=\"{INLINE_CODE}\">{}</code>",
                    escape_html(&code)
                ));
            }
            Event::SoftBreak => {
                if in_code_block {
                    code_text.push('\n');
                } else {
                    out.push('\n');
                }
            }
            Event::HardBreak => out.push_str("<br>"),
            Event::Rule => out.push_str(
                "<hr style=\"border:none;border-top:1px solid #e5e0d8;margin:32px 0;\" />",
            ),
            Event::TaskListMarker(checked) => {
                out.push_str(if checked {
                    "<span style=\"color:#4f8a5f;font-weight:600;\">☑ </span>"
                } else {
                    "<span style=\"color:#b0a89b;\">☐ </span>"
                });
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if !in_image {
                    out.push_str(&html);
                }
            }
            Event::FootnoteReference(name) => {
                out.push_str(&format!(
                    "<sup style=\"color:#576b95;\">[^{}]</sup>",
                    escape_html(&name)
                ));
            }
            _ => {}
        }
    }

    if footnote_links {
        let mut seen = HashSet::new();
        let references: Vec<_> = links
            .into_iter()
            .filter(|(_, url)| {
                !url.is_empty()
                    && !url.starts_with('#')
                    && !url.starts_with("javascript:")
                    && seen.insert(url.clone())
            })
            .collect();
        if !references.is_empty() {
            out.push_str(
                "<section style=\"margin-top:38px;padding-top:18px;border-top:1px dashed #ddd;\"><p style=\"margin:0 0 10px;font-size:13px;color:#9a9389;letter-spacing:1px;\">REFERENCE</p>",
            );
            for (index, (text, url)) in references.iter().enumerate() {
                let text = if text.trim().is_empty() {
                    url.as_str()
                } else {
                    text.as_str()
                };
                out.push_str(&format!(
                    "<p style=\"margin:0 0 6px;font-size:13px;line-height:1.7;color:#8b8378;\">{} · {} <span style=\"color:#576b95;word-break:break-all;\">{}</span></p>",
                    index + 1,
                    escape_html(text),
                    escape_html(url)
                ));
            }
            out.push_str("</section>");
        }
    }

    out
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_attr(value: &str) -> String {
    escape_html(value).replace('\'', "&#39;")
}

pub fn excerpt(markdown: &str, limit: usize) -> String {
    let (_, markdown) = split_frontmatter(markdown);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let mut text = String::new();
    let mut in_image = false;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::Text(value) | Event::Code(value) if !in_image => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak => text.push(' '),
            Event::End(TagEnd::Paragraph) | Event::End(TagEnd::Heading(_)) => text.push('\n'),
            _ => {}
        }
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > limit {
        let mut short = collapsed.chars().take(limit).collect::<String>();
        short.push('…');
        short
    } else {
        collapsed
    }
}

// ---------------------------------------------------------------------------
// WeChat official account

static WECHAT_TOKEN: OnceLock<Mutex<Option<(String, String, Instant)>>> = OnceLock::new();

fn wechat_json(response: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    match response {
        Ok(response) => response
            .into_json::<Value>()
            .map_err(|error| format!("无法解析微信响应：{error}")),
        Err(ureq::Error::Status(code, response)) => {
            let text = response.into_string().unwrap_or_default();
            Err(format!("微信接口 HTTP {code}：{text}"))
        }
        Err(error) => Err(format!("微信请求失败：{error}")),
    }
}

fn check_wechat(value: &Value) -> Result<(), String> {
    if let Some(code) = value.get("errcode").and_then(Value::as_i64) {
        if code != 0 {
            let message = value
                .get("errmsg")
                .and_then(Value::as_str)
                .unwrap_or("未知错误");
            return Err(format!("微信接口错误 {code}：{message}"));
        }
    }
    Ok(())
}

fn wechat_token(app_id: &str, app_secret: &str) -> Result<String, String> {
    let cache = WECHAT_TOKEN.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((cached_app, token, expires)) = guard.as_ref() {
            if cached_app == app_id && Instant::now() < *expires {
                return Ok(token.clone());
            }
        }
    }

    let response = imagebed::agent()
        .get("https://api.weixin.qq.com/cgi-bin/token")
        .query("grant_type", "client_credential")
        .query("appid", app_id)
        .query("secret", app_secret)
        .call();
    let value = wechat_json(response)?;
    check_wechat(&value)?;
    let token = value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| "微信没有返回 access_token".to_string())?
        .to_string();
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(7200);
    let expires = Instant::now() + Duration::from_secs(expires_in.saturating_sub(120).max(60));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((app_id.to_string(), token.clone(), expires));
    }
    Ok(token)
}

pub fn wechat_test(app_id: &str, app_secret: &str) -> Result<(), String> {
    wechat_token(app_id, app_secret).map(|_| ())
}

fn wechat_upload(token: &str, bytes: &[u8], name: &str, permanent: bool) -> Result<String, String> {
    let (content_type, body) = imagebed::multipart("media", name, imagebed::mime_for(name), bytes);
    let (url, key) = if permanent {
        (
            "https://api.weixin.qq.com/cgi-bin/material/add_material",
            "media_id",
        )
    } else {
        (
            "https://api.weixin.qq.com/cgi-bin/media/uploadimg",
            "url",
        )
    };
    let mut request = imagebed::agent()
        .post(url)
        .query("access_token", token)
        .set("Content-Type", &content_type);
    if permanent {
        request = request.query("type", "image");
    }
    let value = wechat_json(request.send_bytes(&body))?;
    check_wechat(&value)?;
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .ok_or_else(|| format!("微信上传没有返回 {key}：{value}"))
}

fn load_image(source: &str, doc_dir: &Path) -> Result<(Vec<u8>, String), String> {
    if is_remote(source) {
        let url = if source.starts_with("//") {
            format!("https:{source}")
        } else {
            source.to_string()
        };
        let (bytes, mime) = imagebed::download(&url, 20 * 1024 * 1024)?;
        let name = url
            .split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit('/').next())
            .filter(|name| !name.is_empty())
            .unwrap_or("image.png");
        let name = ensure_extension(name, &mime);
        Ok((bytes, name))
    } else {
        let path = local_path(source, doc_dir).ok_or_else(|| "无法解析图片路径".to_string())?;
        let bytes = std::fs::read(&path).map_err(|error| format!("读取图片失败：{error}"))?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("image.png")
            .to_string();
        Ok((bytes, name))
    }
}

fn ensure_extension(name: &str, mime: &str) -> String {
    if name.contains('.') {
        return name.to_string();
    }
    let extension = match mime {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    };
    format!("{name}.{extension}")
}

pub struct WechatOutcome {
    pub media_id: String,
    pub warnings: Vec<String>,
}

pub fn wechat_publish(
    config: &WechatConfig,
    title: &str,
    markdown: &str,
    doc_dir: &Path,
) -> Result<WechatOutcome, String> {
    if config.app_id.trim().is_empty() || config.app_secret.trim().is_empty() {
        return Err("还没有填写公众号 AppID / AppSecret".to_string());
    }
    let markdown = split_frontmatter(markdown).1;
    let token = wechat_token(config.app_id.trim(), config.app_secret.trim())?;
    let mut warnings = Vec::new();
    let mut image_map = HashMap::new();

    for source in image_sources(markdown).into_iter().take(50) {
        match load_image(&source, doc_dir) {
            Ok((bytes, name)) => match wechat_upload(&token, &bytes, &name, false) {
                Ok(url) => {
                    image_map.insert(source, url);
                }
                Err(error) => warnings.push(format!("微信图片上传失败：{error}")),
            },
            Err(error) => warnings.push(format!("图片读取失败：{error}")),
        }
    }

    let cover_source = image_sources(markdown)
        .into_iter()
        .next()
        .or_else(|| {
            let path = config.default_cover.trim();
            (!path.is_empty()).then(|| path.to_string())
        });
    let Some(cover_source) = cover_source else {
        return Err(
            "公众号草稿需要封面图。请在文章里插入一张图片，或在设置里填写默认封面路径。"
                .to_string(),
        );
    };
    let (cover_bytes, cover_name) = load_image(&cover_source, doc_dir)
        .map_err(|error| format!("封面图读取失败：{error}"))?;
    let thumb_media_id = wechat_upload(&token, &cover_bytes, &cover_name, true)?;

    let content = render_inline_html(markdown, &image_map, config.footnote_links);
    let digest = excerpt(markdown, 110);
    // WeChat caps the article title at 64 characters.
    let title: String = title.chars().take(64).collect();
    let payload = json!({
        "articles": [{
            "title": title,
            "author": config.author,
            "digest": digest,
            "content": content,
            "content_source_url": "",
            "thumb_media_id": thumb_media_id,
            "need_open_comment": 1,
            "only_fans_can_comment": 0
        }]
    });

    let response = imagebed::agent()
        .post("https://api.weixin.qq.com/cgi-bin/draft/add")
        .query("access_token", &token)
        .set("Content-Type", "application/json; charset=utf-8")
        .send_bytes(serde_json::to_string(&payload).unwrap_or_default().as_bytes());
    let value = wechat_json(response)?;
    check_wechat(&value)?;
    let media_id = value
        .get("media_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("微信没有返回草稿 media_id：{value}"))?
        .to_string();

    Ok(WechatOutcome {
        media_id,
        warnings,
    })
}

// ---------------------------------------------------------------------------
// cnblogs MetaWeblog

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn cnblogs_publish(
    config: &CnblogsConfig,
    title: &str,
    html: &str,
) -> Result<String, String> {
    if config.username.trim().is_empty() || config.token.trim().is_empty() {
        return Err("还没有填写博客园用户名 / MetaWeblog 令牌".to_string());
    }
    let user = config.username.trim();
    let blog_id = if config.blog_id.trim().is_empty() {
        user
    } else {
        config.blog_id.trim()
    };
    let url = format!("https://rpc.cnblogs.com/metaweblog/{}", user);
    let body = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<methodCall>
  <methodName>metaWeblog.newPost</methodName>
  <params>
    <param><value><string>{}</string></value></param>
    <param><value><string>{}</string></value></param>
    <param><value><string>{}</string></value></param>
    <param><value><struct>
      <member><name>title</name><value><string>{}</string></value></member>
      <member><name>description</name><value><string>{}</string></value></member>
    </struct></value></param>
    <param><value><boolean>1</boolean></value></param>
  </params>
</methodCall>"#,
        xml_escape(blog_id),
        xml_escape(user),
        xml_escape(config.token.trim()),
        xml_escape(title),
        xml_escape(html)
    );

    let response = imagebed::agent()
        .post(&url)
        .set("Content-Type", "text/xml; charset=utf-8")
        .send_string(&body);
    let text = match response {
        Ok(response) => response.into_string().unwrap_or_default(),
        Err(ureq::Error::Status(code, response)) => {
            let text = response.into_string().unwrap_or_default();
            return Err(format!("博客园接口 HTTP {code}：{text}"));
        }
        Err(error) => return Err(format!("博客园请求失败：{error}")),
    };

    if let Some(fault_at) = text.find("<fault>") {
        let fault = &text[fault_at..];
        let message = extract_xml_string(fault).unwrap_or_else(|| "未知错误".to_string());
        return Err(format!("博客园返回错误：{message}"));
    }
    extract_xml_string(&text)
        .map(|id| format!("已发布，文章 ID {id}"))
        .ok_or_else(|| format!("博客园响应无法识别：{text}"))
}

fn extract_xml_string(xml: &str) -> Option<String> {
    let start = xml.find("<string>")? + "<string>".len();
    let end = xml[start..].find("</string>")? + start;
    Some(
        xml[start..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

// ---------------------------------------------------------------------------
// Clipboard

pub fn copy_rich_html(html: &str, plain: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|error| format!("打开剪贴板失败：{error}"))?;
    clipboard
        .set_html(html.to_string(), Some(plain.to_string()))
        .map_err(|error| format!("写入剪贴板失败：{error}"))
}

// ---------------------------------------------------------------------------
// Orchestration

pub struct PublishRequest {
    pub title: String,
    pub source: String,
    pub doc_dir: PathBuf,
    pub image_host: ImageHostConfig,
    pub wechat: WechatConfig,
    pub cnblogs: CnblogsConfig,
    pub targets: Targets,
    pub upload_images: bool,
}

pub struct PublishOutcome {
    pub log: Vec<String>,
    pub warnings: Vec<String>,
    pub updated_markdown: Option<String>,
    pub clipboard_html: Option<String>,
    pub plain_text: String,
    pub open_urls: Vec<String>,
    pub uploaded_images: usize,
}

pub fn publish(request: PublishRequest) -> PublishOutcome {
    let mut log = Vec::new();
    let mut warnings = Vec::new();
    let mut open_urls = Vec::new();
    let mut source = request.source;
    let mut uploaded_images = 0;
    let mut updated = false;

    if request.upload_images {
        let result = upload_local_images(&source, &request.doc_dir, &request.image_host);
        uploaded_images = result.uploaded;
        warnings.extend(result.warnings);
        if result.markdown != source {
            source = result.markdown;
            updated = true;
            log.push(format!("已上传 {uploaded_images} 张图片到图床并更新 Markdown"));
        }
    }

    let selected = request.targets.selected();
    let (_, body) = split_frontmatter(&source);
    let generic_html = render_inline_html(body, &HashMap::new(), false);
    let wechat_manual = request.targets.wechat
        && (request.wechat.app_id.trim().is_empty()
            || request.wechat.app_secret.trim().is_empty());
    let clipboard_html = if wechat_manual {
        Some(render_inline_html(
            body,
            &HashMap::new(),
            request.wechat.footnote_links,
        ))
    } else {
        Some(generic_html.clone())
    };
    let mut needs_clipboard = false;

    for platform in selected {
        match platform {
            Platform::Wechat => {
                if wechat_manual {
                    needs_clipboard = true;
                    open_urls.push(platform.editor_url().to_string());
                    warnings.push(
                        "未配置公众号 AppID / AppSecret，已改为复制公众号排版并打开发布页"
                            .to_string(),
                    );
                } else {
                    match wechat_publish(&request.wechat, &request.title, &source, &request.doc_dir)
                    {
                        Ok(outcome) => {
                            log.push(format!("公众号草稿已创建：{}", outcome.media_id));
                            warnings.extend(outcome.warnings);
                            open_urls.push("https://mp.weixin.qq.com/".to_string());
                        }
                        Err(error) => warnings.push(format!("公众号发布失败：{error}")),
                    }
                }
            }
            Platform::Cnblogs => {
                if request.cnblogs.username.trim().is_empty()
                    || request.cnblogs.token.trim().is_empty()
                {
                    needs_clipboard = true;
                    open_urls.push(platform.editor_url().to_string());
                    warnings.push(
                        "未配置博客园 MetaWeblog 令牌，已改为复制排版并打开发布页".to_string(),
                    );
                } else {
                    match cnblogs_publish(&request.cnblogs, &request.title, &generic_html) {
                        Ok(message) => log.push(format!("博客园：{message}")),
                        Err(error) => warnings.push(format!("博客园发布失败：{error}")),
                    }
                }
            }
            _ => {
                needs_clipboard = true;
                open_urls.push(platform.editor_url().to_string());
                log.push(format!("{}：已打开发布页", platform.label()));
            }
        }
    }

    PublishOutcome {
        log,
        warnings,
        updated_markdown: updated.then_some(source.clone()),
        clipboard_html: needs_clipboard.then_some(clipboard_html.unwrap_or_else(|| generic_html.clone())),
        plain_text: source,
        open_urls,
        uploaded_images,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_markdown_and_html_images_once() {
        let source = "![a](images/a.png)\n<img src=\"images/b.png\" />\n![b](images/a.png)";
        assert_eq!(
            image_sources(source),
            vec![
                "images/a.png".to_string(),
                "images/b.png".to_string()
            ]
        );
    }

    #[test]
    fn resolves_relative_local_paths() {
        let base = Path::new("C:/notes");
        let path = local_path("images/a%20b.png", base).unwrap();
        assert!(path.ends_with("images/a b.png"));
        assert!(local_path("https://example.com/a.png", base).is_none());
    }

    #[test]
    fn renders_inline_styled_article() {
        let markdown = "# 标题\n\n正文 **加粗** 与 `code`。\n\n> 引用\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n[链接](https://example.com)\n\n![图](https://example.com/a.png)\n";
        let html = render_inline_html(markdown, &HashMap::new(), true);
        assert!(html.contains("<h1 style="));
        assert!(html.contains("<strong style="));
        assert!(html.contains("<pre style="));
        assert!(html.contains("<table style="));
        assert!(html.contains("img src=\"https://example.com/a.png\""));
        assert!(html.contains("REFERENCE"));
        assert!(html.contains("https://example.com"));
    }


    #[test]
    fn publish_html_ignores_frontmatter() {
        let mut targets = Targets::default();
        targets.zhihu = true;
        let outcome = publish(PublishRequest {
            title: "t".to_string(),
            source: "---\ntitle: 逆向\n---\n# 正文\n".to_string(),
            doc_dir: PathBuf::from("."),
            image_host: ImageHostConfig::default(),
            wechat: WechatConfig::default(),
            cnblogs: CnblogsConfig::default(),
            targets,
            upload_images: false,
        });
        let html = outcome.clipboard_html.expect("clipboard html");
        assert!(!html.contains("title: 逆向"));
        assert!(html.contains("正文"));
    }

    #[test]
    fn excerpt_ignores_markdown_noise() {
        let text = "# 标题\n\n这是 **正文**，带一个[链接](https://example.com)和图片 ![alt](x.png)。";
        let excerpt = excerpt(text, 40);
        assert!(excerpt.contains("正文"));
        assert!(!excerpt.contains("**"));
        assert!(!excerpt.contains("alt"));
    }
}
