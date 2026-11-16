//! Basic code validation of fenced blocks, offline or through an AI endpoint.
//!
//! Local mode runs a downloaded GGUF (the Qwen2.5-Coder ladder, Q4_K_M) via
//! llama.cpp on CPU; API mode posts each block to the OpenAI-compatible
//! endpoint from 设置 → AI 写作. Off by default — the settings switch gates
//! every entry point. Nothing is a black box: the model catalog, the hardware
//! behind the recommendation and the active model's identity are all shown.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaChatMessage;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use serde_json::Value;

use crate::config::{self, CheckSource};
use crate::doc;

/// Per-run limits: a basic check, not an audit — one block batch finishes in
/// well under a minute on CPU.
const MAX_BLOCKS: usize = 12;
const MAX_BLOCK_LINES: usize = 100;
const MAX_NEW_TOKENS: i32 = 384;
const CTX_SIZE: u32 = 4096;

const SYSTEM_PROMPT: &str = "You are a strict code reviewer for short snippets. Report ONLY concrete errors: an unbalanced ( ) [ ] { }, an unclosed or stray quote, a missing colon, an invalid token, an undefined name. Ignore style, naming and formatting. Output ONLY the JSON object (optionally inside a ```json fence), nothing else: {\"issues\":[{\"line\":<1-based line in the code>,\"message\":\"<short description>\"}]}. If the code parses cleanly output {\"issues\":[]}. When in doubt output {\"issues\":[]}.";

/// One downloadable local model. Every `repo`/`file` pair was verified on
/// both ModelScope and HuggingFace (same paths, different hosts).
#[derive(Clone, Copy, Debug)]
pub struct ModelEntry {
    /// Stable id stored in `CodeCheckConfig::local_model`.
    pub id: &'static str,
    pub display: &'static str,
    pub quant: &'static str,
    pub repo: &'static str,
    pub file: &'static str,
    /// Approximate download size, shown before downloading.
    pub size_hint: &'static str,
    /// Memory needed to run comfortably.
    pub ram_hint: &'static str,
    /// One-line positioning so users can pick knowingly.
    pub note: &'static str,
}

/// The model catalog. All Q4_K_M — small enough for CPU inference, big
/// enough that the checker actually catches things.
pub const MODELS: &[ModelEntry] = &[
    ModelEntry {
        id: "qwen2.5-coder-0.5b",
        display: "Qwen2.5-Coder-0.5B-Instruct",
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-Coder-0.5B-Instruct-GGUF",
        file: "Qwen2.5-Coder-0.5B-Instruct-Q4_K_M.gguf",
        size_hint: "约 380 MB",
        ram_hint: "需 4GB+ 内存",
        note: "入门最快，低配机器可用",
    },
    ModelEntry {
        id: "qwen2.5-coder-1.5b",
        display: "Qwen2.5-Coder-1.5B-Instruct",
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-Coder-1.5B-Instruct-GGUF",
        file: "Qwen2.5-Coder-1.5B-Instruct-Q4_K_M.gguf",
        size_hint: "约 990 MB",
        ram_hint: "需 8GB+ 内存",
        note: "速度与检出率均衡，多数机器首选",
    },
    ModelEntry {
        id: "qwen2.5-coder-3b",
        display: "Qwen2.5-Coder-3B-Instruct",
        quant: "Q4_K_M",
        repo: "bartowski/Qwen2.5-Coder-3B-Instruct-GGUF",
        file: "Qwen2.5-Coder-3B-Instruct-Q4_K_M.gguf",
        size_hint: "约 1.9 GB",
        ram_hint: "需 16GB+ 内存",
        note: "检出更细，CPU 推理也更慢",
    },
];

/// The entry for a stored id; unknown ids fall back to the first entry so an
/// old or edited config can never point at nothing.
pub fn model_by_id(id: &str) -> &'static ModelEntry {
    MODELS
        .iter()
        .find(|entry| entry.id == id)
        .unwrap_or(&MODELS[0])
}

/// One problem found inside a code block.
#[derive(Clone, Debug)]
pub struct CodeIssue {
    /// Byte offset of the block's opening fence — the key the preview chip
    /// matches `block.span.start` against.
    pub fence_start: usize,
    /// 1-based line in the document, as the modal shows it.
    pub line: usize,
    pub lang: String,
    pub message: String,
}

/// Optional override for where model files live; `None` means the default
/// `<data>/models`. Set once at startup and again whenever settings flush, so
/// the settings panel can show live download status without re-reading JSON.
static MODELS_DIR: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

pub fn set_models_dir(dir: Option<PathBuf>) {
    let slot = MODELS_DIR.get_or_init(|| RwLock::new(None));
    *slot.write().expect("models dir lock poisoned") =
        dir.filter(|path| !path.as_os_str().is_empty());
}

/// Where the selected model lives on disk: `<models>/<entry.file>`.
pub fn model_path(id: &str) -> PathBuf {
    model_dir().join(model_by_id(id).file)
}

