//! Persistent application settings, stored as JSON in the user's config
//! directory. Secrets live here in plain text; the settings panel says so.

use std::path::PathBuf;
use std::sync::OnceLock;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageHostKind {
    #[default]
    None,
    CloudflareImgbed,
    Lsky,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageHostConfig {
    pub kind: ImageHostKind,
    /// Host root, e.g. https://img.example.com
    pub base_url: String,
    /// CloudFlare-ImgBed upload auth code (query parameter `authCode`).
    pub auth_code: String,
    /// API token (CloudFlare-ImgBed bearer token or Lsky Pro token).
    pub token: String,
    /// Upload an image pasted into the editor straight to the host and insert
    /// its public URL instead of the local assets path.
    #[serde(default = "default_true")]
    pub upload_on_paste: bool,
}

impl Default for ImageHostConfig {
    fn default() -> Self {
        Self {
            kind: ImageHostKind::None,
            base_url: String::new(),
            auth_code: String::new(),
            token: String::new(),
            upload_on_paste: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WechatConfig {
    pub app_id: String,
    pub app_secret: String,
    pub author: String,
    /// Local cover image used when an article has no image of its own.
    pub default_cover: String,
    /// Turn external links into a reference list at the end (WeChat strips
    /// links in article bodies).
    pub footnote_links: bool,
}

impl Default for WechatConfig {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            app_secret: String::new(),
            author: "Inkdown".to_string(),
            default_cover: String::new(),
            footnote_links: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CnblogsConfig {
    pub username: String,
    /// MetaWeblog access token from cnblogs settings.
    pub token: String,
    /// Optional blog id; defaults to the username.
    pub blog_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    pub remote: String,
    pub branch: String,
    pub user_name: String,
    pub user_email: String,
    pub auto_sync: bool,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            remote: String::new(),
            branch: "main".to_string(),
            user_name: String::new(),
            user_email: String::new(),
            auto_sync: false,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupConfig {
    /// Empty means the default Inkdown config directory.
    pub directory: String,
    pub auto_backup: bool,
}

pub const DEFAULT_AI_PROMPT: &str = "你是一位资深中文写作者，熟悉公众号、知乎、博客园等平台的行文风格。根据用户给出的主题，直接输出一篇结构完整的 Markdown 文章：第一行用一级标题作文章标题，正文用二级标题分节，适当使用列表、引用、表格或代码块；语言自然具体，避免空话和排比堆砌。只输出文章本身，不要写前言、开场白或结尾说明。";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// OpenAI-compatible base URL, e.g. https://api.openai.com/v1
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub prompt: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: String::new(),
            model: String::new(),
            prompt: DEFAULT_AI_PROMPT.to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub ai: AiConfig,
    pub image_host: ImageHostConfig,
    pub wechat: WechatConfig,
    pub cnblogs: CnblogsConfig,
    pub git: GitConfig,
    pub backup: BackupConfig,
    /// Last opened folder, restored on the next launch.
    pub last_folder: Option<String>,
    /// The file that was active when the app last closed, restored on launch.
    pub last_document: Option<String>,
    /// Recently opened Markdown files, newest first.
    pub recent_files: Vec<String>,
    /// Restore the dark palette on the next launch.
    pub dark_mode: bool,
}

impl Global for AppConfig {}

/// Optional paths written next to the executable by the Windows installer.
/// They let the installer choose the config and data locations while keeping
/// the portable/zip build working with the platform defaults.
#[derive(Clone, Debug, Default, Deserialize)]
struct InstallPaths {
    #[serde(default)]
    config_dir: String,
    #[serde(default)]
    data_dir: String,
}

fn parse_install_paths(contents: &str) -> InstallPaths {
    serde_json::from_str(contents.trim_start_matches('\u{feff}')).unwrap_or_default()
}

fn install_paths() -> &'static InstallPaths {
    static PATHS: OnceLock<InstallPaths> = OnceLock::new();
    PATHS.get_or_init(|| {
        let Some(exe) = std::env::current_exe().ok() else {
            return InstallPaths::default();
        };
        let Some(parent) = exe.parent() else {
            return InstallPaths::default();
        };
        std::fs::read_to_string(parent.join("install.json"))
            .map(|contents| parse_install_paths(&contents))
            .unwrap_or_default()
    })
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Directory holding `config.json`. The installer can override it; the
/// environment variable wins so tests and portable setups stay controllable.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = env_dir("INKDOWN_CONFIG_DIR") {
        return dir;
    }
    let install = install_paths();
    if !install.config_dir.trim().is_empty() {
        return PathBuf::from(install.config_dir.trim());
    }
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Inkdown")
}

/// Directory for generated data (backups, pasted assets without a document
/// location). `None` means backups follow the config directory.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = env_dir("INKDOWN_DATA_DIR") {
        return Some(dir);
    }
    let install = install_paths();
    (!install.data_dir.trim().is_empty()).then(|| PathBuf::from(install.data_dir.trim()))
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn load() -> AppConfig {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            // Notepad and some editors prepend a UTF-8 BOM; serde rejects it.
            serde_json::from_str(contents.trim_start_matches('\u{feff}')).unwrap_or_default()
        }
        Err(_) => AppConfig::default(),
    }
}

pub fn save(cx: &App) -> Result<(), String> {
    let config = cx.global::<AppConfig>();
    save_to_disk(config)
}

pub fn save_to_disk(config: &AppConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
    std::fs::write(&path, json).map_err(|error| error.to_string())
}

pub fn init(cx: &mut App) {
    cx.set_global(load());
}

/// Default folder for archives and other generated files.
pub fn default_backup_dir(config: &AppConfig) -> PathBuf {
    if config.backup.directory.trim().is_empty() {
        data_dir().unwrap_or_else(config_dir).join("backups")
    } else {
        PathBuf::from(config.backup.directory.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_installer_paths_and_tolerates_bad_json() {
        let parsed = parse_install_paths(
            r#"{"version":"0.1.5","data_dir":"D:/Inkdown data","config_dir":"C:/Users/lee/Inkdown/config"}"#,
        );
        assert_eq!(parsed.data_dir, "D:/Inkdown data");
        assert_eq!(parsed.config_dir, "C:/Users/lee/Inkdown/config");

        let empty = parse_install_paths("not json");
        assert!(empty.data_dir.is_empty());
        assert!(empty.config_dir.is_empty());

        let bom = "\u{feff}{\"data_dir\":\"D:/data\",\"config_dir\":\"C:/config\"}";
        let parsed = parse_install_paths(bom);
        assert_eq!(parsed.data_dir, "D:/data");
        assert_eq!(parsed.config_dir, "C:/config");
    }
}
