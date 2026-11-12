//! Form state and small view-model types shared by the main view.

use std::path::PathBuf;

use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::*;

use crate::config::{
    AiConfig, AppConfig, CnblogsConfig, GitConfig, ImageHostConfig, ImageHostKind, WechatConfig,
};
use crate::publish::Targets;

fn input(
    window: &mut Window,
    cx: &mut App,
    placeholder: &'static str,
    masked: bool,
    value: &str,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(value.to_string());
        if masked {
            state = state.masked(true);
        }
        state
    })
}

pub struct SettingsForm {
    pub ai_base: Entity<InputState>,
    pub ai_key: Entity<InputState>,
    pub ai_model: Entity<InputState>,
    pub ai_prompt: Entity<TextareaState>,
    /// Models returned by the last “拉取模型” click, not persisted.
    pub ai_models: Vec<String>,
    pub image_kind: ImageHostKind,
    pub image_url: Entity<InputState>,
    pub image_auth: Entity<InputState>,
    pub image_token: Entity<InputState>,

    pub wx_appid: Entity<InputState>,
    pub wx_secret: Entity<InputState>,
    pub wx_author: Entity<InputState>,
    pub wx_cover: Entity<InputState>,
    pub wechat_footnote: bool,

    pub cn_user: Entity<InputState>,
    pub cn_token: Entity<InputState>,
    pub cn_blogid: Entity<InputState>,

    pub git_remote: Entity<InputState>,
    pub git_branch: Entity<InputState>,
    pub git_name: Entity<InputState>,
    pub git_email: Entity<InputState>,
    pub git_auto_sync: bool,

    pub backup_dir: Entity<InputState>,
    pub auto_backup: bool,

    pub notice: Option<String>,
    pub busy: Option<String>,
}

impl SettingsForm {
    pub fn new(window: &mut Window, cx: &mut App, config: &AppConfig) -> Self {
        let ai_prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .default_value(config.ai.prompt.clone())
                .placeholder("系统提示词：要求 AI 以什么身份、什么结构、什么风格输出 Markdown 文章")
                .soft_wrap(true)
        });
        Self {
            ai_base: input(window, cx, "https://api.openai.com/v1", false, &config.ai.base_url),
            ai_key: input(window, cx, "sk-…", true, &config.ai.api_key),
            ai_model: input(window, cx, "先拉取模型或手动填写模型名", false, &config.ai.model),
            ai_prompt,
            ai_models: Vec::new(),
            image_kind: config.image_host.kind,
            image_url: input(window, cx, "https://img.example.com", false, &config.image_host.base_url),
            image_auth: input(window, cx, "图床 authCode（可选）", false, &config.image_host.auth_code),
            image_token: input(window, cx, "API Token（可选）", true, &config.image_host.token),
            wx_appid: input(window, cx, "公众号 AppID", false, &config.wechat.app_id),
            wx_secret: input(window, cx, "AppSecret", true, &config.wechat.app_secret),
            wx_author: input(window, cx, "作者名", false, &config.wechat.author),
            wx_cover: input(window, cx, "默认封面图绝对路径（可选）", false, &config.wechat.default_cover),
            wechat_footnote: config.wechat.footnote_links,
            cn_user: input(window, cx, "博客园用户名", false, &config.cnblogs.username),
            cn_token: input(window, cx, "MetaWeblog 访问令牌", true, &config.cnblogs.token),
            cn_blogid: input(window, cx, "Blog ID（留空使用用户名）", false, &config.cnblogs.blog_id),
            git_remote: input(window, cx, "https://github.com/you/repo.git", false, &config.git.remote),
            git_branch: input(window, cx, "main", false, &config.git.branch),
            git_name: input(window, cx, "Git 用户名", false, &config.git.user_name),
            git_email: input(window, cx, "Git 邮箱", false, &config.git.user_email),
            git_auto_sync: config.git.auto_sync,
            backup_dir: input(window, cx, "留空使用默认备份目录", false, &config.backup.directory),
            auto_backup: config.backup.auto_backup,
            notice: None,
            busy: None,
        }
    }

    /// Every single-line field, so the view can watch them all for edits.
    pub fn inputs(&self) -> Vec<Entity<InputState>> {
        vec![
            self.ai_base.clone(),
            self.ai_key.clone(),
            self.ai_model.clone(),
            self.image_url.clone(),
            self.image_auth.clone(),
            self.image_token.clone(),
            self.wx_appid.clone(),
            self.wx_secret.clone(),
            self.wx_author.clone(),
            self.wx_cover.clone(),
            self.cn_user.clone(),
            self.cn_token.clone(),
            self.cn_blogid.clone(),
            self.git_remote.clone(),
            self.git_branch.clone(),
            self.git_name.clone(),
            self.git_email.clone(),
            self.backup_dir.clone(),
        ]
    }

    pub fn apply(&self, config: &mut AppConfig, cx: &App) -> Result<(), String> {
        config.ai = self.ai(cx);
        config.image_host = self.image_host(cx);
        config.wechat = self.wechat(cx);
        config.cnblogs = self.cnblogs(cx);
        config.git = self.git(cx);
        config.backup.directory = self.backup_dir.read(cx).value().to_string();
        config.backup.auto_backup = self.auto_backup;
        Ok(())
    }

    pub fn ai(&self, cx: &App) -> AiConfig {
        AiConfig {
            base_url: self.ai_base.read(cx).value().trim().to_string(),
            api_key: self.ai_key.read(cx).value().trim().to_string(),
            model: self.ai_model.read(cx).value().trim().to_string(),
            prompt: self.ai_prompt.read(cx).value().to_string(),
        }
    }

    pub fn image_host(&self, cx: &App) -> ImageHostConfig {
        ImageHostConfig {
            kind: self.image_kind,
            base_url: self.image_url.read(cx).value().trim().to_string(),
            auth_code: self.image_auth.read(cx).value().trim().to_string(),
            token: self.image_token.read(cx).value().trim().to_string(),
        }
    }

    pub fn wechat(&self, cx: &App) -> WechatConfig {
        WechatConfig {
            app_id: self.wx_appid.read(cx).value().trim().to_string(),
            app_secret: self.wx_secret.read(cx).value().trim().to_string(),
            author: {
                let author = self.wx_author.read(cx).value().trim().to_string();
                if author.is_empty() {
                    "Inkdown".to_string()
                } else {
                    author
                }
            },
            default_cover: self.wx_cover.read(cx).value().trim().to_string(),
            footnote_links: self.wechat_footnote,
        }
    }

    pub fn cnblogs(&self, cx: &App) -> CnblogsConfig {
        CnblogsConfig {
            username: self.cn_user.read(cx).value().trim().to_string(),
            token: self.cn_token.read(cx).value().trim().to_string(),
            blog_id: self.cn_blogid.read(cx).value().trim().to_string(),
        }
    }

    pub fn git(&self, cx: &App) -> GitConfig {
        GitConfig {
            remote: self.git_remote.read(cx).value().trim().to_string(),
            branch: {
                let branch = self.git_branch.read(cx).value().trim().to_string();
                if branch.is_empty() {
                    "main".to_string()
                } else {
                    branch
                }
            },
            user_name: self.git_name.read(cx).value().trim().to_string(),
            user_email: self.git_email.read(cx).value().trim().to_string(),
            auto_sync: self.git_auto_sync,
        }
    }
}