fn model_dir() -> PathBuf {
    if let Some(dir) = MODELS_DIR
        .get()
        .and_then(|slot| slot.read().ok().map(|guard| guard.clone()))
        .flatten()
    {
        return dir;
    }
    config::data_dir()
        .unwrap_or_else(config::config_dir)
        .join("models")
}

pub fn is_downloaded(id: &str) -> bool {
    model_path(id).is_file()
}

/// Download URL for one catalog entry on the selected mirror; all three
/// hosts serve the same repository, so switching trades reachability
/// for speed.
pub fn model_url(source: CheckSource, entry: &ModelEntry) -> String {
    match source {
        CheckSource::ModelScope => format!(
            "https://www.modelscope.cn/api/v1/models/{repo}/repo?Revision=master&FilePath={file}",
            repo = entry.repo,
            file = entry.file
        ),
        CheckSource::HuggingFace => {
            format!(
                "https://huggingface.co/{repo}/resolve/main/{file}",
                repo = entry.repo,
                file = entry.file
            )
        }
        CheckSource::HfMirror => {
            format!(
                "https://hf-mirror.com/{repo}/resolve/main/{file}",
                repo = entry.repo,
                file = entry.file
            )
        }
    }
}

/// Byte counts a UI thread polls while [`download`] runs on a background one.
/// `total` starts at zero until the server's content-length arrives, so a
/// chunked response still reports a growing percentage.
#[derive(Clone, Default)]
pub struct DownloadProgress {
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
}

impl DownloadProgress {
    pub fn percent(&self) -> u64 {
        let total = self.total.load(Ordering::Relaxed).max(1);
        (self.done.load(Ordering::Relaxed) * 100 / total).min(100)
    }
}

/// Streams one catalog entry from `source` to disk. Writes to `.part` first
/// and renames on success, so an interrupted download never counts as
/// installed.
pub fn download(
    entry: &ModelEntry,
    source: CheckSource,
    progress: &DownloadProgress,
) -> Result<PathBuf, String> {
    let url = model_url(source, entry);
    let dir = model_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建模型目录失败：{error}"))?;
    let target = dir.join(entry.file);
    let part = dir.join(format!("{}.part", entry.file));

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(15))
        // 400MB on a slow domestic or international link.
        .timeout_read(std::time::Duration::from_secs(1800))
        .build();
    let response = agent
        .get(&url)
        .set("User-Agent", "Inkdown")
        .call()
        .map_err(|error| format!("下载失败：{error}"))?;

    if let Some(len) = response
        .header("content-length")
        .and_then(|v| v.parse().ok())
    {
        progress.total.store(len, Ordering::Relaxed);
    }
    let mut reader = response.into_reader();
    let mut file =
        std::fs::File::create(&part).map_err(|error| format!("创建临时文件失败：{error}"))?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer)
            .map_err(|error| format!("下载中断：{error}"))?;
        if read == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buffer[..read])
            .map_err(|error| format!("写入失败：{error}"))?;
        progress.done.fetch_add(read as u64, Ordering::Relaxed);
    }
    drop(file);
    let total = progress.total.load(Ordering::Relaxed);
    let done = progress.done.load(Ordering::Relaxed);
    if total > 0 && done < total {
        let _ = std::fs::remove_file(&part);
        return Err("下载不完整，请重试或换一个来源".to_string());
    }
    if !looks_like_model(&part) {
        let _ = std::fs::remove_file(&part);
        return Err("下载内容不是有效的模型文件，请重试或换一个来源".to_string());
    }
    std::fs::rename(&part, &target).map_err(|error| format!("保存模型失败：{error}"))?;
    Ok(target)
}

/// The one shared llama.cpp backend for the process.
fn backend() -> Result<&'static LlamaBackend, String> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    if let Some(backend) = BACKEND.get() {
        return Ok(backend);
    }
    let backend = LlamaBackend::init().map_err(|error| format!("初始化推理后端失败：{error:?}"))?;
    Ok(BACKEND.get_or_init(|| backend))
}

/// The loaded model; mmap means keeping it resident is cheap. Send+Sync, so
/// one load serves every check.
/// Loaded models keyed by their catalog file name — mmap keeps a second
/// model resident cheap, and checks are serialized by the UI anyway.
static MODELS_CACHE: OnceLock<RwLock<HashMap<&'static str, LlamaModel>>> = OnceLock::new();

