//! Image hosting uploads: CloudFlare-ImgBed and Lsky Pro.
//!
//! Both speak multipart form uploads; only the URL, auth and response shape
//! differ. Everything here is blocking and is called from a background task.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::config::{ImageHostConfig, ImageHostKind};

const TEST_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(90))
        .build()
}

pub(crate) fn mime_for(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

fn boundary() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or_default();
    format!("----Inkdown{:x}{:x}", std::process::id(), nanos)
}

pub(crate) fn multipart(
    field: &str,
    file_name: &str,
    mime: &str,
    bytes: &[u8],
) -> (String, Vec<u8>) {
    let boundary = boundary();
    let mut body = Vec::with_capacity(bytes.len() + 512);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"; filename=\"{file_name}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

pub(crate) fn send(request: ureq::Request, body: &[u8]) -> Result<Value, String> {
    match request.send_bytes(body) {
        Ok(response) => response
            .into_json::<Value>()
            .map_err(|error| format!("无法解析图床响应：{error}")),
        Err(ureq::Error::Status(code, response)) => {
            let text = response.into_string().unwrap_or_default();
            let text = if text.len() > 500 {
                format!("{}…", &text[..500])
            } else {
                text
            };
            Err(format!("图床返回 HTTP {code}：{text}"))
        }
        Err(error) => Err(format!("上传请求失败：{error}")),
    }
}

fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Upload raw bytes and return a public URL.
pub fn upload_bytes(
    config: &ImageHostConfig,
    file_name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    if config.base_url.trim().is_empty() {
        return Err("还没有填写图床地址".to_string());
    }
    let base = config.base_url.trim().trim_end_matches('/');
    let mime = mime_for(file_name);
    let (content_type, body) = multipart("file", file_name, mime, bytes);

    let value = match config.kind {
        ImageHostKind::None => return Err("还没有启用图床".to_string()),
        ImageHostKind::CloudflareImgbed => {
            let mut url = format!("{base}/upload?returnFormat=full");
            if !config.auth_code.trim().is_empty() {
                url.push_str("&authCode=");
                url.push_str(&encode(config.auth_code.trim()));
            }
            let mut request = agent()
                .post(&url)
                .set("Content-Type", &content_type)
                .set("Accept", "application/json");
            if !config.token.trim().is_empty() {
                request = request.set("Authorization", &format!("Bearer {}", config.token.trim()));
            }
            send(request, &body)?
        }
        ImageHostKind::Lsky => {
            let base = base.trim_end_matches("/api/v1");
            let url = format!("{base}/api/v1/upload");
            let mut request = agent()
                .post(&url)
                .set("Content-Type", &content_type)
                .set("Accept", "application/json");
            if !config.token.trim().is_empty() {
                request = request.set("Authorization", &format!("Bearer {}", config.token.trim()));
            }
            send(request, &body)?
        }
    };

    extract_url(&value, base, config.kind)
}

fn extract_url(value: &Value, base: &str, kind: ImageHostKind) -> Result<String, String> {
    match kind {
        ImageHostKind::CloudflareImgbed => {
            let item = value
                .as_array()
                .and_then(|items| items.first())
                .unwrap_or(value);
            for key in ["src", "publicUrl", "url"] {
                if let Some(url) = item.get(key).and_then(Value::as_str) {
                    if !url.is_empty() {
                        return Ok(absolute(url, base));
                    }
                }
            }
            Err(format!("图床响应里没有图片地址：{value}"))
        }
        ImageHostKind::Lsky => {
            if value.get("status").and_then(Value::as_bool) == Some(false) {
                let message = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("上传失败");
                return Err(format!("Lsky 返回失败：{message}"));
            }
            let data = value.get("data").unwrap_or(value);
            if let Some(url) = data.pointer("/links/url").and_then(Value::as_str) {
                return Ok(url.to_string());
            }
            for key in ["url", "pathname"] {
                if let Some(url) = data.get(key).and_then(Value::as_str) {
                    if !url.is_empty() {
                        return Ok(absolute(url, base));
                    }
                }
            }
            Err(format!("图床响应里没有图片地址：{value}"))
        }
        ImageHostKind::None => Err("还没有启用图床".to_string()),
    }
}

fn absolute(url: &str, base: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with('/') {
        format!("{base}{url}")
    } else {
        format!("{base}/{url}")
    }
}

pub fn upload_file(config: &ImageHostConfig, path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("读取图片失败：{error}"))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("image.png");
    upload_bytes(config, name, &bytes)
}

/// Upload a tiny PNG so the user can verify a fresh configuration.
pub fn test_upload(config: &ImageHostConfig) -> Result<String, String> {
    upload_bytes(config, "inkdown-test.png", TEST_PNG)
}

/// Download a remote image (used when WeChat needs to mirror it).
pub fn download(url: &str, limit: usize) -> Result<(Vec<u8>, String), String> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|error| format!("下载图片失败：{error}"))?;
    let mime = response
        .header("content-type")
        .unwrap_or("image/png")
        .split(';')
        .next()
        .unwrap_or("image/png")
        .to_string();
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("读取图片失败：{error}"))?;
    if bytes.is_empty() {
        return Err("下载到的图片为空".to_string());
    }
    Ok((bytes, mime))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    fn serve_once(listener: TcpListener) -> String {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let mut content_length = 0usize;
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
            }
        }
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).unwrap();
        let mut stream = reader.into_inner();
        let response = if request_line.contains("/api/v1/upload") {
            r#"{"status":true,"message":"ok","data":{"links":{"url":"http://localhost/lsky.png"}}}"#
        } else {
            r#"[{"src":"http://localhost/cf.png"}]"#
        };
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            response.len(),
            response
        )
        .unwrap();
        request_line
    }

    #[test]
    fn uploads_multipart_to_both_hosts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let first = serve_once(listener.try_clone().unwrap());
            let second = serve_once(listener);
            (first, second)
        });

        let base = format!("http://127.0.0.1:{port}");
        let cf = ImageHostConfig {
            kind: ImageHostKind::CloudflareImgbed,
            base_url: base.clone(),
            auth_code: "abc 123".to_string(),
            token: String::new(),
            upload_on_paste: true,
        };
        let url = upload_bytes(&cf, "a.png", TEST_PNG).unwrap();
        assert_eq!(url, "http://localhost/cf.png");

        let lsky = ImageHostConfig {
            kind: ImageHostKind::Lsky,
            base_url: base,
            auth_code: String::new(),
            token: "token".to_string(),
            upload_on_paste: true,
        };
        let url = upload_bytes(&lsky, "b.png", TEST_PNG).unwrap();
        assert_eq!(url, "http://localhost/lsky.png");

        let (first, second) = server.join().unwrap();
        assert!(first.contains("/upload?returnFormat=full&authCode=abc%20123"));
        assert!(second.contains("/api/v1/upload"));
    }
}
