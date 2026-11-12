//! AI writing: an OpenAI-compatible chat endpoint.
//!
//! Any service that speaks the `/models` and `/chat/completions` protocol —
//! OpenAI, DeepSeek, Moonshot, Ollama, LM Studio, a self-hosted gateway —
//! works here by changing the base URL and key.

use std::time::Duration;

use serde_json::{Value, json};

use crate::config::AiConfig;

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(180))
        .build()
}

fn base(config: &AiConfig) -> Result<String, String> {
    let base = config.base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("还没有填写 AI 服务地址（base URL）".to_string());
    }
    Ok(base.to_string())
}

fn error_body(response: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    match response {
        Ok(response) => response
            .into_json::<Value>()
            .map_err(|error| format!("AI 服务响应无法解析：{error}")),
        Err(ureq::Error::Status(code, response)) => {
            let text = response.into_string().unwrap_or_default();
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| {
                    let trimmed = text.trim();
                    if trimmed.chars().count() > 300 {
                        trimmed.chars().take(300).collect()
                    } else {
                        trimmed.to_string()
                    }
                });
            Err(format!("AI 服务返回 HTTP {code}：{detail}"))
        }
        Err(error) => Err(format!("AI 服务请求失败：{error}")),
    }
}

fn content_of(value: &Value) -> Option<String> {
    let content = value.pointer("/choices/0/message/content")?;
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text: String = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect();
            Some(text)
        }
        _ => None,
    }
}

/// GET {base}/models — the model picker's source.
pub fn fetch_models(config: &AiConfig) -> Result<Vec<String>, String> {
    let base = base(config)?;
    if config.api_key.trim().is_empty() {
        return Err("还没有填写 API Key".to_string());
    }
    let response = agent()
        .get(&format!("{base}/models"))
        .set(
            "Authorization",
            &format!("Bearer {}", config.api_key.trim()),
        )
        .call();
    let value = error_body(response)?;

    let mut models: Vec<String> = value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if models.is_empty() {
        return Err("服务没有返回可用模型列表（响应里没有 data[].id）".to_string());
    }
    models.sort();
    models.dedup();
    Ok(models)
}

/// POST {base}/chat/completions — one non-streaming generation.
///
/// `context` is the current document, passed as user context so the model can
/// continue or revise it; `topic` is what the user typed in the dialog.
pub fn generate(config: &AiConfig, topic: &str, context: &str) -> Result<String, String> {
    let base = base(config)?;
    if config.api_key.trim().is_empty() {
        return Err("还没有填写 API Key".to_string());
    }
    if config.model.trim().is_empty() {
        return Err("还没有选择模型".to_string());
    }
    let topic = topic.trim();
    if topic.is_empty() {
        return Err("先写一句主题或写作要求".to_string());
    }

    let prompt = if config.prompt.trim().is_empty() {
        crate::config::DEFAULT_AI_PROMPT.to_string()
    } else {
        config.prompt.clone()
    };

    let user_message = if context.trim().is_empty() {
        topic.to_string()
    } else {
        format!(
            "请基于下面这篇文档继续写作或改写。\n\n<document>\n{}\n</document>\n\n写作要求：{}",
            context.trim(),
            topic
        )
    };

    let payload = json!({
        "model": config.model.trim(),
        "messages": [
            { "role": "system", "content": prompt },
            { "role": "user", "content": user_message }
        ],
        "stream": false
    });

    let response = agent()
        .post(&format!("{base}/chat/completions"))
        .set(
            "Authorization",
            &format!("Bearer {}", config.api_key.trim()),
        )
        .set("Content-Type", "application/json")
        .send_bytes(
            serde_json::to_string(&payload)
                .map_err(|error| error.to_string())?
                .as_bytes(),
        );
    let value = error_body(response)?;

    content_of(&value).filter(|text| !text.trim().is_empty()).ok_or_else(|| {
        format!(
            "AI 服务没有返回正文（choices[0].message.content 为空）：{}",
            &value.to_string().chars().take(300).collect::<String>()
        )
    })
}

/// Strip a leading ```` ```markdown ```` fence some models wrap output in.
pub fn clean_markdown(text: &str) -> String {
    let trimmed = text.trim_start_matches(['\r', '\n']);
    let trimmed = trimmed.trim_end();
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("```markdown") || lowered.starts_with("```md") {
        let after = &trimmed[trimmed.find('\n').map(|ix| ix + 1).unwrap_or(0)..];
        return after
            .trim_end()
            .strip_suffix("```")
            .unwrap_or(after)
            .trim()
            .to_string();
    }
    if trimmed.starts_with("```") && trimmed.ends_with("```") && trimmed.len() > 6 {
        let after = &trimmed[trimmed.find('\n').map(|ix| ix + 1).unwrap_or(0)..];
        if let Some(end) = after.trim_end().rfind("```") {
            return after.trim_end()[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AiConfig;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn serve(listener: TcpListener) -> (String, String, String) {
        let mut requests = Vec::new();
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut content_length = 0usize;
            let mut authorization = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let line = line.trim_end();
                if line.is_empty() {
                    break;
                }
                if let Some(value) = line.strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                } else if let Some(value) = line.strip_prefix("Content-Length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                } else if let Some(value) = line.strip_prefix("Authorization:") {
                    authorization = value.trim().to_string();
                }
            }
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                reader.read_exact(&mut body).unwrap();
            }
            let mut stream = reader.into_inner();
            let response = if request_line.starts_with("GET ") {
                r##"{"object":"list","data":[{"id":"ink-model-a"},{"id":"ink-model-b"}]}"##
            } else {
                r##"{"choices":[{"message":{"content":"# 生成的文章\n\n正文内容。"}}]}"##
            };
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
            requests.push((
                request_line.trim().to_string(),
                authorization,
                String::from_utf8_lossy(&body).to_string(),
            ));
        }
        (requests[0].0.clone(), requests[0].1.clone(), requests[1].2.clone())
    }

    #[test]
    fn lists_models_then_generates_article() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || serve(listener));

        let config = AiConfig {
            base_url: format!("http://127.0.0.1:{port}"),
            api_key: "sk-test".to_string(),
            model: String::new(),
            prompt: "你是测试写手。".to_string(),
        };

        let mut models = fetch_models(&config).unwrap();
        assert_eq!(models, vec!["ink-model-a", "ink-model-b"]);

        let mut config = config;
        config.model = models.remove(1);
        let article = generate(&config, "写一篇关于测试的文章", "").unwrap();
        assert!(article.contains("生成的文章"));

        let (get_line, auth, chat_body) = server.join().unwrap();
        assert!(get_line.starts_with("GET /models"));
        assert_eq!(auth, "Bearer sk-test");
        assert!(chat_body.contains("ink-model-b"));
        assert!(chat_body.contains("你是测试写手。"));
    }

    #[test]
    fn cleans_wrapped_markdown() {
        assert_eq!(
            clean_markdown("```markdown\n# 标题\n\n正文\n```"),
            "# 标题\n\n正文"
        );
        assert_eq!(clean_markdown("# 直接输出"), "# 直接输出");
        assert_eq!(clean_markdown("```\ncode\n```"), "code");
    }
}