/// Which engine runs a document check.
pub enum CheckTarget<'a> {
    /// The selected catalog model, offline through llama.cpp.
    Local { model_id: &'a str },
    /// The OpenAI-compatible endpoint from 设置 → AI 写作.
    Api { config: &'a crate::config::AiConfig },
}

/// Validates up to [`MAX_BLOCKS`] non-empty fenced blocks with the given
/// target, offline for `Local` and one request per block for `Api`.
///
/// Returns the number of blocks actually checked alongside the findings;
/// lines are lifted into document coordinates so the modal can show the line
/// the user searches for.
pub fn check_document(
    target: &CheckTarget<'_>,
    blocks: &[doc::CodeBlockSpan],
) -> Result<(usize, Vec<CodeIssue>), String> {
    match target {
        CheckTarget::Local { model_id } => check_local_document(model_id, blocks),
        CheckTarget::Api { config } => check_api_document(config, blocks),
    }
}

/// The head, the language and a non-empty check for one block; shared by
/// both engines so their limits stay identical.
fn block_setup<'a>(
    block: &'a doc::CodeBlockSpan,
    checked: usize,
) -> Option<(Vec<&'a str>, String, String)> {
    if checked >= MAX_BLOCKS || block.code.trim().is_empty() {
        return None;
    }
    // Truncate at line granularity so the model sees a whole head of the
    // block and never a multi-kilobyte tail.
    let lines: Vec<&str> = block.code.lines().take(MAX_BLOCK_LINES).collect();
    let lang = if block.lang.is_empty() {
        "text".to_string()
    } else {
        block.lang.clone()
    };
    let head = lines.join("\n");
    Some((lines, lang, head))
}

/// Block-relative findings → document lines, keyed to the block's fence.
fn lift_issues(
    block: &doc::CodeBlockSpan,
    line_count: usize,
    found: Vec<(usize, String)>,
) -> Vec<CodeIssue> {
    let lang = if block.lang.is_empty() {
        "text".to_string()
    } else {
        block.lang.clone()
    };
    found
        .into_iter()
        .map(|(line, message)| {
            let relative = line.min(line_count.max(1)).max(1);
            CodeIssue {
                fence_start: block.fence_start,
                line: (block.line + relative - 1).max(1),
                lang: lang.clone(),
                message,
            }
        })
        .collect()
}

/// One offline run: load the selected model once, reuse a single context
/// (a per-block context would re-allocate the KV cache for every block).
fn check_local_document(
    model_id: &str,
    blocks: &[doc::CodeBlockSpan],
) -> Result<(usize, Vec<CodeIssue>), String> {
    let entry = model_by_id(model_id);
    let path = model_dir().join(entry.file);
    if !path.is_file() {
        return Err(format!(
            "模型「{}」还没有下载，请先在 设置 → 代码校验 里下载",
            entry.display
        ));
    }
    let cache = MODELS_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    if !cache
        .read()
        .map(|guard| guard.contains_key(entry.file))
        .unwrap_or(false)
    {
        let backend = backend()?;
        let loaded = LlamaModel::load_from_file(backend, &path, &LlamaModelParams::default())
            .map_err(|error| format!("加载模型失败：{error:?}"))?;
        cache
            .write()
            .map_err(|error| format!("模型缓存不可用：{error}"))?
            .insert(entry.file, loaded);
    }
    let cache = cache
        .read()
        .map_err(|error| format!("模型缓存不可用：{error}"))?;
    let model = cache
        .get(entry.file)
        .ok_or_else(|| "模型加载失败".to_string())?;
    let vocab = model.vocab();
    let template = model
        .chat_template(None)
        .map_err(|error| format!("模型缺少 chat template：{error:?}"))?;
    let params = LlamaContextParams::default();
    let params = params.with_n_ctx(std::num::NonZeroU32::new(CTX_SIZE));
    let mut context = model
        .new_context(backend()?, params)
        .map_err(|error| format!("创建推理上下文失败：{error:?}"))?;

    let mut issues = Vec::new();
    let mut checked = 0usize;
    for block in blocks {
        let Some((lines, lang, head)) = block_setup(block, checked) else {
            if checked >= MAX_BLOCKS {
                break;
            }
            continue;
        };
        checked += 1;
        let reply = generate(model, &mut context, &vocab, &template, &lang, &head)
            .map_err(|error| format!("校验第 {checked} 块失败：{error}"))?;
        // generate() prefilled `{"issues":[`; a reply that already carries
        // the word "issues" emitted the whole object itself.
        let full = if reply.contains("issues") {
            reply.clone()
        } else {
            format!("{{\"issues\":[{reply}")
        };
        let found = filter_issues(&head, parse_issues(&full));
        issues.extend(lift_issues(block, lines.len(), found));
    }
    Ok((checked, issues))
}

/// One API run: a single chat request per block, same limits and filters.
fn check_api_document(
    config: &crate::config::AiConfig,
    blocks: &[doc::CodeBlockSpan],
) -> Result<(usize, Vec<CodeIssue>), String> {
    let mut issues = Vec::new();
    let mut checked = 0usize;
    for block in blocks {
        let Some((lines, lang, head)) = block_setup(block, checked) else {
            if checked >= MAX_BLOCKS {
                break;
            }
            continue;
        };
        checked += 1;
        let found = api_check(config, &lang, &head)
            .map_err(|error| format!("校验第 {checked} 块失败：{error}"))?;
        issues.extend(lift_issues(block, lines.len(), found));
    }
    Ok((checked, issues))
}