pub struct PublishForm {
    pub title: Entity<InputState>,
    pub targets: Targets,
    pub upload_images: bool,
    pub status: Vec<String>,
    pub busy: bool,
}

impl PublishForm {
    pub fn new(
        window: &mut Window,
        cx: &mut App,
        title: &str,
        config: &AppConfig,
    ) -> Self {
        let mut targets = Targets::default();
        targets.wechat = !config.wechat.app_id.trim().is_empty()
            && !config.wechat.app_secret.trim().is_empty();
        targets.cnblogs = !config.cnblogs.username.trim().is_empty()
            && !config.cnblogs.token.trim().is_empty();
        Self {
            title: input(window, cx, "文章标题", false, title),
            targets,
            upload_images: true,
            status: Vec::new(),
            busy: false,
        }
    }

    pub fn reset(&mut self, title: &str, window: &mut Window, cx: &mut App) {
        self.title
            .update(cx, |state, cx| state.set_value(title.to_string(), window, cx));
        self.status.clear();
        self.busy = false;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Documents,
    Files,
}

#[derive(Clone)]
pub enum PromptKind {
    NewFile(PathBuf),
    NewFolder(PathBuf),
    Rename(PathBuf),
    Delete(PathBuf),
}

pub struct PromptState {
    pub kind: PromptKind,
    pub title: SharedString,
    pub hint: SharedString,
    pub input: Entity<InputState>,
    pub needs_input: bool,
    pub error: Option<String>,
}

pub fn prompt_title(kind: &PromptKind) -> (&'static str, &'static str, bool) {
    match kind {
        PromptKind::NewFile(_) => ("新建 Markdown 文件", "输入文件名，会自动补上 .md", true),
        PromptKind::NewFolder(_) => ("新建文件夹", "输入文件夹名称", true),
        PromptKind::Rename(_) => ("重命名", "输入新的名称", true),
        PromptKind::Delete(_) => ("确认删除", "删除后无法恢复，确定继续吗？", false),
    }
}


pub struct AiForm {
    pub topic: Entity<InputState>,
    pub use_context: bool,
    pub status: Vec<String>,
    pub busy: bool,
}

impl AiForm {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            topic: input(
                window,
                cx,
                "例如：写一篇介绍 Rust 所有权的文章，1200 字左右",
                false,
                "",
            ),
            use_context: false,
            status: Vec::new(),
            busy: false,
        }
    }

    pub fn reset(&mut self, window: &mut Window, cx: &mut App) {
        self.topic
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.status.clear();
        self.busy = false;
    }
}