/// Checks one block through the configured AI endpoint.
pub fn api_check(
    config: &crate::config::AiConfig,
    lang: &str,
    code: &str,
) -> Result<Vec<(usize, String)>, String> {
    let turns = [crate::ai::ChatTurn {
        role: "user".to_string(),
        content: format!("检查这段 {lang} 代码：\n```{lang}\n{code}\n```"),
    }];
    let reply = crate::ai::chat(config, crate::ai::CHECK_SYSTEM_PROMPT, &turns)?;
    Ok(filter_issues(code, parse_issues(&reply)))
}

/// One offline completion: chat template → tokenize → decode → greedy loop
/// until EOG, the model's own bounds, or the token cap.
fn generate(
    model: &LlamaModel,
    context: &mut LlamaContext<'_>,
    vocab: &llama_cpp_2::vocab::LlamaVocab<'_>,
    template: &llama_cpp_2::model::LlamaChatTemplate,
    lang: &str,
    code: &str,
) -> Result<String, String> {
    // Every block is its own conversation: a KV cache left dirty by the
    // previous one makes the next prompt decode fail (NTokensZero).
    context.clear_kv_cache();
    // The code goes LAST: a 0.5B model heavily continues whatever ends the
    // prompt, so closing with the code (not with an empty-issues template)
    // pushes it to answer about the code instead of echoing the template.
    let prompt = format!(
        "Review this {lang} code for concrete errors: unbalanced brackets, bad quotes, \
        missing colons, undefined names. Reply with ONLY a JSON list shaped \
        {{\"issues\":[{{\"line\":1,\"message\":\"...\"}}]}}, or {{\"issues\":[]}} when the \
        code is clean.\n\n```{lang}\n{code}\n```"
    );
    let messages = [
        LlamaChatMessage::new("system".to_string(), SYSTEM_PROMPT.to_string())
            .map_err(|error| error.to_string())?,
        LlamaChatMessage::new("user".to_string(), prompt).map_err(|error| error.to_string())?,
    ];
    let mut rendered = model
        .apply_chat_template(template, &messages, true)
        .map_err(|error| format!("套用 chat template 失败：{error:?}"))?;
    // Prefill the JSON shape so the model must finish the object — that
    // forces the semantic choice (entries vs empty) instead of free prose.
    rendered.push_str("{\"issues\":[");
    let prompt_tokens = vocab.tokenize(rendered.as_bytes(), true, true);
    if prompt_tokens.is_empty() {
        return Err("提示词无法分词".to_string());
    }
    if prompt_tokens.len() as i32 >= CTX_SIZE as i32 - MAX_NEW_TOKENS {
        return Err("代码块太长，超出上下文窗口".to_string());
    }

    let mut batch = LlamaBatch::new(prompt_tokens.len() + 8, 1);
    for (index, token) in prompt_tokens.iter().enumerate() {
        let last = index + 1 == prompt_tokens.len();
        batch
            .add(*token, index as i32, &[0], last)
            .map_err(|error| error.to_string())?;
    }
    context
        .decode(&mut batch)
        .map_err(|error| format!("编码提示词失败：{error:?}"))?;

    let mut output = String::new();
    let mut pos = prompt_tokens.len() as i32;
    let mut logits_index = (prompt_tokens.len() - 1) as i32;
    let mut batch = LlamaBatch::new(1, 1);
    for _ in 0..MAX_NEW_TOKENS {
        let logits = context.get_logits_ith(logits_index);
        let Some(next) = sample_token(logits, TEMPERATURE) else {
            break;
        };
        let token = llama_cpp_2::token::LlamaToken(next);
        if vocab.is_eog(token) {
            break;
        }
        let piece = vocab.token_to_piece(token, false, None);
        output.push_str(&String::from_utf8_lossy(&piece));

        // decode() consumes the batch but does not reset it.
        batch.clear();
        batch
            .add(token, pos, &[0], true)
            .map_err(|error| error.to_string())?;
        context
            .decode(&mut batch)
            .map_err(|error| format!("解码失败：{error:?}"))?;
        pos += 1;
        // The reused batch only ever holds this one token, so its logits
        // always sit at index 0 after a decode.
        logits_index = 0;
    }
    Ok(output)
}

/// Sampling temperature: low enough to stay coherent, high enough that the
/// 0.5B checker surfaces errors greedy decoding glosses over.
const TEMPERATURE: f32 = 0.0;

/// Temperature-samples one token index from `logits`. Greedy decoding on a
/// 0.5B model consistently reports broken snippets as clean, so a little
/// entropy trades a rare false positive for actually catching real errors.
fn sample_token(logits: &[f32], temperature: f32) -> Option<i32> {
    let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    if temperature <= f32::EPSILON {
        // Greedy: pick the argmax (keeps T=0 runs deterministic).
        return logits
            .iter()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(right.1))
            .map(|(index, _)| index as i32);
    }
    if !max.is_finite() {
        return None;
    }
    let mut weights = Vec::with_capacity(logits.len());
    let mut total = 0f64;
    for logit in logits {
        let weight = (((logit - max) / temperature) as f64).exp();
        weights.push(weight);
        total += weight;
    }
    if !total.is_finite() || total <= 0.0 {
        return None;
    }
    // xorshift64* off the clock: one random draw without an RNG dependency.
    let mut seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_nanos() as u64)
        .unwrap_or(0x2545_F491_4F6C_DD1D)
        | 1;
    seed ^= seed << 13;
    seed ^= seed >> 7;
    seed ^= seed << 17;
    let draw = ((seed >> 11) as f64) / ((u64::MAX >> 11) as f64) * total;
    let mut cumulative = 0f64;
    for (index, weight) in weights.iter().enumerate() {
        cumulative += *weight;
        if draw <= cumulative {
            return Some(index as i32);
        }
    }
    Some((weights.len() - 1) as i32)
}

/// Pulls `{"issues":[…]}` out of a model reply. Tolerates surrounding prose,
/// ```` ```json ```` fences, the JSON object our prefill opened, and the bare
/// keys (`line:1`) a 0.5B model emits; anything hopeless becomes empty, and
/// the caller decides how to report that.
pub fn parse_issues(text: &str) -> Vec<(usize, String)> {
    let cleaned = crate::ai::clean_markdown(text);
    // Runaway generations (repetition loops) carry no usable signal — mining
    // them for `line`/`message` would invent false positives.
    if cleaned.chars().count() > 600 {
        return Vec::new();
    }
    let normalized = quote_bare_keys(&cleaned);
    let Some(start) = normalized.find('{') else {
        return Vec::new();
    };
    let Some(end) = normalized.rfind('}') else {
        return Vec::new();
    };
    if start > end {
        return Vec::new();
    }
    let slice = &normalized[start..=end];
    let Ok(value) = serde_json::from_str::<Value>(slice) else {
        // Not strict JSON: fall back to a tolerant line/message scan.
        return parse_lenient(slice);
    };
    let Some(items) = value.get("issues").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let line = item.get("line").and_then(Value::as_u64)? as usize;
            let message = item.get("message").and_then(Value::as_str)?.trim();
            if message.is_empty() {
                return None;
            }
            Some((line.max(1), message.to_string()))
        })
        .collect()
}

/// Quotes bare `line:` / `message:` / `issues:` keys so serde can read them.
/// Already-quoted keys are parked first so they do not double-quote.
fn quote_bare_keys(text: &str) -> String {
    let parked = text
        .replace("\"issues\":", "\u{0}i\u{0}")
        .replace("\"line\":", "\u{0}l\u{0}")
        .replace("\"message\":", "\u{0}m\u{0}");
    parked
        .replace("issues:", "\"issues\":")
        .replace("line:", "\"line\":")
        .replace("message:", "\"message\":")
        .replace("\u{0}i\u{0}", "\"issues\":")
        .replace("\u{0}l\u{0}", "\"line\":")
        .replace("\u{0}m\u{0}", "\"message\":")
}

/// Last-resort scan for `<line…>: <message>` pairs in replies serde rejects.
fn parse_lenient(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = text[cursor..].find("line") {
        let pos = cursor + 4 + relative;
        cursor = pos;
        let tail = &text[pos..];
        let Some(skip) = tail.find(|c: char| c.is_ascii_digit()) else {
            continue;
        };
        let digits: String = tail[skip..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let Ok(line) = digits.parse::<usize>() else {
            continue;
        };
        let after_line = &tail[skip + digits.len()..];
        let Some(message_at) = after_line.find("message") else {
            continue;
        };
        let area =
            after_line[message_at + 7..].trim_start_matches(|c| c == '"' || c == ':' || c == ' ');
        let value = if let Some(quoted) = area.strip_prefix('"') {
            quoted.split('"').next().unwrap_or("")
        } else {
            area.split([',', '}']).next().unwrap_or("")
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        out.push((line.max(1), value.to_string()));
    }
    out
}

/// Drops delimiter claims the cheap balance check can refute — the local
/// model names "unbalanced brackets" first and reports it even on perfectly
/// balanced code. Shared by the local and API check paths.
pub fn filter_issues(code_head: &str, issues: Vec<(usize, String)>) -> Vec<(usize, String)> {
    issues
        .into_iter()
        .filter(|(_, message)| !(mentions_delimiters(message) && !naive_imbalance(code_head)))
        .collect()
}

/// Whether a finding claims a delimiter problem — the class the model names
/// first and therefore hallucinates most often.
fn mentions_delimiters(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    [
        "bracket",
        "paren",
        "brace",
        "unbalanced",
        "unclosed",
        "mismatch",
        "括号",
        "括弧",
        "不匹配",
        "未闭合",
    ]
    .iter()
    .any(|word| message.contains(word))
}

/// Rough () [] {} balance, deliberately ignoring strings and comments: it
/// only needs to refute a confident delimiter claim, not parse the code.
fn naive_imbalance(code: &str) -> bool {
    let (mut round, mut square, mut curly) = (0i32, 0i32, 0i32);
    for ch in code.chars() {
        match ch {
            '(' => round += 1,
            ')' => round -= 1,
            '[' => square += 1,
            ']' => square -= 1,
            '{' => curly += 1,
            '}' => curly -= 1,
            _ => {}
        }
        if round < 0 || square < 0 || curly < 0 {
            return true;
        }
    }
    round != 0 || square != 0 || curly != 0
}

/// What the checker runs on, shown so the recommendation is never a black
/// box: detected hardware next to the backends this build can really use.
#[derive(Clone, Debug, Default)]
pub struct Hardware {
    pub cpu: String,
    pub cores: Option<usize>,
    pub ram_gb: Option<u64>,
    pub gpu: Option<String>,
    pub vram_gb: Option<u64>,
    /// Compute backends llama.cpp exposes in this build (CPU unless a GPU
    /// backend feature was compiled in).
    pub backends: Vec<String>,
}

static HARDWARE: OnceLock<RwLock<Option<Hardware>>> = OnceLock::new();

/// Cached result; `None` until [`detect_hardware`] has run (the settings
/// panel shows 检测中… and kicks it off once).
pub fn hardware() -> Option<Hardware> {
    HARDWARE
        .get()
        .and_then(|slot| slot.read().ok().map(|guard| guard.clone()))
        .flatten()
}

/// Detect hardware once per process and cache it. Blocking (WMI round-trip)
/// — call it from a background task, then read [`hardware`] for rendering.
pub fn detect_hardware() -> Hardware {
    if let Some(cached) = hardware() {
        return cached;
    }
    let mut hardware = probe_hardware();
    if hardware.backends.is_empty() {
        hardware.backends = compute_backends();
    }
    if hardware.cores.is_none() {
        hardware.cores = std::thread::available_parallelism().ok().map(|n| n.get());
    }
    let slot = HARDWARE.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = slot.write() {
        *guard = Some(hardware.clone());
    }
    hardware
}

/// One WMI round-trip on Windows; the fixed `KEY:value` line format keeps the
/// parser testable and the fallback honest on other platforms.
#[cfg(windows)]
fn probe_hardware() -> Hardware {
    let script = concat!(
        "$ErrorActionPreference='SilentlyContinue';",
        "$c=Get-CimInstance Win32_Processor | Select-Object -First 1;",
        "'CPU:'+$c.Name;",
        "'CORES:'+$c.NumberOfCores;",
        "'RAM:'+(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory;",
        // AdapterRAM is a truncated uint32; the driver registry value is the
        // truthful 64-bit size (an 8 GB card otherwise reports ~4 GB - 1 MB).
        "Get-CimInstance Win32_VideoController|%{ $n=$_.Name; $v=$_.AdapterRAM; ",
        "foreach($k in Get-ChildItem 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}'){",
        "$p=Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue;",
        "if($p.DriverDesc -eq $n){ $q=$p.'HardwareInformation.qwMemorySize';",
        "if($q -is [byte[]]){ $v=[BitConverter]::ToUInt64($q,0) } elseif($q){ $v=[int64]$q } } };",
        "'GPU:'+$n+'|'+$v }",
    );
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output();
    match output {
        Ok(out) => parse_hardware(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => Hardware::default(),
    }
}

#[cfg(not(windows))]
fn probe_hardware() -> Hardware {
    Hardware::default()
}

/// Parse the fixed `KEY:value` lines [`probe_hardware`] prints.
pub fn parse_hardware(stdout: &str) -> Hardware {
    let mut hardware = Hardware::default();
    for line in stdout.lines() {
        if let Some(value) = line.strip_prefix("CPU:") {
            if !value.trim().is_empty() {
                hardware.cpu = value.trim().to_string();
            }
        } else if let Some(value) = line.strip_prefix("CORES:") {
            hardware.cores = value.trim().parse().ok();
        } else if let Some(value) = line.strip_prefix("RAM:") {
            hardware.ram_gb = value
                .trim()
                .parse::<u64>()
                .ok()
                .map(|bytes| bytes / (1024 * 1024 * 1024));
        } else if let Some(value) = line.strip_prefix("GPU:") {
            let (name, vram) = value.split_once('|').unwrap_or((value, ""));
            if !name.trim().is_empty() {
                hardware.gpu = Some(name.trim().to_string());
                hardware.vram_gb = vram
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .map(|bytes| bytes / (1024 * 1024 * 1024));
            }
        }
    }
    hardware
}

/// The backends llama.cpp exposes after the backend is initialized.
fn compute_backends() -> Vec<String> {
    if backend().is_err() {
        return Vec::new();
    }
    let mut labels: Vec<String> = Vec::new();
    for device in llama_cpp_2::list_llama_ggml_backend_devices() {
        let label = if device.description.is_empty() || device.description == device.backend {
            device.backend.clone()
        } else {
            format!("{} · {}", device.backend, device.description)
        };
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
}

/// Pick the biggest catalog model that fits the machine's memory; [`MODELS`]
/// is ordered small to large, so the first fit wins.
pub fn recommend(hardware: &Hardware) -> &'static ModelEntry {
    match hardware.ram_gb {
        Some(ram) if ram >= 16 => model_by_id("qwen2.5-coder-3b"),
        Some(ram) if ram >= 8 => model_by_id("qwen2.5-coder-1.5b"),
        _ => model_by_id("qwen2.5-coder-0.5b"),
    }
}

/// Whether `path` looks like a model file of the expected minimum size —
/// guards against a zero-byte or HTML error page saved as the model.
pub fn looks_like_model(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|meta| meta.len() > 100 * 1024 * 1024)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CheckSource;

    #[test]
    fn model_urls_cover_both_regions() {
        assert_eq!(MODELS.len(), 3, "the verified catalog is three tiers");
        for entry in MODELS {
            assert!(entry.id.starts_with("qwen2.5-coder"));
            assert!(entry.file.ends_with("-Q4_K_M.gguf"));
            let scope = model_url(CheckSource::ModelScope, entry);
            let hf = model_url(CheckSource::HuggingFace, entry);
            let mirror = model_url(CheckSource::HfMirror, entry);
            assert!(scope.starts_with("https://www.modelscope.cn/"));
            assert!(hf.starts_with("https://huggingface.co/"));
            assert!(mirror.starts_with("https://hf-mirror.com/"));
            for url in [scope, hf, mirror] {
                assert!(url.ends_with(entry.file), "url misses the file: {url}");
                assert!(url.contains(entry.repo));
            }
        }
        // Domestic default beats an international default behind the GFW.
        assert_eq!(CheckSource::default(), CheckSource::ModelScope);
    }

    #[test]
    fn model_lookup_falls_back_to_the_first_entry() {
        assert_eq!(
            model_by_id("qwen2.5-coder-1.5b").display,
            "Qwen2.5-Coder-1.5B-Instruct"
        );
        assert_eq!(model_by_id("no-such-model").id, MODELS[0].id);
        // The default config id must resolve to a real entry.
        assert_eq!(
            model_by_id(&crate::config::CodeCheckConfig::default().local_model).id,
            MODELS[0].id
        );
        // Nothing on disk in a bare checkout.
        assert!(
            !is_downloaded("qwen2.5-coder-0.5b")
                || looks_like_model(&model_path("qwen2.5-coder-0.5b"))
        );
    }

    #[test]
    fn parses_hardware_probe_output() {
        let sample = concat!(
            "AMD Ryzen 9 7950X 16-Core Processor\n",
            "CORES:16\n",
            "RAM:34359738368\n",
            "GPU:NVIDIA GeForce RTX 4060|8589934592\n",
        );
        let prefix = "CPU:";
        let sample = format!("{prefix}{sample}");
        let hardware = parse_hardware(&sample);
        assert_eq!(hardware.cpu, "AMD Ryzen 9 7950X 16-Core Processor");
        assert_eq!(hardware.cores, Some(16));
        assert_eq!(hardware.ram_gb, Some(32));
        assert_eq!(hardware.gpu.as_deref(), Some("NVIDIA GeForce RTX 4060"));
        assert_eq!(hardware.vram_gb, Some(8));

        // Partial output degrades instead of panicking.
        let partial = parse_hardware("CPU:Intel i5\nRAM:not-a-number\nGPU:|\n");
        assert_eq!(partial.cpu, "Intel i5");
        assert!(partial.cores.is_none());
        assert!(partial.ram_gb.is_none());
        assert!(partial.gpu.is_none());

        // Empty probe: conservative defaults, recommendation still works.
        let empty = parse_hardware("");
        assert!(empty.cpu.is_empty());
        assert_eq!(recommend(&empty).id, MODELS[0].id);
    }

    #[test]
    fn recommend_follows_available_memory() {
        let mut hardware = Hardware::default();
        assert_eq!(recommend(&hardware).id, MODELS[0].id);
        hardware.ram_gb = Some(8);
        assert_eq!(recommend(&hardware).id, MODELS[1].id);
        hardware.ram_gb = Some(16);
        assert_eq!(recommend(&hardware).id, MODELS[2].id);
        hardware.ram_gb = Some(64);
        assert_eq!(recommend(&hardware).id, MODELS[2].id);
        hardware.ram_gb = Some(4);
        assert_eq!(recommend(&hardware).id, MODELS[0].id);
    }

    #[test]
    fn api_check_parses_and_filters_via_mock_endpoint() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Read, Write};
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(value) = line.strip_prefix("Content-Length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body).unwrap();
            let response = r##"{"choices":[{"message":{"content":"```json\n{\"issues\":[{\"line\":1,\"message\":\"unbalanced parenthesis\"},{\"line\":2,\"message\":\"missing colon\"}]}\n```"}}]}"##;
            let mut stream = reader.into_inner();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
            String::from_utf8_lossy(&body).to_string()
        });

        let config = crate::config::AiConfig {
            base_url: format!("http://127.0.0.1:{port}"),
            api_key: "sk-test".to_string(),
            model: "big-model".to_string(),
            prompt: String::new(),
        };
        // Balanced code: the delimiter claim is refuted, the colon claim stays.
        let code = "x = (1 + 2)\ny = 2";
        let issues = api_check(&config, "python", code).unwrap();
        let body = server.join().unwrap();
        assert!(
            body.contains("代码审查器"),
            "check prompt must travel: {body}"
        );
        assert!(
            issues
                .iter()
                .any(|(_, message)| message.contains("missing colon"))
        );
        assert!(
            !issues
                .iter()
                .any(|(_, message)| message.contains("parenthesis")),
            "delimiter claim on balanced code must be filtered"
        );
    }

    #[test]
    fn parses_issues_from_plain_and_fenced_json() {
        let raw = r#"{"issues":[{"line":3,"message":"undefined name 'lenght'"}]}"#;
        assert_eq!(
            parse_issues(raw),
            vec![(3, "undefined name 'lenght'".to_string())]
        );
        let fenced = format!("```json\n{raw}\n```");
        assert_eq!(parse_issues(&fenced).len(), 1);
        // Two issues keep document order.
        let two = r#"{"issues":[{"line":1,"message":"a"},{"line":9,"message":"b"}]}"#;
        assert_eq!(
            parse_issues(two),
            vec![(1, "a".to_string()), (9, "b".to_string())]
        );
    }

    #[test]
    fn unparseable_replies_degrade_to_empty() {
        assert!(parse_issues("no json here").is_empty());
        assert!(parse_issues("{broken").is_empty());
        assert!(parse_issues(r#"{"issues":"not-an-array"}"#).is_empty());
        assert!(parse_issues(r#"{"issues":[{"message":"missing line"}]}"#).is_empty());
        assert!(parse_issues("").is_empty());
    }

    #[test]
    fn accepts_bare_keys_and_prefill_fragments() {
        // Prefilled prefix + a bare-key completion (0.5B style).
        let bare = "{\"issues\":[{line:2,\"message\":\"missing colon\"}]}";
        assert_eq!(parse_issues(bare), vec![(2, "missing colon".to_string())]);
        // Prefilled prefix closed with an empty list.
        assert!(parse_issues("{\"issues\":[]}").is_empty());
        // Quoted keys still round-trip.
        let quoted = r#"{"issues":[{"line":3,"message":"stray brace"}]}"#;
        assert_eq!(parse_issues(quoted), vec![(3, "stray brace".to_string())]);
        // Message scanning does not stop at the first item.
        let two = "{\"issues\":[{line:1,\"message\":\"first\"},{line:9,\"message\":\"second\"}]}";
        assert_eq!(
            parse_issues(two),
            vec![(1, "first".to_string()), (9, "second".to_string())]
        );
    }

    #[test]
    fn runaway_generations_are_ignored() {
        let mut runaway = String::from("{\"issues\":[");
        for _ in 0..200 {
            runaway.push_str("{\"line\":1,\"message\":\"x\"},");
        }
        assert!(runaway.chars().count() > 600);
        // A repetition loop is noise — mining it would invent findings.
        assert!(parse_issues(&runaway).is_empty());
    }

    #[test]
    fn delimiter_claims_need_a_real_imbalance() {
        // The clean Go snippet: a bracket hallucination gets refuted.
        let go = "package main\n\nfunc main() {\n\tprintln(\"hi\")\n}";
        assert!(mentions_delimiters("unbalanced brackets"));
        assert!(!naive_imbalance(go));
        // The broken expression: imbalance holds, the claim survives.
        let broken = "def total(values):\n    result = 1 +\n    return result";
        assert!(naive_imbalance("x = (1 + 2"));
        assert!(!naive_imbalance(broken)); // balanced parens — colon-class claims pass unchecked
        // Negative depths count too.
        assert!(naive_imbalance("x = 1)"));
        assert!(naive_imbalance("a[1}"));
        // Non-delimiter messages are never gated.
        assert!(!mentions_delimiters("missing colon after return"));
        assert!(mentions_delimiters("Unclosed parenthesis"));
        assert!(mentions_delimiters("括号不匹配"));
    }

    #[test]
    fn model_path_sits_under_the_data_dir() {
        let id = &crate::config::CodeCheckConfig::default().local_model;
        let path = model_path(id);
        assert!(path.ends_with(Path::new("models").join(MODELS[0].file)));
        // Not downloaded in a bare checkout — if it is, size-check it.
        assert!(!is_downloaded(id) || looks_like_model(&path));
    }
}
