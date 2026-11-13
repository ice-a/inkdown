//! The application view: title bar, command toolbar, document sidebar,
//! editor/preview workspace and status bar.

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, StyledExt as _,
    TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{
        Editor, EditorState, Input, InputEvent, InputState, Redo, RopeExt as _, TabSize, Textarea,
        TextareaState, Undo,
    },
    menu::{DropdownMenu as _, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    text::{
        FrontmatterPlugin, MarkdownExtensions, RangeHighlight, SelectionFormat, TextView,
        TextViewState, TextViewStyle,
    },
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::actions::*;
use crate::doc::{self, Document, Heading, Stats};
use crate::publish::upload_local_images;
use crate::ui::{AiForm, PromptKind, PromptState, PublishForm, SettingsForm, SidebarTab};
use crate::{ai, backup, commands, config, git, imagebed, publish, theme, ui, widgets, workspace};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Write,
    Split,
    Preview,
}

pub struct InkdownApp {
    docs: Vec<Document>,
    active: usize,
    next_id: u64,
    untitled_count: usize,
    editor: Entity<EditorState>,
    preview: Entity<TextViewState>,
    view_mode: ViewMode,
    sidebar_visible: bool,
    outline: Vec<Heading>,
    stats: Stats,
    preview_zoom: f32,
    preview_find_open: bool,
    preview_find: Entity<InputState>,
    preview_matches: Vec<Range<usize>>,
    preview_current: usize,
    notice: Option<String>,
    _subscriptions: Vec<Subscription>,

    workspace: Option<workspace::FileTree>,
    expanded: HashSet<PathBuf>,
    selected_path: Option<PathBuf>,
    sidebar_tab: SidebarTab,

    settings_open: bool,
    settings: SettingsForm,
    settings_watch: Vec<Subscription>,
    ai_open: bool,
    ai: AiForm,
    chat_open: bool,
    chat_turns: Vec<ai::ChatTurn>,
    chat_input: Entity<TextareaState>,
    chat_busy: bool,
    chat_error: Option<String>,
    chat_use_doc: bool,
    chat_scroll: ScrollHandle,
    publish_open: bool,
    publish: PublishForm,
    prompt: Option<PromptState>,
    action_busy: Option<String>,
    /// Set when a close was refused for unsaved documents; holds the count.
    close_confirm: Option<usize>,
    quit_confirmed: bool,

    meta_open: bool,
    meta_fields: Vec<(Entity<InputState>, Entity<InputState>)>,
    templates_open: bool,
    /// Recent Markdown files that still exist, newest first.
    recent: Vec<PathBuf>,
    slash: Option<SlashState>,
    code_lang: Option<CodeLangPicker>,
    custom_commands: Vec<commands::CustomCommand>,
}

/// Where the AI result lands.
#[derive(Clone, Copy)]
enum AiTarget {
    NewDocument,
    Insert,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuiltinCommand {
    Table,
    Image,
    Ai,
    Time,
    CodeBlock,
    Quote,
    Todo,
    Link,
    Templates,
    Metadata,
    OpenCommands,
    NewCommandTemplates,
}

impl BuiltinCommand {
    const ALL: [Self; 12] = [
        Self::Table,
        Self::Image,
        Self::Ai,
        Self::Time,
        Self::CodeBlock,
        Self::Quote,
        Self::Todo,
        Self::Link,
        Self::Templates,
        Self::Metadata,
        Self::OpenCommands,
        Self::NewCommandTemplates,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Table => "插入表格",
            Self::Image => "插入图片",
            Self::Ai => "询问 AI",
            Self::Time => "当前时间",
            Self::CodeBlock => "代码块",
            Self::Quote => "引用",
            Self::Todo => "待办事项",
            Self::Link => "插入链接",
            Self::Templates => "从模板新建",
            Self::Metadata => "文档元数据",
            Self::OpenCommands => "打开命令目录",
            Self::NewCommandTemplates => "生成命令模板",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Table => "插入一个 3 列的 Markdown 表格",
            Self::Image => "从剪贴板或本地文件插入图片",
            Self::Ai => "打开对话窗口，问完可直接插入文稿",
            Self::Time => "插入当前日期和时间",
            Self::CodeBlock => "插入带语言的围栏代码块",
            Self::Quote => "插入引用标记",
            Self::Todo => "插入一个待办事项",
            Self::Link => "插入链接",
            Self::Templates => "打开文章、笔记、周报等模板",
            Self::Metadata => "编辑文档开头的 YAML frontmatter",
            Self::OpenCommands => "在文件管理器中打开自定义命令目录",
            Self::NewCommandTemplates => "生成 JS / Python 示例命令和说明",
        }
    }

    fn keywords(self) -> &'static str {
        match self {
            Self::Table => "table 表格",
            Self::Image => "image picture photo 图片 照片",
            Self::Ai => "ai ask 询问 提问 生成",
            Self::Time => "time date clock now 时间 日期",
            Self::CodeBlock => "code fence 代码",
            Self::Quote => "quote 引用",
            Self::Todo => "todo task checkbox 待办 任务",
            Self::Link => "link url 链接",
            Self::Templates => "template 模板 新建",
            Self::Metadata => "metadata frontmatter 元数据",
            Self::OpenCommands => "custom script folder 命令 目录 自定义",
            Self::NewCommandTemplates => "template example custom 模板 示例 自定义",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Table => IconName::Table,
            Self::Image => IconName::Image,
            Self::Ai => IconName::Sparkles,
            Self::Time => IconName::Clock,
            Self::CodeBlock => IconName::Code,
            Self::Quote => IconName::Quote,
            Self::Todo => IconName::ListTodo,
            Self::Link => IconName::Link,
            Self::Templates => IconName::LayoutTemplate,
            Self::Metadata => IconName::Type,
            Self::OpenCommands => IconName::FolderCog,
            Self::NewCommandTemplates => IconName::FilePlus,
        }
    }
}

#[derive(Clone)]
enum SlashAction {
    Builtin(BuiltinCommand),
    Custom(commands::CustomCommand),
}

#[derive(Clone)]
struct SlashEntry {
    name: String,
    description: String,
    keywords: String,
    icon: IconName,
    badge: Option<&'static str>,
    action: SlashAction,
}

struct SlashState {
    /// Byte offset of the `/` in the editor.
    trigger: usize,
    /// Text typed after the slash at the caret.
    query: String,
    selected: usize,
}

/// The code-block language picker.
struct CodeLangPicker {
    /// Byte offset of the block's opening fence in the document.
    fence: usize,
    /// The language the fence carries now, empty when it has none.
    current: String,
}

/// The system message for the chat dialog: the fixed prompt, plus the
/// document as context when the caller asked for it.
fn chat_system_prompt(context: &str) -> String {
    if context.trim().is_empty() {
        ai::CHAT_SYSTEM_PROMPT.to_string()
    } else {
        format!(
            "{}\n\n当前文稿（供参考）：\n<document>\n{}\n</document>",
            ai::CHAT_SYSTEM_PROMPT,
            context.trim()
        )
    }
}

impl InkdownApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = cx
            .try_global::<config::AppConfig>()
            .cloned()
            .unwrap_or_default();

        // Restore the last real file if it is still on disk; otherwise start
        // with a clean sheet and let the preview show the workbench. Demo
        // documents stay opt-in so they never hide the user's own workspace.
        let restored = config.last_document.as_ref().and_then(|stored| {
            let path = PathBuf::from(stored);
            let content = std::fs::read_to_string(&path).ok()?;
            Some((path, content))
        });
        let mut untitled_count = 0;
        let docs = match restored {
            Some((path, content)) => {
                let title = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("未命名.md");
                vec![Document {
                    id: 1,
                    title: title.into(),
                    path: Some(path),
                    saved_content: content.clone(),
                    content,
                }]
            }
            None => {
                untitled_count = 1;
                vec![Document::untitled(1, 1)]
            }
        };
        let first = docs[0].content.clone();

        let mut recent = Vec::new();
        let mut seen = HashSet::new();
        for stored in &config.recent_files {
            let path = PathBuf::from(stored);
            if path.is_file() && seen.insert(path.clone()) {
                recent.push(path);
            }
            if recent.len() >= 12 {
                break;
            }
        }

        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .default_value(first.clone())
                .language("markdown")
                .line_number(true)
                .soft_wrap(true)
                .searchable(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    ..Default::default()
                })
                .placeholder("开始写下你的想法…")
        });
        let preview = cx.new(|cx| TextViewState::markdown(&first, cx));
        let preview_find = cx.new(|cx| InputState::new(window, cx).placeholder("在预览中查找"));
        let outline = doc::outline(&first);
        let stats = doc::stats(&first);

        let mut expanded = HashSet::new();
        let workspace = config.last_folder.as_ref().and_then(|folder| {
            let path = PathBuf::from(folder);
            if path.is_dir() {
                expanded.insert(path.clone());
                Some(workspace::FileTree::open(path))
            } else {
                None
            }
        });
        let selected_path = docs[0]
            .path
            .as_ref()
            .filter(|path| {
                workspace
                    .as_ref()
                    .is_some_and(|tree| path.starts_with(&tree.root))
            })
            .cloned();
        if let (Some(root), Some(path)) = (
            workspace.as_ref().map(|tree| tree.root.clone()),
            selected_path.as_ref(),
        ) {
            workspace::expand_ancestors(&root, path, &mut expanded);
        }
        let sidebar_tab = if workspace.is_some() {
            SidebarTab::Files
        } else {
            SidebarTab::Documents
        };
        let settings = SettingsForm::new(window, cx, &config);
        let ai = AiForm::new(window, cx);
        let chat_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("问点什么：改写、润色、解释、出大纲……")
                .soft_wrap(true)
                .submit_on_enter(true)
        });
        let publish = PublishForm::new(window, cx, &docs[0].file_stem(), &config);

        let mut this = Self {
            docs,
            active: 0,
            next_id: 10,
            untitled_count,
            editor: editor.clone(),
            preview,
            view_mode: ViewMode::Split,
            sidebar_visible: true,
            outline,
            stats,
            preview_zoom: 1.0,
            preview_find_open: false,
            preview_find: preview_find.clone(),
            preview_matches: Vec::new(),
            preview_current: 0,
            notice: None,
            _subscriptions: Vec::new(),
            workspace,
            expanded,
            selected_path,
            sidebar_tab,
            settings_open: false,
            settings,
            settings_watch: Vec::new(),
            ai_open: false,
            ai,
            chat_open: false,
            chat_turns: Vec::new(),
            chat_input,
            chat_busy: false,
            chat_error: None,
            chat_use_doc: false,
            chat_scroll: ScrollHandle::default(),
            publish_open: false,
            publish,
            prompt: None,
            action_busy: None,
            close_confirm: None,
            quit_confirmed: false,
            meta_open: false,
            meta_fields: Vec::new(),
            templates_open: false,
            recent,
            slash: None,
            code_lang: None,
            custom_commands: Vec::new(),
        };

        let change_sub = cx.subscribe(&editor, |this, _editor, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_editor(cx);
            }
        });
        // The status bar and the outline follow the caret, which can move
        // without changing the text, so listen to every editor notification.
        let cursor_sub = cx.observe(&editor, |this, _editor, cx| {
            this.sync_slash_cursor(cx);
            cx.notify();
        });
        let find_sub = cx.subscribe(&preview_find, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.refresh_preview_find(true, cx);
            }
        });
        this._subscriptions.push(change_sub);
        this._subscriptions.push(cursor_sub);
        this._subscriptions.push(find_sub);

        // Slash-palette navigation is intercepted before the keymap so the
        // editor's own Up/Down/Enter/Tab/Escape bindings cannot win.
        let slash_handle = cx.weak_entity();
        let slash_sub = cx.intercept_keystrokes(move |event, window, cx| {
            let modifiers = event.keystroke.modifiers;
            // Ignore shortcuts; plain arrows/Enter/Tab/Esc drive the palette.
            if modifiers.control || modifiers.alt || modifiers.platform {
                return;
            }
            let key = event.keystroke.key.as_str();
            if !matches!(key, "up" | "down" | "enter" | "tab" | "escape") {
                return;
            }
            let handled = slash_handle
                .update(cx, |this, cx| {
                    if this.slash.is_none() {
                        return false;
                    }
                    match key {
                        "up" => this.slash_up(cx),
                        "down" => this.slash_down(cx),
                        "enter" | "tab" => this.accept_slash(window, cx),
                        "escape" => this.slash_close(cx),
                        _ => return false,
                    }
                    true
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        });
        this._subscriptions.push(slash_sub);

        let focus = this.editor.read(cx).focus_handle(cx);
        let blur_sub = cx.on_blur(&focus, window, |this, _window, cx| {
            if this.slash.take().is_some() {
                cx.notify();
            }
        });
        this._subscriptions.push(blur_sub);
        let initial_focus = focus.clone();
        window.defer(cx, move |window, cx| initial_focus.focus(window, cx));

        // Closing the window quits the app; keep the user from losing unsaved
        // work without a confirmation, and flush pending settings first.
        let handle = cx.weak_entity();
        window.on_window_should_close(cx, move |_window, cx| {
            let handle = handle.clone();
            handle
                .update(cx, |this, cx| {
                    if this.quit_confirmed {
                        return true;
                    }
                    if this.settings_open {
                        let _ = this.flush_settings(cx);
                    }
                    let dirty = this.docs.iter().filter(|doc| doc.is_dirty()).count();
                    if dirty == 0 {
                        return true;
                    }
                    if this.close_confirm != Some(dirty) {
                        this.close_confirm = Some(dirty);
                        cx.notify();
                    }
                    false
                })
                .unwrap_or(true)
        });
        this
    }

    fn sync_editor(&mut self, cx: &mut Context<Self>) {
        let value = self.editor.read(cx).value().to_string();
        let cursor = self.editor.read(cx).cursor();
        let previous = self
            .docs
            .get(self.active)
            .map(|doc| doc.content.clone())
            .unwrap_or_default();
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.content = value.clone();
        }
        self.preview
            .update(cx, |state, cx| state.set_text(&value, cx));
        self.outline = doc::outline(&value);
        self.stats = doc::stats(&value);
        if self.preview_find_open {
            self.refresh_preview_find(false, cx);
        }
        self.update_slash_state(&previous, &value, cursor);
        self.notice = None;
        cx.notify();
    }

    fn focus_editor(&self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.editor.read(cx).focus_handle(cx);
        focus.focus(window, cx);
    }

    fn after_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_editor(cx);
        self.focus_editor(window, cx);
    }

    fn activate_doc(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.docs.len() || index == self.active {
            self.focus_editor(window, cx);
            return;
        }
        self.slash = None;
        let current = self.editor.read(cx).value().to_string();
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.content = current;
        }

        self.active = index;
        let content = self.docs[index].content.clone();
        self.editor.update(cx, |state, cx| {
            state.set_value(&content, window, cx);
            state.set_selected_range(0..0, cx);
            state.set_scroll_offset(point(px(0.), px(0.)), cx);
        });
        self.preview
            .update(cx, |state, cx| state.set_text(&content, cx));
        self.outline = doc::outline(&content);
        self.stats = doc::stats(&content);
        self.notice = None;
        if let Some(path) = self.docs[index].path.clone() {
            self.remember_document(&path, cx);
        }
        self.focus_editor(window, cx);
        cx.notify();
    }

    /// Keep the recent-files list and the last-document marker in sync with
    /// the file the user is working on.
    fn remember_document(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.recent.retain(|item| item.as_path() != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(12);

        let stored = path.to_string_lossy().to_string();
        {
            let config = cx.global_mut::<config::AppConfig>();
            config.recent_files.retain(|item| item != &stored);
            config.recent_files.insert(0, stored.clone());
            config.recent_files.truncate(12);
            config.last_document = Some(stored);
        }
        config::save(cx).ok();
        cx.notify();
    }

    fn recent_item(&self, index: usize, path: PathBuf, cx: &mut Context<Self>) -> AnyElement {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("未命名.md")
            .to_string();
        div()
            .id(("recent-doc", index))
            .w_full()
            .px_2()
            .py(px(4.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .cursor(CursorStyle::PointingHand)
            .text_color(cx.theme().sidebar_foreground)
            .hover(|style| style.bg(cx.theme().sidebar_accent.opacity(0.55)))
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_recent(path.clone(), window, cx)),
            )
            .child(
                Icon::new(IconName::FileClock)
                    .xsmall()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().flex_1().min_w_0().truncate().text_sm().child(title))
            .into_any_element()
    }

    fn open_recent(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if let Some(root) = self.workspace.as_ref().map(|tree| tree.root.clone())
                    && path.starts_with(&root)
                {
                    workspace::expand_ancestors(&root, &path, &mut self.expanded);
                    self.selected_path = Some(path.clone());
                }
                self.open_path(path, content, window, cx);
            }
            Err(error) => {
                self.recent.retain(|item| item != &path);
                let stored = path.to_string_lossy().to_string();
                {
                    let config = cx.global_mut::<config::AppConfig>();
                    config.recent_files.retain(|item| item != &stored);
                    if config.last_document.as_deref() == Some(stored.as_str()) {
                        config.last_document = None;
                    }
                }
                config::save(cx).ok();
                self.notice = Some(format!("打开最近文稿失败：{error}"));
                cx.notify();
            }
        }
    }

    fn new_from_template(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(template) = doc::TEMPLATES.get(index) else {
            return;
        };
        let id = self.next_id;
        self.next_id += 1;
        let mut document = template.document(id);
        let base = template.file_stem.to_string();
        let mut title = format!("{base}.md");
        let mut suffix = 2;
        while self
            .docs
            .iter()
            .any(|doc| doc.title.as_ref() == title.as_str())
        {
            title = format!("{base} {suffix}.md");
            suffix += 1;
        }
        document.title = title.into();
        self.docs.push(document);
        let added = self.docs.len() - 1;
        self.templates_open = false;
        self.activate_doc(added, window, cx);
        self.notice = Some(format!("已从「{}」模板新建文稿", template.name));
        cx.notify();
    }

    fn load_samples(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let start = self.docs.len();
        for mut sample in doc::sample_documents() {
            sample.id = self.next_id;
            self.next_id += 1;
            self.docs.push(sample);
        }
        self.activate_doc(start, window, cx);
        self.notice = Some("已载入示例文稿，随时可以关掉".to_string());
        cx.notify();
    }

    fn new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.untitled_count += 1;
        let id = self.next_id;
        self.next_id += 1;
        self.docs.push(Document::untitled(id, self.untitled_count));
        let index = self.docs.len() - 1;
        self.activate_doc(index, window, cx);
    }

    fn open_path(
        &mut self,
        path: PathBuf,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self
            .docs
            .iter()
            .position(|doc| doc.path.as_deref() == Some(path.as_path()))
        {
            self.activate_doc(index, window, cx);
            return;
        }

        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("未命名.md");
        let id = self.next_id;
        self.next_id += 1;
        self.docs.push(Document {
            id,
            title: title.into(),
            path: Some(path),
            saved_content: content.clone(),
            content,
        });
        let index = self.docs.len() - 1;
        self.activate_doc(index, window, cx);
    }

    fn write_doc(&mut self, id: u64, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.docs.iter().position(|doc| doc.id == id) else {
            return;
        };
        let content = self.docs[index].content.clone();
        match std::fs::write(&path, content.as_bytes()) {
            Ok(()) => {
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    self.docs[index].title = name.into();
                }
                self.docs[index].path = Some(path.clone());
                self.docs[index].saved_content = content;
                self.notice = Some("已保存到磁盘".to_string());
                self.remember_document(&path, cx);
                let auto_backup = cx.global::<config::AppConfig>().backup.auto_backup;
                let auto_git = cx.global::<config::AppConfig>().git.auto_sync;
                let backup_dir = config::default_backup_dir(cx.global::<config::AppConfig>());
                let git_config = cx.global::<config::AppConfig>().git.clone();
                self.refresh_tree(cx);
                if auto_backup {
                    self.backup_to(backup_dir, window, cx);
                }
                if auto_git {
                    if let Some(root) = self.workspace.as_ref().map(|tree| tree.root.clone()) {
                        if git::is_repo(&root) {
                            self.run_git(root, git_config, false, window, cx);
                        }
                    }
                }
            }
            Err(error) => {
                self.notice = Some(format!("保存失败：{error}"));
            }
        }
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc = &self.docs[self.active];
        if let Some(path) = doc.path.clone() {
            let id = doc.id;
            self.write_doc(id, path, window, cx);
        } else {
            self.save_as(window, cx);
        }
    }

    fn save_as(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc = &self.docs[self.active];
        let id = doc.id;
        let suggested = format!("{}.md", doc.file_stem());
        let directory = doc
            .path
            .as_ref()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.write_doc(id, path, window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn export_html(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc = &self.docs[self.active];
        let id = doc.id;
        let suggested = format!("{}.html", doc.file_stem());
        let directory = doc
            .path
            .as_ref()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = receiver.await else {
                return;
            };
            this.update_in(cx, |this, _window, cx| {
                if let Some(doc) = this.docs.iter().find(|doc| doc.id == id) {
                    let page = doc::export_html(&doc.title, &doc.content);
                    match std::fs::write(&path, page) {
                        Ok(()) => {
                            this.notice = Some(format!("已导出 {}", path.display()));
                        }
                        Err(error) => {
                            this.notice = Some(format!("导出失败：{error}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_view(&mut self, mode: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        self.view_mode = mode;
        if mode != ViewMode::Preview {
            self.focus_editor(window, cx);
        }
        cx.notify();
    }

    // ---- formatting -----------------------------------------------------

    fn wrap_selection(
        &mut self,
        prefix: &'static str,
        suffix: &'static str,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            let selected = state.selected_text().to_string();
            if selected.is_empty() {
                let start = state.cursor();
                state.replace(format!("{prefix}{placeholder}{suffix}"), window, cx);
                let inner = start + prefix.len();
                state.set_selected_range(inner..inner + placeholder.len(), cx);
            } else {
                state.replace(format!("{prefix}{selected}{suffix}"), window, cx);
            }
        });
        self.after_edit(window, cx);
    }

    fn code_block(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |state, cx| {
            let selected = state.selected_text().to_string();
            if selected.is_empty() {
                let start = state.cursor();
                state.replace("```\n\n```", window, cx);
                state.set_selected_range(start + 4..start + 4, cx);
            } else {
                state.replace(format!("```\n{selected}\n```"), window, cx);
            }
        });
        self.after_edit(window, cx);
    }

    /// Open the code-block language picker for the fence at `fence`.
    fn open_code_lang(&mut self, fence: usize, current: String, cx: &mut Context<Self>) {
        self.slash = None;
        self.code_lang = Some(CodeLangPicker { fence, current });
        cx.notify();
    }

    /// Write `token` on the fence the picker was opened for. An empty token
    /// removes the language, leaving a plain fence.
    fn set_code_lang(&mut self, token: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(picker) = self.code_lang.take() else {
            return;
        };
        let source = self.editor.read(cx).value().to_string();
        let Some(range) = doc::fence_lang_range(&source, picker.fence) else {
            self.notice = Some("没有找到这段代码块的围栏，可能已被修改".to_string());
            self.focus_editor(window, cx);
            cx.notify();
            return;
        };
        // Keep the caret where it was, shifted by the length of the edit.
        let cursor = self.editor.read(cx).cursor();
        let delta = token.len() as isize - (range.end - range.start) as isize;
        let caret = if cursor <= range.start {
            cursor
        } else if cursor >= range.end {
            (cursor as isize + delta).max(0) as usize
        } else {
            range.start + token.len()
        };
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(range.clone(), cx);
            state.replace(token.to_string(), window, cx);
            let caret = caret.min(state.value().len());
            state.set_selected_range(caret..caret, cx);
        });
        self.after_edit(window, cx);
    }

    fn insert_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |state, cx| {
            let selected = state.selected_text().to_string();
            let start = state.selected_range().start;
            let label = if selected.is_empty() {
                "链接文字".to_string()
            } else {
                selected
            };
            state.replace(format!("[{label}](https://)"), window, cx);
            let inner = start + 1;
            state.set_selected_range(inner..inner + label.len(), cx);
        });
        self.after_edit(window, cx);
    }

    fn set_heading(&mut self, level: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |state, cx| {
            let text = state.text().clone();
            let row = state.cursor_position().line as usize;
            let line_start = text.line_start_offset(row);
            let line = text.slice_line(row).to_string();
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            let existing = trimmed.chars().take_while(|ch| *ch == '#').count();
            let has_heading = existing >= 1 && trimmed[existing..].starts_with(' ');
            let remove = if has_heading { existing + 1 } else { 0 };
            let prefix = format!("{} ", "#".repeat(level));
            let content_start = line_start + indent;
            let cursor = state.cursor();
            let new_cursor = adjust_cursor(cursor, content_start, remove, prefix.len());

            state.set_selected_range(content_start..content_start + remove, cx);
            state.replace(prefix, window, cx);
            state.set_selected_range(new_cursor..new_cursor, cx);
        });
        self.after_edit(window, cx);
    }

    fn toggle_line_prefix(
        &mut self,
        prefix: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            let text = state.text().clone();
            let row = state.cursor_position().line as usize;
            let line_start = text.line_start_offset(row);
            let line = text.slice_line(row).to_string();
            let trimmed = line.trim_start();
            let indent = line.len() - trimmed.len();
            let content_start = line_start + indent;
            let cursor = state.cursor();

            if trimmed.starts_with(prefix) {
                let remove = prefix.len();
                let new_cursor = adjust_cursor(cursor, content_start, remove, 0);
                state.set_selected_range(content_start..content_start + remove, cx);
                state.replace("", window, cx);
                state.set_selected_range(new_cursor..new_cursor, cx);
            } else {
                let new_cursor = adjust_cursor(cursor, content_start, 0, prefix.len());
                state.set_selected_range(content_start..content_start, cx);
                state.replace(prefix, window, cx);
                state.set_selected_range(new_cursor..new_cursor, cx);
            }
        });
        self.after_edit(window, cx);
    }

    fn insert_template(&mut self, template: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.insert_text(template, window, cx);
    }

    fn insert_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.insert(text.to_string(), window, cx));
        self.after_edit(window, cx);
    }

    fn insert_text_with_cursor(
        &mut self,
        text: &str,
        cursor_offset: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = self.editor.read(cx).selected_range().start;
        self.editor.update(cx, |state, cx| {
            state.insert(text.to_string(), window, cx);
            let cursor = start + cursor_offset;
            state.set_selected_range(cursor..cursor, cx);
        });
        self.after_edit(window, cx);
    }

    // ---- slash commands -------------------------------------------------

    fn update_slash_state(&mut self, previous: &str, current: &str, cursor: usize) {
        if self.slash.is_some() {
            let (trigger, old_query) = {
                let state = self.slash.as_ref().expect("slash state checked");
                (state.trigger, state.query.clone())
            };
            if cursor <= trigger
                || cursor > current.len()
                || trigger >= current.len()
                || !current[trigger..].starts_with('/')
            {
                self.slash = None;
                return;
            }
            let query = current[trigger + 1..cursor].to_string();
            if query.contains('\n') || query.chars().count() > 32 {
                self.slash = None;
                return;
            }
            if query != old_query {
                if let Some(state) = self.slash.as_mut() {
                    state.query = query;
                    state.selected = 0;
                }
            }
            return;
        }

        // Only open on a freshly typed slash at a word boundary, never for
        // an existing slash the caret happens to sit behind.
        if current.len() != previous.len() + 1 {
            return;
        }
        let Some(slash_at) = cursor.checked_sub(1) else {
            return;
        };
        if current.as_bytes().get(slash_at) != Some(&b'/') || !slash_boundary(current, slash_at) {
            return;
        }
        // The edit must be exactly the typed slash: the text after removing the
        // byte at `slash_at` has to equal the previous document. Comparing
        // slices avoids copying the whole document on every keystroke.
        if current[..slash_at] != previous[..slash_at] || current[cursor..] != previous[slash_at..]
        {
            return;
        }
        if self.settings_open
            || self.publish_open
            || self.ai_open
            || self.chat_open
            || self.meta_open
            || self.templates_open
            || self.code_lang.is_some()
            || self.prompt.is_some()
            || self.close_confirm.is_some()
        {
            return;
        }

        self.custom_commands = commands::load_custom().unwrap_or_default();
        self.slash = Some(SlashState {
            trigger: slash_at,
            query: String::new(),
            selected: 0,
        });
    }

    fn sync_slash_cursor(&mut self, cx: &mut Context<Self>) {
        let Some(trigger) = self.slash.as_ref().map(|state| state.trigger) else {
            return;
        };
        // Recompute only the query, straight from the editor's rope: this
        // runs on every caret move and keystroke, so it must not copy the
        // whole document.
        let (closed, query) = {
            let state = self.editor.read(cx);
            let text = state.text();
            let cursor = state.cursor();
            if cursor <= trigger || cursor > text.len() || trigger >= text.len() {
                (true, String::new())
            } else if text.slice(trigger..trigger + 1).to_string() != "/" {
                (true, String::new())
            } else {
                let query = text.slice(trigger + 1..cursor).to_string();
                let closed = query.contains('\n') || query.chars().count() > 32;
                (closed, query)
            }
        };
        if closed {
            self.slash = None;
            return;
        }
        if let Some(state) = self.slash.as_mut()
            && state.query != query
        {
            state.query = query;
            state.selected = 0;
        }
    }

    fn slash_entries(&self) -> Vec<SlashEntry> {
        let query = self
            .slash
            .as_ref()
            .map(|state| state.query.trim().to_lowercase())
            .unwrap_or_default();
        let mut entries = BuiltinCommand::ALL
            .iter()
            .map(|command| SlashEntry {
                name: command.name().to_string(),
                description: command.description().to_string(),
                keywords: command.keywords().to_string(),
                icon: command.icon(),
                badge: None,
                action: SlashAction::Builtin(*command),
            })
            .collect::<Vec<_>>();

        for command in &self.custom_commands {
            entries.push(SlashEntry {
                name: command.name.clone(),
                description: command.description.clone(),
                keywords: format!("{} {}", command.name, command.description),
                icon: IconName::SquareTerminal,
                badge: Some(command.runtime.label()),
                action: SlashAction::Custom(command.clone()),
            });
        }

        if !query.is_empty() {
            entries.retain(|entry| {
                entry.name.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query)
                    || entry.keywords.to_lowercase().contains(&query)
            });
        }
        entries
    }

    fn accept_slash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.slash.is_none() {
            return;
        }
        let entries = self.slash_entries();
        if entries.is_empty() {
            return;
        }
        let index = self
            .slash
            .as_ref()
            .map(|state| state.selected)
            .unwrap_or(0)
            .min(entries.len() - 1);
        let query = self
            .slash
            .as_ref()
            .map(|state| state.query.clone())
            .unwrap_or_default();
        let action = entries[index].action.clone();
        if !self.take_slash_trigger(window, cx) {
            self.slash = None;
            cx.notify();
            return;
        }
        self.execute_slash_action(action, query, window, cx);
    }

    /// Remove the `/query` text, leaving the caret where the slash started.
    fn take_slash_trigger(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.slash.take() else {
            return false;
        };
        let text = self.editor.read(cx).value().to_string();
        let cursor = self.editor.read(cx).cursor().min(text.len());
        let start = state.trigger;
        if cursor <= start || start >= text.len() || !text[start..].starts_with('/') {
            self.focus_editor(window, cx);
            return false;
        }
        let mut updated = String::with_capacity(text.len().saturating_sub(cursor - start));
        updated.push_str(&text[..start]);
        updated.push_str(&text[cursor..]);
        self.editor.update(cx, |state, cx| {
            state.replace_all(updated, window, cx);
            state.set_selected_range(start..start, cx);
        });
        self.sync_editor(cx);
        self.focus_editor(window, cx);
        true
    }

    fn execute_slash_action(
        &mut self,
        action: SlashAction,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            SlashAction::Builtin(command) => match command {
                BuiltinCommand::Table => self.insert_template(TABLE_TEMPLATE, window, cx),
                BuiltinCommand::Image => self.insert_image_command(window, cx),
                BuiltinCommand::Ai => self.open_chat(window, cx),
                BuiltinCommand::Time => {
                    let now = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
                    self.insert_text(&now, window, cx);
                }
                BuiltinCommand::CodeBlock => {
                    let fence = self.editor.read(cx).selected_range().start;
                    self.insert_text_with_cursor("```\n\n```", 4, window, cx);
                    self.open_code_lang(fence, String::new(), cx);
                }
                BuiltinCommand::Quote => self.toggle_line_prefix("> ", window, cx),
                BuiltinCommand::Todo => self.insert_text("- [ ] ", window, cx),
                BuiltinCommand::Link => {
                    self.insert_text("[链接文字](https://example.com)", window, cx)
                }
                BuiltinCommand::Templates => {
                    self.templates_open = true;
                    cx.notify();
                }
                BuiltinCommand::Metadata => self.open_metadata(window, cx),
                BuiltinCommand::OpenCommands => {
                    match commands::ensure_dir() {
                        Ok(dir) => {
                            cx.reveal_path(&dir);
                            self.notice = Some(format!("命令目录：{}", dir.display()));
                        }
                        Err(error) => self.notice = Some(error),
                    }
                    cx.notify();
                }
                BuiltinCommand::NewCommandTemplates => {
                    match commands::ensure_templates() {
                        Ok((dir, created)) => {
                            self.custom_commands = commands::load_custom().unwrap_or_default();
                            self.notice = Some(if created > 0 {
                                format!("已生成 {created} 个示例文件：{}", dir.display())
                            } else {
                                format!("命令模板已存在：{}", dir.display())
                            });
                            cx.reveal_path(&dir);
                        }
                        Err(error) => self.notice = Some(error),
                    }
                    cx.notify();
                }
            },
            SlashAction::Custom(command) => self.run_custom_command(command, query, window, cx),
        }
    }

    fn run_custom_command(
        &mut self,
        command: commands::CustomCommand,
        query: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.editor.read(cx).value().to_string();
        let selection = self.editor.read(cx).selected_range();
        let selected = text
            .get(selection.start..selection.end)
            .unwrap_or("")
            .to_string();
        let now = chrono::Local::now();
        let context = commands::ScriptContext {
            document: text,
            selection: selected,
            cursor: self.editor.read(cx).cursor(),
            now: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            date: now.format("%Y-%m-%d").to_string(),
            time: now.format("%H:%M:%S").to_string(),
            query,
        };
        let name = command.name.clone();
        self.notice = Some(format!("正在运行 /{name}…"));
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { commands::run(&command, &context) })
                .await;
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(output) if output.trim().is_empty() => {
                        this.notice = Some(format!("/{name} 没有输出"));
                    }
                    Ok(output) => {
                        this.insert_text(&output, window, cx);
                        this.notice = Some(format!("/{name} 已插入"));
                    }
                    Err(error) => {
                        this.notice = Some(format!("/{name}：{error}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn insert_image_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(image) = cx
            .read_from_clipboard()
            .and_then(|item| clipboard_image(&item))
        {
            self.paste_image(image, window, cx);
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("选择图片".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.insert_image_file(path, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn insert_image_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !path.is_file() {
            self.notice = Some("选择的不是图片文件".to_string());
            cx.notify();
            return;
        }
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .filter(|extension| !extension.is_empty())
        else {
            self.notice = Some("请选择带扩展名的图片文件".to_string());
            cx.notify();
            return;
        };
        if !matches!(
            extension.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp"
        ) {
            self.notice = Some("支持 PNG / JPG / GIF / WebP / SVG / BMP 图片".to_string());
            cx.notify();
            return;
        }
        let (bytes, output_extension) = if extension == "bmp" {
            // Preview and publishing targets do not take BMP; normalize it.
            match image::open(&path) {
                Ok(image) => {
                    let mut png = Vec::new();
                    if image
                        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                        .is_err()
                    {
                        self.notice = Some("BMP 转 PNG 失败".to_string());
                        cx.notify();
                        return;
                    }
                    (png, "png".to_string())
                }
                Err(error) => {
                    self.notice = Some(format!("读取 BMP 失败：{error}"));
                    cx.notify();
                    return;
                }
            }
        } else {
            match std::fs::read(&path) {
                Ok(bytes) => (bytes, extension.clone()),
                Err(error) => {
                    self.notice = Some(format!("读取图片失败：{error}"));
                    cx.notify();
                    return;
                }
            }
        };
        let assets = self.current_doc_dir().join("assets");
        if let Err(error) = std::fs::create_dir_all(&assets) {
            self.notice = Some(format!("创建 assets 目录失败：{error}"));
            cx.notify();
            return;
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut name = format!("image-{stamp}.{output_extension}");
        let mut suffix = 2;
        while assets.join(&name).exists() {
            name = format!("image-{stamp}-{suffix}.{output_extension}");
            suffix += 1;
        }
        match std::fs::write(assets.join(&name), &bytes) {
            Ok(()) => {
                let relative = format!("assets/{name}");
                self.insert_text(&format!("![]({relative})"), window, cx);
                self.notice = Some(format!("图片已保存到 {relative}"));
                cx.notify();
            }
            Err(error) => {
                self.notice = Some(format!("保存图片失败：{error}"));
                cx.notify();
            }
        }
    }

    fn jump_to(
        &mut self,
        offset: usize,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |state, cx| {
            let row = state.text().offset_to_point(offset).row;
            let line_height = state.line_height().unwrap_or(px(22.));
            state.set_selected_range(offset..offset, cx);
            let y = px((row as f32 * line_height.as_f32() - 96.).max(0.));
            let x = state.scroll_offset().x;
            state.set_scroll_offset(point(x, y), cx);
        });
        if !title.is_empty() {
            self.preview.update(cx, |state, cx| {
                let rendered = state.rendered_text();
                if let Some(position) = rendered.as_str().find(&title) {
                    let _ = state.reveal_range(position..position + title.len(), cx);
                }
            });
        }
        self.focus_editor(window, cx);
        cx.notify();
    }

    // ---- interaction ----------------------------------------------------

    fn tool<F>(
        id: &'static str,
        icon: IconName,
        tip: &'static str,
        cx: &mut Context<Self>,
        handler: F,
    ) -> Button
    where
        F: Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    {
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(icon)
            .tooltip(tip)
            .on_click(cx.listener(move |this, _, window, cx| handler(this, window, cx)))
    }

    fn menu_button(&self, id: &'static str, label: &'static str) -> Button {
        Button::new(id).ghost().xsmall().label(label)
    }

    fn file_menu(&self) -> impl IntoElement {
        self.menu_button("menu-file", "文件")
            .dropdown_menu(|menu, _, _| {
                menu.item(
                    PopupMenuItem::new("新建文档")
                        .icon(IconName::FilePlus)
                        .action(Box::new(NewDocument)),
                )
                .item(
                    PopupMenuItem::new("从模板新建…")
                        .icon(IconName::LayoutTemplate)
                        .action(Box::new(NewFromTemplate)),
                )
                .item(
                    PopupMenuItem::new("打开 Markdown…")
                        .icon(IconName::FileText)
                        .action(Box::new(OpenDocument)),
                )
                .item(
                    PopupMenuItem::new("打开文件夹…")
                        .icon(IconName::FolderOpen)
                        .action(Box::new(OpenFolder)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("保存")
                        .icon(IconName::Save)
                        .action(Box::new(SaveDocument)),
                )
                .item(
                    PopupMenuItem::new("另存为…")
                        .icon(IconName::Save)
                        .action(Box::new(SaveDocumentAs)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("导出 HTML…")
                        .icon(IconName::Download)
                        .action(Box::new(ExportHtml)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("立即备份")
                        .icon(IconName::Archive)
                        .action(Box::new(CreateBackup)),
                )
                .item(
                    PopupMenuItem::new("Git 同步")
                        .icon(IconName::GitBranch)
                        .action(Box::new(GitSync)),
                )
            })
    }

    fn edit_menu(&self) -> impl IntoElement {
        let undo_editor = self.editor.clone();
        let redo_editor = self.editor.clone();
        self.menu_button("menu-edit", "编辑")
            .dropdown_menu(move |menu, _, _| {
                let undo_editor = undo_editor.clone();
                let redo_editor = redo_editor.clone();
                menu.item(PopupMenuItem::new("撤销").icon(IconName::Undo).on_click(
                    move |_, window, cx| {
                        let focus = undo_editor.read(cx).focus_handle(cx);
                        focus.focus(window, cx);
                        window.dispatch_action(Box::new(Undo), cx);
                    },
                ))
                .item(PopupMenuItem::new("重做").icon(IconName::Redo).on_click(
                    move |_, window, cx| {
                        let focus = redo_editor.read(cx).focus_handle(cx);
                        focus.focus(window, cx);
                        window.dispatch_action(Box::new(Redo), cx);
                    },
                ))
            })
    }

    fn format_menu(&self) -> impl IntoElement {
        self.menu_button("menu-format", "格式")
            .dropdown_menu(|menu, _, _| {
                menu.item(
                    PopupMenuItem::new("一级标题")
                        .icon(IconName::Heading1)
                        .action(Box::new(FormatHeading1)),
                )
                .item(
                    PopupMenuItem::new("二级标题")
                        .icon(IconName::Heading2)
                        .action(Box::new(FormatHeading2)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("加粗")
                        .icon(IconName::Bold)
                        .action(Box::new(FormatBold)),
                )
                .item(
                    PopupMenuItem::new("斜体")
                        .icon(IconName::Italic)
                        .action(Box::new(FormatItalic)),
                )
                .item(
                    PopupMenuItem::new("删除线")
                        .icon(IconName::Strikethrough)
                        .action(Box::new(FormatStrike)),
                )
                .item(
                    PopupMenuItem::new("行内代码")
                        .icon(IconName::Code)
                        .action(Box::new(FormatInlineCode)),
                )
                .item(
                    PopupMenuItem::new("代码块")
                        .icon(IconName::Code)
                        .action(Box::new(FormatCodeBlock)),
                )
                .item(
                    PopupMenuItem::new("链接")
                        .icon(IconName::Link)
                        .action(Box::new(FormatLink)),
                )
                .item(
                    PopupMenuItem::new("插入图片")
                        .icon(IconName::Image)
                        .action(Box::new(FormatImage)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("文档元数据…")
                        .icon(IconName::Type)
                        .action(Box::new(EditMetadata)),
                )
                .separator()
                .item(
                    PopupMenuItem::new("引用")
                        .icon(IconName::Quote)
                        .action(Box::new(FormatQuote)),
                )
                .item(
                    PopupMenuItem::new("无序列表")
                        .icon(IconName::List)
                        .action(Box::new(FormatBulletList)),
                )
                .item(
                    PopupMenuItem::new("有序列表")
                        .icon(IconName::ListOrdered)
                        .action(Box::new(FormatOrderedList)),
                )
                .item(
                    PopupMenuItem::new("表格")
                        .icon(IconName::Table)
                        .action(Box::new(FormatTable)),
                )
                .item(
                    PopupMenuItem::new("分隔线")
                        .icon(IconName::Minus)
                        .action(Box::new(FormatDivider)),
                )
            })
    }

    fn view_menu(&self, dark: bool) -> impl IntoElement {
        let write = self.view_mode == ViewMode::Write;
        let split = self.view_mode == ViewMode::Split;
        let preview = self.view_mode == ViewMode::Preview;
        let sidebar = self.sidebar_visible;
        self.menu_button("menu-view", "视图")
            .dropdown_menu(move |menu, _, _| {
                menu.item(
                    PopupMenuItem::new("写作视图")
                        .checked(write)
                        .action(Box::new(ViewWrite)),
                )
                .item(
                    PopupMenuItem::new("分栏视图")
                        .checked(split)
                        .action(Box::new(ViewSplit)),
                )
                .item(
                    PopupMenuItem::new("预览视图")
                        .checked(preview)
                        .action(Box::new(ViewPreview)),
                )
                .separator()
                .item(PopupMenuItem::new("在预览中查找").action(Box::new(PreviewFind)))
                .item(
                    PopupMenuItem::new("显示侧边栏")
                        .checked(sidebar)
                        .action(Box::new(ToggleSidebar)),
                )
                .separator()
                .item(
                    PopupMenuItem::new(if dark {
                        "切换到浅色"
                    } else {
                        "切换到深色"
                    })
                    .icon(if dark { IconName::Sun } else { IconName::Moon })
                    .action(Box::new(ToggleTheme)),
                )
            })
    }

    // ---- rendering ------------------------------------------------------

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let doc = &self.docs[self.active];
        let title = doc.title.clone();
        let dirty = doc.is_dirty();
        let accent = cx.theme().accent_foreground;

        TitleBar::new().child(
            h_flex()
                .w_full()
                .h_full()
                .pr_2()
                .gap_1()
                .items_center()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_shrink_0()
                        .child(widgets::logo())
                        .child(div().text_sm().font_semibold().child("Inkdown")),
                )
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .justify_center()
                        .items_center()
                        .gap_2()
                        .child(
                            Icon::new(IconName::FileText)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .text_sm()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(title),
                        )
                        .when(dirty, |this| this.child(widgets::dot(accent))),
                ),
        )
    }

    fn menu_bar(&self, dark: bool, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(30.))
            .w_full()
            .flex_shrink_0()
            .px_2()
            .gap_1()
            .items_center()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.file_menu())
            .child(self.edit_menu())
            .child(self.format_menu())
            .child(self.view_menu(dark))
            .child(widgets::hairline(cx))
            .child(
                Button::new("publish-open")
                    .primary()
                    .xsmall()
                    .icon(IconName::Send)
                    .label("发布")
                    .tooltip("发布到公众号 / 知乎 / 头条 / CSDN 等 · Ctrl+Shift+P")
                    .on_click(cx.listener(|this, _, window, cx| this.open_publish(window, cx))),
            )
            .child(
                Button::new("ai-write")
                    .outline()
                    .xsmall()
                    .icon(IconName::Sparkles)
                    .label("AI 写作")
                    .tooltip("用 AI 生成文章 · Ctrl+Shift+A")
                    .on_click(cx.listener(|this, _, window, cx| this.open_ai(window, cx))),
            )
            .child(
                Button::new("ai-chat")
                    .outline()
                    .xsmall()
                    .icon(IconName::BotMessageSquare)
                    .label("询问 AI")
                    .tooltip("打开 AI 对话窗口，回复可以插入文稿")
                    .on_click(cx.listener(|this, _, window, cx| this.open_chat(window, cx))),
            )
            .child(widgets::hairline(cx))
            .child(
                Button::new("menu-backup")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Archive)
                    .tooltip("立即备份为 zip")
                    .on_click(cx.listener(|this, _, window, cx| this.backup_now(window, cx))),
            )
            .child(
                Button::new("menu-git")
                    .ghost()
                    .xsmall()
                    .icon(IconName::GitBranch)
                    .tooltip("Git 同步 · Ctrl+Shift+G")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.git_sync_from_config(window, cx)),
                    ),
            )
            .child(div().flex_1())
            .when_some(self.action_busy.clone(), |this, busy| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().accent_foreground)
                        .child(busy),
                )
            })
            .child(
                Button::new("open-settings")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Settings)
                    .tooltip("设置")
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
            )
            .child(
                Button::new("theme-toggle")
                    .ghost()
                    .xsmall()
                    .icon(if dark { IconName::Sun } else { IconName::Moon })
                    .tooltip(if dark {
                        "切换到浅色"
                    } else {
                        "切换到深色"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx))),
            )
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .h(px(48.))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .gap_1()
            .items_center()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Self::tool(
                    "toggle-sidebar",
                    IconName::PanelLeft,
                    "显示/隐藏侧边栏",
                    cx,
                    |this, window, cx| this.toggle_sidebar(window, cx),
                )
                .selected(self.sidebar_visible),
            )
            .child(widgets::hairline(cx))
            .child(Self::tool(
                "toolbar-new",
                IconName::FilePlus,
                "新建文档",
                cx,
                |this, window, cx| this.new_document(window, cx),
            ))
            .child(Self::tool(
                "toolbar-template",
                IconName::LayoutTemplate,
                "从模板新建 · Ctrl+Shift+N",
                cx,
                |this, _, cx| {
                    this.slash = None;
                    this.templates_open = true;
                    cx.notify();
                },
            ))
            .child(Self::tool(
                "toolbar-open",
                IconName::FolderOpen,
                "打开 Markdown",
                cx,
                |this, window, cx| this.open_document(window, cx),
            ))
            .child(Self::tool(
                "toolbar-save",
                IconName::Save,
                "保存 · Ctrl+S",
                cx,
                |this, window, cx| this.save(window, cx),
            ))
            .child(widgets::hairline(cx))
            .child(Self::tool(
                "toolbar-undo",
                IconName::Undo,
                "撤销",
                cx,
                |this, window, cx| this.dispatch_editor_action(Box::new(Undo), window, cx),
            ))
            .child(Self::tool(
                "toolbar-redo",
                IconName::Redo,
                "重做",
                cx,
                |this, window, cx| this.dispatch_editor_action(Box::new(Redo), window, cx),
            ))
            .child(widgets::hairline(cx))
            .child(Self::tool(
                "toolbar-h1",
                IconName::Heading1,
                "一级标题",
                cx,
                |this, window, cx| this.set_heading(1, window, cx),
            ))
            .child(Self::tool(
                "toolbar-h2",
                IconName::Heading2,
                "二级标题",
                cx,
                |this, window, cx| this.set_heading(2, window, cx),
            ))
            .child(Self::tool(
                "toolbar-bold",
                IconName::Bold,
                "加粗 · Ctrl+B",
                cx,
                |this, window, cx| this.wrap_selection("**", "**", "粗体", window, cx),
            ))
            .child(Self::tool(
                "toolbar-italic",
                IconName::Italic,
                "斜体 · Ctrl+I",
                cx,
                |this, window, cx| this.wrap_selection("*", "*", "斜体", window, cx),
            ))
            .child(Self::tool(
                "toolbar-strike",
                IconName::Strikethrough,
                "删除线",
                cx,
                |this, window, cx| this.wrap_selection("~~", "~~", "删除线", window, cx),
            ))
            .child(Self::tool(
                "toolbar-code",
                IconName::Code,
                "行内代码 · Ctrl+E",
                cx,
                |this, window, cx| this.wrap_selection("`", "`", "代码", window, cx),
            ))
            .child(Self::tool(
                "toolbar-link",
                IconName::Link,
                "插入链接 · Ctrl+K",
                cx,
                |this, window, cx| this.insert_link(window, cx),
            ))
            .child(widgets::hairline(cx))
            .child(Self::tool(
                "toolbar-quote",
                IconName::Quote,
                "引用",
                cx,
                |this, window, cx| this.toggle_line_prefix("> ", window, cx),
            ))
            .child(Self::tool(
                "toolbar-list",
                IconName::List,
                "无序列表",
                cx,
                |this, window, cx| this.toggle_line_prefix("- ", window, cx),
            ))
            .child(Self::tool(
                "toolbar-ordered",
                IconName::ListOrdered,
                "有序列表",
                cx,
                |this, window, cx| this.toggle_line_prefix("1. ", window, cx),
            ))
            .child(Self::tool(
                "toolbar-table",
                IconName::Table,
                "插入表格",
                cx,
                |this, window, cx| this.insert_template(TABLE_TEMPLATE, window, cx),
            ))
            .child(div().flex_1())
            .child(self.view_switcher(cx))
    }

    fn view_switcher(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_1()
            .p_1()
            .rounded_md()
            .bg(cx.theme().muted)
            .child(
                Button::new("view-write")
                    .ghost()
                    .xsmall()
                    .icon(IconName::PenLine)
                    .tooltip("写作视图")
                    .selected(self.view_mode == ViewMode::Write)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_view(ViewMode::Write, window, cx)
                    })),
            )
            .child(
                Button::new("view-split")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Columns2)
                    .tooltip("分栏视图")
                    .selected(self.view_mode == ViewMode::Split)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_view(ViewMode::Split, window, cx)
                    })),
            )
            .child(
                Button::new("view-preview")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Eye)
                    .tooltip("预览视图")
                    .selected(self.view_mode == ViewMode::Preview)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.set_view(ViewMode::Preview, window, cx)
                    })),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor_offset = self.editor.read(cx).cursor();
        let active_heading = self
            .outline
            .iter()
            .enumerate()
            .filter(|(_, heading)| heading.offset <= cursor_offset)
            .map(|(index, _)| index)
            .last();

        v_flex()
            .w(px(236.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().sidebar_border)
            .child(
                h_flex()
                    .h(px(44.))
                    .px_3()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(self.sidebar_tab_button("文稿", SidebarTab::Documents, cx))
                            .child(self.sidebar_tab_button("文件", SidebarTab::Files, cx)),
                    )
                    .child(self.sidebar_actions(cx)),
            )
            .child(match self.sidebar_tab {
                SidebarTab::Documents => self.documents_list(cx),
                SidebarTab::Files => self.files_list(cx),
            })
            .child(
                div()
                    .mx_3()
                    .my_2()
                    .h(px(1.))
                    .flex_shrink_0()
                    .bg(cx.theme().sidebar_border),
            )
            .child(
                h_flex()
                    .h(px(34.))
                    .px_3()
                    .pb_1()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("大纲"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{}", self.outline.len())),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("outline-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .child(if self.outline.is_empty() {
                        div()
                            .px_2()
                            .py_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("用 # 写下第一个标题")
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_1()
                            .children(
                                self.outline
                                    .iter()
                                    .enumerate()
                                    .map(|(index, heading)| {
                                        self.outline_item(
                                            heading,
                                            active_heading == Some(index),
                                            cx,
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .into_any_element()
                    }),
            )
            .child(self.sidebar_footer(cx))
    }

    fn sidebar_tab_button(
        &self,
        label: &'static str,
        tab: SidebarTab,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(label)
            .ghost()
            .xsmall()
            .label(label)
            .selected(self.sidebar_tab == tab)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.sidebar_tab = tab;
                cx.notify();
            }))
    }

    fn sidebar_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_1()
            .when(self.sidebar_tab == SidebarTab::Documents, |this| {
                this.child(
                    Button::new("sidebar-new")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Plus)
                        .tooltip("新建文档")
                        .on_click(cx.listener(|this, _, window, cx| this.new_document(window, cx))),
                )
                .child(
                    Button::new("sidebar-template")
                        .ghost()
                        .xsmall()
                        .icon(IconName::LayoutTemplate)
                        .tooltip("从模板新建")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.slash = None;
                            this.templates_open = true;
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("sidebar-open")
                        .ghost()
                        .xsmall()
                        .icon(IconName::FileText)
                        .tooltip("打开 Markdown")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_document(window, cx)),
                        ),
                )
            })
            .when(self.sidebar_tab == SidebarTab::Files, |this| {
                this.child(
                    Button::new("sidebar-open-folder")
                        .ghost()
                        .xsmall()
                        .icon(IconName::FolderOpen)
                        .tooltip("打开文件夹")
                        .on_click(cx.listener(|this, _, window, cx| this.open_folder(window, cx))),
                )
                .child(
                    Button::new("sidebar-files-menu")
                        .ghost()
                        .xsmall()
                        .icon(IconName::Ellipsis)
                        .tooltip("文件操作")
                        .dropdown_menu(|menu, _, _| {
                            menu.item(
                                PopupMenuItem::new("新建文件")
                                    .icon(IconName::FilePlus)
                                    .action(Box::new(NewFile)),
                            )
                            .item(
                                PopupMenuItem::new("新建文件夹")
                                    .icon(IconName::FolderPlus)
                                    .action(Box::new(NewFolder)),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("重命名")
                                    .icon(IconName::Pencil)
                                    .action(Box::new(RenameFile)),
                            )
                            .item(
                                PopupMenuItem::new("删除")
                                    .icon(IconName::Trash)
                                    .action(Box::new(DeleteFile)),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("刷新")
                                    .icon(IconName::RefreshCw)
                                    .action(Box::new(RefreshFolder)),
                            )
                            .item(
                                PopupMenuItem::new("在文件管理器中显示")
                                    .icon(IconName::ExternalLink)
                                    .action(Box::new(RevealInFiles)),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    fn documents_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let open: HashSet<PathBuf> = self
            .docs
            .iter()
            .filter_map(|doc| doc.path.clone())
            .collect();
        let recent = self
            .recent
            .iter()
            .filter(|path| !open.contains(*path))
            .take(5)
            .cloned()
            .collect::<Vec<_>>();

        v_flex()
            .id("doc-list")
            .flex_shrink_0()
            .max_h(px(238.))
            .overflow_y_scroll()
            .px_2()
            .gap_1()
            .children(
                (0..self.docs.len())
                    .map(|index| self.doc_item(index, cx))
                    .collect::<Vec<_>>(),
            )
            .when(!recent.is_empty(), |this| {
                this.child(
                    h_flex()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .gap_1()
                        .items_center()
                        .child(
                            Icon::new(IconName::FileClock)
                                .with_size(px(11.))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("最近"),
                        ),
                )
                .children(
                    recent
                        .into_iter()
                        .enumerate()
                        .map(|(index, path)| self.recent_item(index, path, cx))
                        .collect::<Vec<_>>(),
                )
            })
            .into_any_element()
    }

    fn files_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tree) = self.workspace.as_ref() else {
            return v_flex()
                .px_3()
                .py_3()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("打开一个本地文件夹，按目录管理 Markdown 文稿。"),
                )
                .child(
                    Button::new("files-open-empty")
                        .outline()
                        .xsmall()
                        .icon(IconName::FolderOpen)
                        .label("打开文件夹")
                        .on_click(cx.listener(|this, _, window, cx| this.open_folder(window, cx))),
                )
                .into_any_element();
        };

        let root_name = tree
            .root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace")
            .to_string();
        let visible = tree.visible(&self.expanded);
        let mut rows = Vec::new();
        if visible.is_empty() {
            rows.push(
                v_flex()
                    .px_2()
                    .py_2()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("这个文件夹里还没有 Markdown 文件"),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("folder-empty-new-file")
                                    .outline()
                                    .xsmall()
                                    .icon(IconName::FilePlus)
                                    .label("新建文件")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(dir) = this.prompt_target_dir() {
                                            this.open_prompt(PromptKind::NewFile(dir), window, cx);
                                        }
                                    })),
                            )
                            .child(
                                Button::new("folder-empty-new-folder")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::FolderPlus)
                                    .label("新建文件夹")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(dir) = this.prompt_target_dir() {
                                            this.open_prompt(
                                                PromptKind::NewFolder(dir),
                                                window,
                                                cx,
                                            );
                                        }
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        } else {
            for (index, node) in visible.into_iter().enumerate() {
                rows.push(self.file_row(index, node, cx));
            }
        }

        v_flex()
            .id("file-list")
            .flex_shrink_0()
            .max_h(px(300.))
            .overflow_y_scroll()
            .px_2()
            .gap_1()
            .child(
                h_flex()
                    .px_1()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::Folder).xsmall())
                    .child(div().flex_1().min_w_0().truncate().child(root_name)),
            )
            .children(rows)
            .into_any_element()
    }

    fn file_row(
        &self,
        index: usize,
        node: workspace::FlatNode,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = !node.is_dir && self.selected_path.as_deref() == Some(node.path.as_path());
        let path = node.path.clone();
        let is_dir = node.is_dir;
        let name = node.name.clone();
        let depth = node.depth;
        let expanded = node.expanded;
        let icon_color = if active {
            cx.theme().accent_foreground
        } else {
            cx.theme().muted_foreground
        };

        div()
            .id(("file-row", index))
            .w_full()
            .pl(px(6.0 + depth as f32 * 12.0))
            .pr_2()
            .py(px(4.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_1()
            .cursor(CursorStyle::PointingHand)
            .when(active, |this| this.bg(cx.theme().sidebar_accent))
            .when(!active, |this| {
                this.hover(|style| style.bg(cx.theme().sidebar_accent.opacity(0.5)))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected_path = Some(path.clone());
                if is_dir {
                    if !this.expanded.remove(&path) {
                        this.expanded.insert(path.clone());
                    }
                    cx.notify();
                } else {
                    this.open_workspace_file(path.clone(), window, cx);
                }
            }))
            .child(
                div()
                    .size(px(14.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(node.has_children, |this| {
                        this.child(if expanded {
                            Icon::new(IconName::ChevronDown).with_size(px(12.))
                        } else {
                            Icon::new(IconName::ChevronRight).with_size(px(12.))
                        })
                    }),
            )
            .child(
                Icon::new(if is_dir {
                    if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    }
                } else {
                    IconName::FileText
                })
                .xsmall()
                .text_color(icon_color),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if active {
                        cx.theme().accent_foreground
                    } else {
                        cx.theme().sidebar_foreground
                    })
                    .child(name),
            )
            .into_any_element()
    }

    // ---- folders -------------------------------------------------------

    fn open_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择文稿文件夹".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                this.update_in(cx, |this, window, cx| {
                    this.open_folder_at(path, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn open_folder_at(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !path.is_dir() {
            self.notice = Some("选择的路径不是文件夹".to_string());
            cx.notify();
            return;
        }
        self.workspace = Some(workspace::FileTree::open(path.clone()));
        self.expanded.clear();
        self.expanded.insert(path.clone());
        self.selected_path = None;
        self.sidebar_tab = SidebarTab::Files;
        self.sidebar_visible = true;
        cx.global_mut::<config::AppConfig>().last_folder = Some(path.to_string_lossy().to_string());
        config::save(cx).ok();
        self.notice = Some(format!("已打开文件夹 {}", path.display()));
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(tree) = self.workspace.as_mut() {
            tree.refresh();
        }
        cx.notify();
    }

    fn open_workspace_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.selected_path = Some(path.clone());
                self.sidebar_tab = SidebarTab::Files;
                self.open_path(path.clone(), content, window, cx);
                if let Some(root) = self.workspace.as_ref().map(|tree| tree.root.clone()) {
                    workspace::expand_ancestors(&root, &path, &mut self.expanded);
                }
                cx.notify();
            }
            Err(error) => {
                self.notice = Some(format!("打开文件失败：{error}"));
                cx.notify();
            }
        }
    }

    fn current_doc_dir(&self) -> PathBuf {
        self.docs[self.active]
            .path
            .as_ref()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
            .or_else(|| self.workspace.as_ref().map(|tree| tree.root.clone()))
            .or_else(config::data_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn prompt_target_dir(&self) -> Option<PathBuf> {
        let root = self.workspace.as_ref().map(|tree| tree.root.clone())?;
        Some(workspace::target_dir(&root, self.selected_path.as_deref()))
    }

    fn open_prompt(&mut self, kind: PromptKind, window: &mut Window, cx: &mut Context<Self>) {
        self.slash = None;
        let (title, hint, needs_input) = ui::prompt_title(&kind);
        let (default, placeholder) = match &kind {
            PromptKind::NewFile(dir) => (
                workspace::unique_markdown_path(dir, "新文稿")
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("新文稿")
                    .to_string(),
                "文件名",
            ),
            PromptKind::NewFolder(dir) => (
                workspace::unique_dir_path(dir, "新文件夹")
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("新文件夹")
                    .to_string(),
                "文件夹名称",
            ),
            PromptKind::Rename(path) => (
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_string(),
                "新名称",
            ),
            PromptKind::Delete(_) => (String::new(), ""),
        };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(default)
        });
        if needs_input {
            let focus = input.read(cx).focus_handle(cx);
            window.defer(cx, move |window, cx| focus.focus(window, cx));
        }
        self.prompt = Some(PromptState {
            kind,
            title: title.into(),
            hint: hint.into(),
            input,
            needs_input,
            error: None,
        });
        cx.notify();
    }

    fn apply_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut prompt) = self.prompt.take() else {
            return;
        };
        let value = prompt.input.read(cx).value().to_string();
        let kind = prompt.kind.clone();

        let result: Result<Option<PathBuf>, String> = match kind {
            PromptKind::NewFile(dir) => {
                let name = match workspace::validate_name(&value) {
                    Ok(name) => name,
                    Err(error) => {
                        prompt.error = Some(error);
                        self.prompt = Some(prompt);
                        cx.notify();
                        return;
                    }
                };
                let name = if name.to_ascii_lowercase().ends_with(".md") {
                    name
                } else {
                    format!("{name}.md")
                };
                let path = dir.join(name);
                if path.exists() {
                    prompt.error = Some("同名文件已经存在".to_string());
                    self.prompt = Some(prompt);
                    cx.notify();
                    return;
                }
                match std::fs::write(&path, "") {
                    Ok(()) => {
                        self.expanded.insert(dir.clone());
                        Ok(Some(path))
                    }
                    Err(error) => Err(format!("创建文件失败：{error}")),
                }
            }
            PromptKind::NewFolder(dir) => {
                let name = match workspace::validate_name(&value) {
                    Ok(name) => name,
                    Err(error) => {
                        prompt.error = Some(error);
                        self.prompt = Some(prompt);
                        cx.notify();
                        return;
                    }
                };
                let path = dir.join(name);
                if path.exists() {
                    prompt.error = Some("同名文件夹已经存在".to_string());
                    self.prompt = Some(prompt);
                    cx.notify();
                    return;
                }
                match std::fs::create_dir_all(&path) {
                    Ok(()) => {
                        self.expanded.insert(path.clone());
                        self.expanded.insert(dir);
                        self.selected_path = Some(path);
                        Ok(None)
                    }
                    Err(error) => Err(format!("创建文件夹失败：{error}")),
                }
            }
            PromptKind::Rename(path) => {
                let name = match workspace::validate_name(&value) {
                    Ok(name) => name,
                    Err(error) => {
                        prompt.error = Some(error);
                        self.prompt = Some(prompt);
                        cx.notify();
                        return;
                    }
                };
                let was_dir = path.is_dir();
                let mut final_name = name;
                if !was_dir && Path::new(&final_name).extension().is_none() {
                    if let Some(extension) = path.extension() {
                        final_name = format!("{}.{}", final_name, extension.to_string_lossy());
                    }
                }
                let target = path
                    .parent()
                    .map(|parent| parent.join(&final_name))
                    .unwrap_or_else(|| PathBuf::from(&final_name));
                if target.exists() {
                    prompt.error = Some("目标名称已经存在".to_string());
                    self.prompt = Some(prompt);
                    cx.notify();
                    return;
                }
                match std::fs::rename(&path, &target) {
                    Ok(()) => {
                        for doc in self.docs.iter_mut() {
                            let Some(doc_path) = doc.path.as_ref() else {
                                continue;
                            };
                            if doc_path == &path {
                                doc.path = Some(target.clone());
                                doc.title = target
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("未命名.md")
                                    .into();
                            } else if was_dir {
                                if let Ok(relative) = doc_path.strip_prefix(&path) {
                                    let new_path = target.join(relative);
                                    doc.path = Some(new_path.clone());
                                    doc.title = new_path
                                        .file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("未命名.md")
                                        .into();
                                }
                            }
                        }
                        self.selected_path = Some(target.clone());
                        Ok(if was_dir { None } else { Some(target) })
                    }
                    Err(error) => Err(format!("重命名失败：{error}")),
                }
            }
            PromptKind::Delete(path) => {
                let was_dir = path.is_dir();
                let removal = if was_dir {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                match removal {
                    Ok(()) => {
                        let active_id = self.docs.get(self.active).map(|doc| doc.id);
                        self.docs.retain(|doc| {
                            let Some(doc_path) = doc.path.as_ref() else {
                                return true;
                            };
                            !(doc_path == &path || (was_dir && doc_path.starts_with(&path)))
                        });
                        if self.docs.is_empty() {
                            self.next_id += 1;
                            self.docs.push(Document::untitled(self.next_id - 1, 1));
                        }
                        self.active = active_id
                            .and_then(|id| self.docs.iter().position(|doc| doc.id == id))
                            .unwrap_or(0);
                        self.selected_path = None;
                        self.reload_current_doc(window, cx);
                        Ok(None)
                    }
                    Err(error) => Err(format!("删除失败：{error}")),
                }
            }
        };

        match result {
            Ok(path) => {
                self.refresh_tree(cx);
                if let Some(path) = path {
                    self.open_workspace_file(path, window, cx);
                }
                self.notice = Some("文件已更新".to_string());
                cx.notify();
            }
            Err(error) => {
                prompt.error = Some(error);
                self.prompt = Some(prompt);
                cx.notify();
            }
        }
    }

    fn reload_current_doc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.docs[self.active].content.clone();
        self.editor.update(cx, |state, cx| {
            state.set_value(&content, window, cx);
            state.set_selected_range(0..0, cx);
            state.set_scroll_offset(point(px(0.), px(0.)), cx);
        });
        self.preview
            .update(cx, |state, cx| state.set_text(&content, cx));
        self.outline = doc::outline(&content);
        self.stats = doc::stats(&content);
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn reveal_selected(&mut self, cx: &mut Context<Self>) {
        let path = self
            .selected_path
            .clone()
            .or_else(|| self.workspace.as_ref().map(|tree| tree.root.clone()))
            .or_else(|| self.docs[self.active].path.clone());
        match path {
            Some(path) => cx.reveal_path(&path),
            None => {
                self.notice = Some("还没有可显示的文件".to_string());
                cx.notify();
            }
        }
    }

    fn open_dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
        let mut opened = 0usize;
        for path in paths.paths() {
            if path.is_dir() {
                self.open_folder_at(path.clone(), window, cx);
                opened += 1;
            } else if workspace::is_markdown(path)
                && let Ok(content) = std::fs::read_to_string(path)
            {
                self.open_path(path.clone(), content, window, cx);
                opened += 1;
            }
        }
        if opened == 0 {
            self.notice = Some("拖入的内容里没有 Markdown 文件".to_string());
            cx.notify();
        }
    }

    /// Save a clipboard image next to the document and insert its Markdown.
    ///
    /// Clipboard images often arrive as BMP (screenshot tools), which neither
    /// the preview nor the publishing targets accept; those are re-encoded as
    /// PNG before they are written.
    fn paste_image(&mut self, image: Image, window: &mut Window, cx: &mut Context<Self>) {
        let (bytes, extension) = match image.format {
            ImageFormat::Png => (image.bytes.clone(), "png"),
            ImageFormat::Jpeg => (image.bytes.clone(), "jpg"),
            ImageFormat::Webp => (image.bytes.clone(), "webp"),
            ImageFormat::Gif => (image.bytes.clone(), "gif"),
            ImageFormat::Svg => (image.bytes.clone(), "svg"),
            _ => match image::load_from_memory(&image.bytes) {
                Ok(dynamic) => {
                    let mut png = Vec::new();
                    if dynamic
                        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                        .is_err()
                    {
                        self.notice = Some("图片转码失败".to_string());
                        cx.notify();
                        return;
                    }
                    (png, "png")
                }
                Err(error) => {
                    self.notice = Some(format!("无法读取剪贴板图片：{error}"));
                    cx.notify();
                    return;
                }
            },
        };
        let assets = self.current_doc_dir().join("assets");
        if let Err(error) = std::fs::create_dir_all(&assets) {
            self.notice = Some(format!("创建 assets 目录失败：{error}"));
            cx.notify();
            return;
        }
        let name = format!(
            "pasted-{}.{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S"),
            extension
        );
        let path = assets.join(&name);
        if let Err(error) = std::fs::write(&path, &bytes) {
            self.notice = Some(format!("保存图片失败：{error}"));
            cx.notify();
            return;
        }

        let relative = format!("assets/{name}");
        let host = cx.global::<config::AppConfig>().image_host.clone();
        if !(host.upload_on_paste && host.kind != config::ImageHostKind::None) {
            self.editor.update(cx, |state, cx| {
                state.insert(format!("![]({relative})"), window, cx);
            });
            self.after_edit(window, cx);
            self.notice = Some(format!("图片已保存到 {relative}"));
            cx.notify();
            return;
        }

        self.notice = Some("图片已保存，正在上传图床…".to_string());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { imagebed::upload_file(&host, &path) })
                .await;
            this.update_in(cx, |this, window, cx| {
                let markdown = match &result {
                    Ok(url) => format!("![]({url})"),
                    Err(_) => format!("![]({relative})"),
                };
                this.editor
                    .update(cx, |state, cx| state.insert(markdown, window, cx));
                this.after_edit(window, cx);
                this.notice = Some(match result {
                    Ok(url) => format!("图片已上传：{url}"),
                    Err(error) => format!("上传失败，使用本地图片：{error}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn on_edit_metadata(&mut self, _: &EditMetadata, window: &mut Window, cx: &mut Context<Self>) {
        self.open_metadata(window, cx);
    }

    fn on_preview_find(&mut self, _: &PreviewFind, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open
            || self.publish_open
            || self.ai_open
            || self.chat_open
            || self.prompt.is_some()
            || self.close_confirm.is_some()
            || self.meta_open
            || self.templates_open
            || self.code_lang.is_some()
            || self.slash.is_some()
        {
            return;
        }
        self.toggle_preview_find(window, cx);
    }

    fn slash_up(&mut self, cx: &mut Context<Self>) {
        let count = self.slash_entries().len();
        if count == 0 {
            return;
        }
        if let Some(state) = self.slash.as_mut() {
            let current = state.selected.min(count - 1);
            state.selected = if current == 0 { count - 1 } else { current - 1 };
            cx.notify();
        }
    }

    fn slash_down(&mut self, cx: &mut Context<Self>) {
        let count = self.slash_entries().len();
        if count == 0 {
            return;
        }
        if let Some(state) = self.slash.as_mut() {
            state.selected = (state.selected.min(count - 1) + 1) % count;
            cx.notify();
        }
    }

    fn slash_close(&mut self, cx: &mut Context<Self>) {
        if self.slash.take().is_some() {
            cx.notify();
        }
    }

    fn on_format_image(&mut self, _: &FormatImage, window: &mut Window, cx: &mut Context<Self>) {
        let image = cx
            .read_from_clipboard()
            .and_then(|item| clipboard_image(&item));
        match image {
            Some(image) => self.paste_image(image, window, cx),
            None => {
                self.notice = Some("剪贴板里没有图片".to_string());
                cx.notify();
            }
        }
    }

    // ---- preview find ---------------------------------------------------

    fn toggle_preview_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.preview_find_open {
            self.close_preview_find(window, cx);
            return;
        }
        if self.view_mode == ViewMode::Write {
            self.view_mode = ViewMode::Split;
        }
        self.preview_find_open = true;
        let focus = self.preview_find.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        self.refresh_preview_find(true, cx);
        cx.notify();
    }

    fn close_preview_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.preview_find_open {
            return;
        }
        self.preview_find_open = false;
        self.preview_matches.clear();
        self.preview_current = 0;
        self.preview
            .update(cx, |state, cx| state.clear_range_highlights(cx));
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn refresh_preview_find(&mut self, reveal: bool, cx: &mut Context<Self>) {
        if !self.preview_find_open {
            return;
        }
        let query = self.preview_find.read(cx).value().to_string();
        let rendered = self.preview.read(cx).rendered_text();
        let text = rendered.as_str();
        self.preview_matches = if query.is_empty() {
            Vec::new()
        } else {
            text.match_indices(query.as_str())
                .map(|(start, _)| start..start + query.len())
                .collect()
        };
        self.preview_current = 0;
        self.paint_preview_find(reveal, cx);
    }

    fn paint_preview_find(&mut self, reveal: bool, cx: &mut Context<Self>) {
        let strong = cx.theme().warning;
        let weak = cx.theme().warning.opacity(0.35);
        let highlights = self
            .preview_matches
            .iter()
            .enumerate()
            .map(|(index, range)| {
                RangeHighlight::new(
                    range.clone(),
                    if index == self.preview_current {
                        strong
                    } else {
                        weak
                    },
                )
            })
            .collect::<Vec<_>>();
        let current = self.preview_matches.get(self.preview_current).cloned();
        self.preview.update(cx, |state, cx| {
            let _ = state.set_range_highlights(highlights, cx);
            if reveal && let Some(range) = current {
                let _ = state.reveal_range(range, cx);
            }
        });
        cx.notify();
    }

    fn preview_find_next(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.preview_matches.len();
        if count == 0 {
            return;
        }
        self.preview_current = if forward {
            (self.preview_current + 1) % count
        } else {
            (self.preview_current + count - 1) % count
        };
        self.paint_preview_find(true, cx);
    }

    fn preview_find_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.preview_matches.len();
        let current = if count == 0 {
            0
        } else {
            self.preview_current + 1
        };
        h_flex()
            .h(px(38.))
            .flex_shrink_0()
            .px_3()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted.opacity(0.5))
            .child(Input::new(&self.preview_find).xsmall().flex_1())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{current}/{count}")),
            )
            .child(
                Button::new("preview-find-prev")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronUp)
                    .tooltip("上一个")
                    .on_click(cx.listener(|this, _, _, cx| this.preview_find_next(false, cx))),
            )
            .child(
                Button::new("preview-find-next")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ChevronDown)
                    .tooltip("下一个")
                    .on_click(cx.listener(|this, _, _, cx| this.preview_find_next(true, cx))),
            )
            .child(
                Button::new("preview-find-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::X)
                    .tooltip("关闭查找")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_preview_find(window, cx)),
                    ),
            )
    }

    // ---- document metadata ----------------------------------------------

    fn open_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.slash = None;
        let source = self.editor.read(cx).value().to_string();
        let mut pairs = Vec::new();
        for field in doc::frontmatter_fields(&source) {
            let key = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(field.key)
                    .placeholder("字段名")
            });
            let value = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(field.value)
                    .placeholder("值")
            });
            pairs.push((key, value));
        }
        if pairs.is_empty() {
            let key = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value("title")
                    .placeholder("字段名")
            });
            let value = cx.new(|cx| InputState::new(window, cx).placeholder("值"));
            pairs.push((key, value));
        }
        self.meta_fields = pairs;
        self.meta_open = true;
        cx.notify();
    }

    fn add_meta_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = cx.new(|cx| InputState::new(window, cx).placeholder("字段名"));
        let value = cx.new(|cx| InputState::new(window, cx).placeholder("值"));
        self.meta_fields.push((key, value));
        cx.notify();
    }

    fn save_metadata(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fields: Vec<(String, String)> = self
            .meta_fields
            .iter()
            .filter_map(|(key, value)| {
                let key = key.read(cx).value().trim().to_string();
                (!key.is_empty()).then(|| (key, value.read(cx).value().to_string()))
            })
            .collect();
        let source = self.editor.read(cx).value().to_string();
        let updated = doc::set_frontmatter_fields(&source, &fields);
        if updated != source {
            self.editor
                .update(cx, |state, cx| state.set_value(&updated, window, cx));
            self.sync_editor(cx);
            self.notice = Some("元数据已更新".to_string());
        }
        self.meta_open = false;
        cx.notify();
    }

    fn meta_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        self.modal_shell("meta-overlay", cx)
            .child(
                v_flex()
                    .w(px(620.))
                    .h(relative(0.8))
                    .max_h(px(560.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .overflow_hidden()
                    .occlude()
                    .child(
                        h_flex()
                            .h(px(56.))
                            .flex_shrink_0()
                            .px_5()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .gap_0()
                                    .child(div().text_base().font_semibold().child("文档元数据"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("写入文档开头的 YAML frontmatter，留空的字段不会保存"),
                                    ),
                            )
                            .child(
                                Button::new("meta-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::X)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.meta_open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("meta-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_5()
                            .gap_2()
                            .children(
                                (0..self.meta_fields.len())
                                    .map(|index| {
                                        let (key, value) = &self.meta_fields[index];
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(Input::new(key).xsmall().w(px(170.)))
                                            .child(Input::new(value).xsmall().flex_1())
                                            .child(
                                                Button::new(("meta-remove", index))
                                                    .ghost()
                                                    .xsmall()
                                                    .icon(IconName::Trash)
                                                    .tooltip("删除字段")
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            if index < this.meta_fields.len() {
                                                                this.meta_fields.remove(index);
                                                            }
                                                            cx.notify();
                                                        },
                                                    )),
                                            )
                                            .into_any_element()
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(58.))
                            .flex_shrink_0()
                            .px_5()
                            .gap_2()
                            .items_center()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("meta-add")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Plus)
                                    .label("添加字段")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.add_meta_field(window, cx)
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new("meta-cancel")
                                    .ghost()
                                    .xsmall()
                                    .label("取消")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.meta_open = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("meta-save")
                                    .primary()
                                    .xsmall()
                                    .label("保存")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.save_metadata(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    // ---- templates ------------------------------------------------------

    fn template_card(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(template) = doc::TEMPLATES.get(index) else {
            return div().into_any_element();
        };
        let name = template.name;
        let description = template.description;
        v_flex()
            .id(("template-card", index))
            .flex_1()
            .min_w_0()
            .p_4()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted)
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(cx.theme().accent))
            .on_click(
                cx.listener(move |this, _, window, cx| this.new_from_template(index, window, cx)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(IconName::LayoutTemplate)
                            .with_size(px(15.))
                            .text_color(cx.theme().accent_foreground),
                    )
                    .child(div().text_sm().font_semibold().child(name)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
            .into_any_element()
    }

    fn templates_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let indices = (0..doc::TEMPLATES.len()).collect::<Vec<_>>();
        let rows = indices
            .chunks(2)
            .map(|chunk| {
                h_flex()
                    .w_full()
                    .gap_3()
                    .children(
                        chunk
                            .iter()
                            .map(|index| self.template_card(*index, cx))
                            .collect::<Vec<_>>(),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        self.modal_shell("template-overlay", cx)
            .child(
                v_flex()
                    .w(px(700.))
                    .max_h(px(560.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .overflow_hidden()
                    .occlude()
                    .child(
                        h_flex()
                            .h(px(58.))
                            .flex_shrink_0()
                            .px_5()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .gap_0()
                                    .child(div().text_base().font_semibold().child("从模板开始"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("模板只是起笔，标题、正文和元数据都可以改"),
                                    ),
                            )
                            .child(
                                Button::new("template-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::X)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.templates_open = false;
                                        this.focus_editor(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("template-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_5()
                            .gap_3()
                            .children(rows),
                    )
                    .child(
                        h_flex()
                            .h(px(58.))
                            .flex_shrink_0()
                            .px_5()
                            .gap_2()
                            .items_center()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("新文稿会保持未保存状态，首次保存时再选择位置"),
                            )
                            .child(
                                Button::new("template-blank")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::FilePlus)
                                    .label("空白文档")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.templates_open = false;
                                        this.new_document(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("template-cancel")
                                    .outline()
                                    .xsmall()
                                    .label("取消")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.templates_open = false;
                                        this.focus_editor(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn code_lang_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self
            .code_lang
            .as_ref()
            .map(|picker| picker.current.clone())
            .unwrap_or_default();
        let current_label = if current.is_empty() {
            "未设置".to_string()
        } else {
            doc::code_language_display(&current)
                .map(str::to_string)
                .unwrap_or_else(|| current.clone())
        };
        let mut languages = doc::CODE_LANGUAGES.to_vec();
        languages.sort_by(|(_, left), (_, right)| left.cmp(right));
        let rows = languages
            .chunks(3)
            .map(|chunk| {
                h_flex()
                    .w_full()
                    .gap_2()
                    .children(
                        chunk
                            .iter()
                            .map(|(token, display)| {
                                let id = SharedString::from(format!("code-lang-{token}"));
                                let mut button = Button::new(id).outline().xsmall();
                                if current.as_str() == *token {
                                    button = button.primary();
                                }
                                let token = *token;
                                button
                                    .label(*display)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.set_code_lang(token, window, cx);
                                    }))
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>(),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        self.modal_shell("code-lang-overlay", cx)
            .child(
                v_flex()
                    .w(px(620.))
                    .max_h(px(560.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .overflow_hidden()
                    .occlude()
                    .child(
                        h_flex()
                            .h(px(58.))
                            .flex_shrink_0()
                            .px_5()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .gap_0()
                                    .child(div().text_base().font_semibold().child("代码块语言"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "当前：{current_label} · 选择后写入围栏，预览按它高亮"
                                            )),
                                    ),
                            )
                            .child(
                                Button::new("code-lang-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::X)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.code_lang = None;
                                        this.focus_editor(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            // Per-fence id: reopening for another block starts
                            // its list from the top again.
                            .id(SharedString::from(format!(
                                "code-lang-body-{}",
                                self.code_lang.as_ref().map_or(0, |picker| picker.fence)
                            )))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_4()
                            .gap_2()
                            .children(rows),
                    )
                    .child(
                        h_flex()
                            .h(px(54.))
                            .flex_shrink_0()
                            .px_5()
                            .gap_2()
                            .items_center()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("列表中的语言都带语法高亮，也可以直接在围栏上手写"),
                            )
                            .child(
                                Button::new("code-lang-plain")
                                    .ghost()
                                    .xsmall()
                                    .label("纯文本")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_code_lang("", window, cx);
                                    })),
                            )
                            .child(
                                Button::new("code-lang-cancel")
                                    .outline()
                                    .xsmall()
                                    .label("取消")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.code_lang = None;
                                        this.focus_editor(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    // ---- AI -------------------------------------------------------------

    fn open_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.slash = None;
        self.ai.reset(window, cx);
        self.ai_open = true;
        cx.notify();
    }

    /// Pull the model list from `{base}/models` for the picker.
    fn fetch_ai_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = self.settings.ai(cx);
        self.settings.busy = Some("正在拉取模型列表…".to_string());
        self.settings.notice = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { ai::fetch_models(&config) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.settings.busy = None;
                match result {
                    Ok(models) => {
                        let count = models.len();
                        let current = this.settings.ai_model.read(cx).value().trim().to_string();
                        let needs_default = current.is_empty() || !models.contains(&current);
                        this.settings.ai_models = models.clone();
                        if needs_default && let Some(first) = models.first() {
                            let first = first.clone();
                            this.settings.ai_model.update(cx, |state, cx| {
                                state.set_value(first, window, cx);
                            });
                        }
                        this.flush_settings(cx).ok();
                        this.settings.notice = Some(format!("拉取到 {count} 个模型"));
                    }
                    Err(error) => {
                        this.settings.notice = Some(format!("拉取模型失败：{error}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Dropdown listing the models the last fetch returned.
    fn ai_model_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = cx.weak_entity();
        let models = self.settings.ai_models.clone();
        let current = self.settings.ai_model.read(cx).value();
        Button::new("ai-model-menu")
            .ghost()
            .xsmall()
            .icon(IconName::ChevronDown)
            .label(if current.is_empty() {
                SharedString::from("选择模型")
            } else {
                current.clone()
            })
            .tooltip("从拉取到的模型中选择")
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for model in models.clone() {
                    let handle = handle.clone();
                    let selected = model == current;
                    let value = model.clone();
                    menu = menu.item(PopupMenuItem::new(model).checked(selected).on_click(
                        move |_, window, cx| {
                            handle
                                .update(cx, |this, cx| {
                                    let value = value.clone();
                                    this.settings.ai_model.update(cx, |state, cx| {
                                        state.set_value(value, window, cx);
                                    });
                                    this.flush_settings(cx).ok();
                                    cx.notify();
                                })
                                .ok();
                        },
                    ));
                }
                menu
            })
    }

    fn run_ai(&mut self, target: AiTarget, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai.busy {
            return;
        }
        let topic = self.ai.topic.read(cx).value().trim().to_string();
        let config = cx.global::<config::AppConfig>().ai.clone();
        if config.base_url.is_empty() {
            self.ai.status = vec!["还没有填写 AI 服务地址，请到设置 → AI 写作里配置".to_string()];
            cx.notify();
            return;
        }
        if config.api_key.is_empty() {
            self.ai.status = vec!["还没有填写 API Key，请到设置 → AI 写作里配置".to_string()];
            cx.notify();
            return;
        }
        if config.model.is_empty() {
            self.ai.status = vec!["还没有选择模型，先在设置里点“拉取模型”".to_string()];
            cx.notify();
            return;
        }
        if topic.is_empty() {
            self.ai.status = vec!["先写一句主题或写作要求".to_string()];
            cx.notify();
            return;
        }

        let context = if self.ai.use_context {
            let text = self.editor.read(cx).value().to_string();
            if text.chars().count() > 6000 {
                text.chars().take(6000).collect()
            } else {
                text
            }
        } else {
            String::new()
        };

        self.ai.busy = true;
        self.ai.status = vec![format!("正在用 {} 生成，请稍候…", config.model)];
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    ai::generate(&config, &topic, &context).map(|text| ai::clean_markdown(&text))
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.ai.busy = false;
                match result {
                    Ok(article) => {
                        let chars = article.chars().count();
                        this.ai.status.push(format!("✓ 生成完成，{chars} 字"));
                        match target {
                            AiTarget::NewDocument => {
                                this.new_document_from(&article, window, cx);
                                this.notice = Some(format!("AI 已生成新文档（{chars} 字）"));
                            }
                            AiTarget::Insert => {
                                this.insert_ai_article(&article, window, cx);
                                this.notice = Some(format!("AI 内容已插入光标处（{chars} 字）"));
                            }
                        }
                        this.ai_open = false;
                    }
                    Err(error) => this.ai.status.push(format!("! {error}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn new_document_from(&mut self, content: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.untitled_count += 1;
        let id = self.next_id;
        self.next_id += 1;
        let title =
            first_heading(content).unwrap_or_else(|| format!("AI 文稿 {}", self.untitled_count));
        self.docs.push(Document {
            id,
            title: title.into(),
            path: None,
            saved_content: String::new(),
            content: content.to_string(),
        });
        let index = self.docs.len() - 1;
        self.activate_doc(index, window, cx);
    }

    /// Splice the article into the current selection, or at the caret when the
    /// selection is empty. `replace_all` keeps the undo history, so the whole
    /// insertion can be undone in one step.
    fn insert_ai_article(&mut self, article: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.editor.read(cx).value().to_string();
        let selection = self.editor.read(cx).selected_range();
        let start = selection.start.min(text.len());
        let end = selection.end.min(text.len());
        let mut spliced = String::with_capacity(text.len() + article.len());
        spliced.push_str(&text[..start]);
        spliced.push_str(article);
        spliced.push_str(&text[end..]);
        let cursor = start + article.len();

        self.editor.update(cx, |state, cx| {
            state.replace_all(spliced, window, cx);
            state.set_selected_range(cursor..cursor, cx);
        });
        self.sync_editor(cx);
        self.focus_editor(window, cx);
    }

    // ---- chat ------------------------------------------------------------

    fn open_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.slash = None;
        self.chat_error = None;
        self.chat_open = true;
        let focus = self.chat_input.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        cx.notify();
    }

    fn on_chat_enter(
        &mut self,
        _: &gpui_kit::component::input::Enter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.chat_open {
            self.send_chat(window, cx);
        }
    }

    fn send_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.chat_busy || !self.chat_open {
            return;
        }
        let message = self.chat_input.read(cx).value().trim().to_string();
        if message.is_empty() {
            return;
        }
        let config = cx.global::<config::AppConfig>().ai.clone();
        if config.base_url.trim().is_empty()
            || config.api_key.trim().is_empty()
            || config.model.trim().is_empty()
        {
            self.chat_error =
                Some("先在 设置 → AI 写作 里填写服务地址、API Key 和模型".to_string());
            cx.notify();
            return;
        }
        let context = if self.chat_use_doc {
            let text = self.editor.read(cx).value().to_string();
            if text.chars().count() > 6000 {
                text.chars().take(6000).collect()
            } else {
                text
            }
        } else {
            String::new()
        };
        let system = chat_system_prompt(&context);

        self.chat_turns.push(ai::ChatTurn {
            role: "user".to_string(),
            content: message,
        });
        self.chat_input
            .update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.chat_busy = true;
        self.chat_error = None;
        cx.on_next_frame(window, |this, _, _| {
            this.chat_scroll.scroll_to_bottom();
        });
        cx.notify();

        let turns = self.chat_turns.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { ai::chat(&config, &system, &turns) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.chat_busy = false;
                match result {
                    Ok(reply) => this.chat_turns.push(ai::ChatTurn {
                        role: "assistant".to_string(),
                        content: reply,
                    }),
                    Err(error) => this.chat_error = Some(error),
                }
                cx.on_next_frame(window, |this, _, _| {
                    this.chat_scroll.scroll_to_bottom();
                });
                let focus = this.chat_input.read(cx).focus_handle(cx);
                focus.focus(window, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn insert_chat_reply(&mut self, content: String, window: &mut Window, cx: &mut Context<Self>) {
        self.chat_open = false;
        self.insert_ai_article(&content, window, cx);
        self.notice = Some("AI 回复已插入光标处".to_string());
    }

    fn chat_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let ai_config = cx
            .try_global::<config::AppConfig>()
            .map(|config| config.ai.clone())
            .unwrap_or_default();
        let configured = !ai_config.base_url.trim().is_empty()
            && !ai_config.api_key.trim().is_empty()
            && !ai_config.model.trim().is_empty();
        let model = if ai_config.model.trim().is_empty() {
            "未选择模型".to_string()
        } else {
            ai_config.model.clone()
        };
        let can_send = !self.chat_busy && !self.chat_input.read(cx).value().trim().is_empty();
        let turns = self
            .chat_turns
            .iter()
            .enumerate()
            .map(|(index, turn)| self.chat_turn_row(index, turn, cx))
            .collect::<Vec<_>>();

        let mut use_doc = Button::new("chat-use-doc")
            .xsmall()
            .label("参考文稿")
            .tooltip("把当前文稿（最多 6000 字）一起发给模型");
        use_doc = if self.chat_use_doc {
            use_doc.primary()
        } else {
            use_doc.outline()
        };

        self.modal_shell("chat-overlay", cx)
            .child(
                v_flex()
                    .w(px(640.))
                    .h(relative(0.84))
                    .max_h(px(680.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .overflow_hidden()
                    .occlude()
                    .on_action(cx.listener(Self::on_chat_enter))
                    .child(
                        h_flex()
                            .h(px(58.))
                            .flex_shrink_0()
                            .px_5()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .gap_0()
                                    .child(div().text_base().font_semibold().child("询问 AI"))
                                    .child(
                                        div()
                                            .max_w(px(460.))
                                            .truncate()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if configured {
                                                format!(
                                                    "OpenAI 兼容接口 · {model} · Enter 发送，Shift+Enter 换行"
                                                )
                                            } else {
                                                "尚未配置：设置 → AI 写作".to_string()
                                            }),
                                    ),
                            )
                            .child(
                                Button::new("chat-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::X)
                                    .disabled(self.chat_busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.chat_open = false;
                                        this.focus_editor(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("chat-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.chat_scroll)
                            .p_5()
                            .gap_3()
                            .when(self.chat_turns.is_empty(), |this| {
                                this.child(
                                    v_flex()
                                        .w_full()
                                        .py_6()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Icon::new(IconName::Sparkles)
                                                .with_size(px(22.))
                                                .text_color(cx.theme().accent_foreground),
                                        )
                                        .child(div().text_sm().font_semibold().child("和 AI 聊几句"))
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(
                                                    "例如：把这段改得更简洁 / 给文章起五个标题 / 解释这个报错",
                                                ),
                                        ),
                                )
                            })
                            .children(turns)
                            .when(self.chat_busy, |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("正在思考…"),
                                )
                            })
                            .when_some(self.chat_error.clone(), |this, error| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().danger)
                                        .child(error),
                                )
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_shrink_0()
                            .px_5()
                            .py_3()
                            .gap_2()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Textarea::new(&self.chat_input)
                                    .w_full()
                                    .h(px(72.))
                                    .small(),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(use_doc.on_click(cx.listener(|this, _, _, cx| {
                                        this.chat_use_doc = !this.chat_use_doc;
                                        cx.notify();
                                    })))
                                    .child(div().flex_1())
                                    .child(
                                        Button::new("chat-clear")
                                            .ghost()
                                            .xsmall()
                                            .label("清空")
                                            .disabled(self.chat_busy || self.chat_turns.is_empty())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.chat_turns.clear();
                                                this.chat_error = None;
                                                let focus =
                                                    this.chat_input.read(cx).focus_handle(cx);
                                                focus.focus(window, cx);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("chat-send")
                                            .primary()
                                            .xsmall()
                                            .label(if self.chat_busy { "发送中…" } else { "发送" })
                                            .disabled(!can_send)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.send_chat(window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn chat_turn_row(
        &self,
        index: usize,
        turn: &ai::ChatTurn,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let is_user = turn.role != "assistant";
        let content = turn.content.clone();
        let row = v_flex()
            .id(SharedString::from(format!("chat-turn-{index}")))
            .w_full()
            .gap_1()
            .when(is_user, |this| this.items_end())
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(if is_user { "你" } else { "AI" }),
            )
            .child(
                div()
                    .text_sm()
                    .when(is_user, |this| {
                        this.max_w(relative(0.86))
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(cx.theme().muted)
                    })
                    .child(content.clone()),
            );
        if is_user {
            return row.into_any_element();
        }
        row.child(
            Button::new(SharedString::from(format!("chat-insert-{index}")))
                .ghost()
                .xsmall()
                .label("插入文稿")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.insert_chat_reply(content.clone(), window, cx);
                })),
        )
        .into_any_element()
    }

    fn ai_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.settings.ai_model.read(cx).value().to_string();
        let configured = cx
            .try_global::<config::AppConfig>()
            .map(|config| !config.ai.base_url.is_empty() && !config.ai.api_key.is_empty())
            .unwrap_or(false);

        self.modal_shell("ai-overlay", cx)
            .child(
                v_flex()
                    .w(px(720.))
                    .h(relative(0.86))
                    .max_h(px(680.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .overflow_hidden()
                    .occlude()
                    .child(
                        h_flex()
                            .h(px(58.))
                            .flex_shrink_0()
                            .px_5()
                            .items_center()
                            .justify_between()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                v_flex()
                                    .gap_0()
                                    .child(div().text_base().font_semibold().child("AI 写作"))
                                    .child(
                                        div()
                                            .text_xs()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if configured {
                                                format!(
                                                    "OpenAI 兼容接口 · {}",
                                                    if model.is_empty() {
                                                        "未选择模型".to_string()
                                                    } else {
                                                        model
                                                    }
                                                )
                                            } else {
                                                "尚未配置：设置 → AI 写作".to_string()
                                            }),
                                    ),
                            )
                            .child(
                                Button::new("ai-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::X)
                                    .disabled(self.ai.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.ai_open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .id("ai-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_5()
                            .gap_4()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_baseline()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_semibold()
                                                    .child("主题 / 要求"),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("越具体越好：主题、字数、读者、语气"),
                                            ),
                                    )
                                    .child(Input::new(&self.ai.topic).w_full().small()),
                            )
                            .child(self.toggle_row(
                                "ai-use-context",
                                "参考当前文档",
                                "把当前文稿（最多 6000 字）发给模型，用于续写、改写或扩写",
                                self.ai.use_context,
                                false,
                                cx,
                                |this| {
                                    this.ai.use_context = !this.ai.use_context;
                                },
                            ))
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_baseline()
                                            .child(div().text_xs().font_semibold().child("提示词"))
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("作为 system 消息发送，可在设置里修改"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("ai-prompt-preview")
                                            .w_full()
                                            .px_3()
                                            .py_2()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .bg(cx.theme().muted)
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .line_clamp(4)
                                            .child(
                                                cx.try_global::<config::AppConfig>()
                                                    .map(|config| config.ai.prompt.clone())
                                                    .unwrap_or_default(),
                                            ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .id("ai-log")
                                    .w_full()
                                    .min_h(px(96.))
                                    .max_h(px(160.))
                                    .overflow_y_scroll()
                                    .p_3()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .bg(cx.theme().muted)
                                    .gap_1()
                                    .when(self.ai.status.is_empty(), |this| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("生成结果会先经过这里，再写入文稿。"),
                                        )
                                    })
                                    .children(
                                        self.ai
                                            .status
                                            .iter()
                                            .map(|line| {
                                                div()
                                                    .text_xs()
                                                    .font_family(
                                                        cx.theme().mono_font_family.clone(),
                                                    )
                                                    .text_color(if line.starts_with('!') {
                                                        cx.theme().danger
                                                    } else {
                                                        cx.theme().foreground
                                                    })
                                                    .child(line.clone())
                                                    .into_any_element()
                                            })
                                            .collect::<Vec<_>>(),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(60.))
                            .flex_shrink_0()
                            .px_5()
                            .gap_2()
                            .items_center()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if self.ai.busy {
                                        "生成中…"
                                    } else {
                                        "生成到新文档，或插入当前光标位置（选中文字则替换）"
                                    }),
                            )
                            .child(
                                Button::new("ai-cancel")
                                    .ghost()
                                    .xsmall()
                                    .label("取消")
                                    .disabled(self.ai.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.ai_open = false;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("ai-insert")
                                    .outline()
                                    .xsmall()
                                    .label("插入光标处")
                                    .disabled(self.ai.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.run_ai(AiTarget::Insert, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("ai-generate")
                                    .primary()
                                    .xsmall()
                                    .label(if self.ai.busy {
                                        "生成中…"
                                    } else {
                                        "生成新文档"
                                    })
                                    .disabled(self.ai.busy)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.run_ai(AiTarget::NewDocument, window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    // ---- settings ------------------------------------------------------

    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.slash = None;
        let config = cx.global::<config::AppConfig>().clone();
        self.settings = SettingsForm::new(window, cx, &config);
        self.settings_watch = Self::watch_settings(&self.settings, cx);
        self.settings_open = true;
        cx.notify();
    }

    /// Write the settings form into the global config and onto disk.
    ///
    /// Called on every edit in the settings dialog, so the dialog needs no
    /// explicit save: closing it in any way keeps what was typed.
    fn flush_settings(&self, cx: &mut App) -> Result<(), String> {
        let mut config = cx.global::<config::AppConfig>().clone();
        self.settings.apply(&mut config, cx)?;
        *cx.global_mut::<config::AppConfig>() = config.clone();
        config::save_to_disk(&config)
    }

    /// Persist every edit made in the settings dialog as it happens.
    fn watch_settings(form: &SettingsForm, cx: &mut Context<Self>) -> Vec<Subscription> {
        let mut watch = Vec::new();
        for input in form.inputs() {
            watch.push(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.flush_settings(cx).ok();
                }
            }));
        }
        let prompt = form.ai_prompt.clone();
        watch.push(cx.subscribe(&prompt, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.flush_settings(cx).ok();
            }
        }));
        watch
    }

    fn close_settings(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.flush_settings(cx) {
            self.settings.notice = Some(format!("保存失败：{error}"));
        }
        self.settings_open = false;
        cx.notify();
    }

    fn save_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        match self.flush_settings(cx) {
            Ok(()) => {
                self.settings_open = false;
                self.notice = Some("设置已保存".to_string());
            }
            Err(error) => {
                self.settings.notice = Some(format!("保存失败：{error}"));
            }
        }
        cx.notify();
    }

    fn test_image_host(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.settings.image_host(cx);
        if host.kind == config::ImageHostKind::None {
            self.settings.notice = Some("请先选择并填写图床".to_string());
            cx.notify();
            return;
        }
        self.settings.busy = Some("正在上传测试图片…".to_string());
        self.settings.notice = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { imagebed::test_upload(&host) })
                .await;
            this.update_in(cx, |this, _, cx| {
                this.settings.busy = None;
                this.settings.notice = Some(match result {
                    Ok(url) => format!("图床可用，测试图片：{url}"),
                    Err(error) => format!("图床测试失败：{error}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn test_wechat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wechat = self.settings.wechat(cx);
        if wechat.app_id.is_empty() || wechat.app_secret.is_empty() {
            self.settings.notice = Some("请先填写 AppID 和 AppSecret".to_string());
            cx.notify();
            return;
        }
        self.settings.busy = Some("正在获取微信 access_token…".to_string());
        self.settings.notice = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { publish::wechat_test(&wechat.app_id, &wechat.app_secret) })
                .await;
            this.update_in(cx, |this, _, cx| {
                this.settings.busy = None;
                this.settings.notice = Some(match result {
                    Ok(()) => "公众号凭证有效，access_token 获取成功".to_string(),
                    Err(error) => format!("公众号测试失败：{error}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn settings_git_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.workspace.as_ref().map(|tree| tree.root.clone());
        self.settings.busy = Some("正在检测 Git…".to_string());
        self.settings.notice = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result: Result<String, String> = cx
                .background_executor()
                .spawn(async move {
                    let version = git::version()?;
                    match root {
                        Some(root) => Ok(format!("{version} · {}", git::status(&root)?)),
                        None => Ok(format!("{version} · 还没有打开文件夹")),
                    }
                })
                .await;
            this.update_in(cx, |this, _, cx| {
                this.settings.busy = None;
                this.settings.notice = Some(match result {
                    Ok(message) => message,
                    Err(error) => format!("Git 检测失败：{error}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn settings_git_init(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.workspace.as_ref().map(|tree| tree.root.clone()) else {
            self.settings.notice = Some("请先在侧边栏打开一个文件夹".to_string());
            cx.notify();
            return;
        };
        let config = self.settings.git(cx);
        self.run_git(root, config, true, window, cx);
    }

    fn settings_git_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.workspace.as_ref().map(|tree| tree.root.clone()) else {
            self.settings.notice = Some("请先在侧边栏打开一个文件夹".to_string());
            cx.notify();
            return;
        };
        let config = self.settings.git(cx);
        self.run_git(root, config, false, window, cx);
    }

    fn run_git(
        &mut self,
        root: PathBuf,
        config: config::GitConfig,
        init: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = if init {
            "正在初始化 Git…"
        } else {
            "正在 Git 同步…"
        };
        self.settings.busy = Some(label.to_string());
        self.action_busy = Some(label.to_string());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if init {
                        git::init(&root, &config)
                    } else {
                        git::sync(&root, &config)
                    }
                })
                .await;
            this.update_in(cx, |this, _, cx| {
                this.settings.busy = None;
                this.action_busy = None;
                match result {
                    Ok(log) => {
                        this.settings.notice = Some(log.clone());
                        this.notice =
                            Some(log.lines().last().unwrap_or("Git 操作完成").to_string());
                    }
                    Err(error) => {
                        this.settings.notice = Some(format!("Git 操作失败：{error}"));
                        this.notice = Some(format!("Git 操作失败：{error}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn backup_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = cx.global::<config::AppConfig>().clone();
        let dir = config::default_backup_dir(&config);
        self.backup_to(dir, window, cx);
    }

    fn settings_backup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let config = cx.global::<config::AppConfig>().clone();
        let entered = self.settings.backup_dir.read(cx).value().trim().to_string();
        let dir = if entered.is_empty() {
            config::default_backup_dir(&config)
        } else {
            PathBuf::from(entered)
        };
        self.backup_to(dir, window, cx);
    }

    fn backup_to(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let root = self.workspace.as_ref().map(|tree| tree.root.clone());
        let documents: Vec<(String, String)> = if root.is_none() {
            self.docs
                .iter()
                .map(|doc| (doc.title.to_string(), doc.content.clone()))
                .collect()
        } else {
            Vec::new()
        };
        self.settings.busy = Some("正在打包备份…".to_string());
        self.action_busy = Some("正在备份…".to_string());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if let Some(root) = root {
                        backup::backup_workspace(&root, &dir)
                    } else {
                        backup::backup_documents(&documents, &dir)
                    }
                })
                .await;
            this.update_in(cx, |this, _, cx| {
                this.settings.busy = None;
                this.action_busy = None;
                match result {
                    Ok(path) => {
                        let message = format!("备份完成：{}", path.display());
                        this.notice = Some(message.clone());
                        this.settings.notice = Some(message);
                    }
                    Err(error) => {
                        let message = format!("备份失败：{error}");
                        this.notice = Some(message.clone());
                        this.settings.notice = Some(message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- publishing ----------------------------------------------------

    fn open_publish(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.slash = None;
        let title = self.docs[self.active].file_stem();
        let config = cx.global::<config::AppConfig>().clone();
        self.publish.reset(&title, _window, cx);
        self.publish.targets.wechat =
            !config.wechat.app_id.trim().is_empty() && !config.wechat.app_secret.trim().is_empty();
        self.publish.targets.cnblogs =
            !config.cnblogs.username.trim().is_empty() && !config.cnblogs.token.trim().is_empty();
        self.publish_open = true;
        cx.notify();
    }

    fn start_publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.publish.busy {
            return;
        }
        let targets = self.publish.targets;
        if !targets.any() {
            self.publish.status = vec!["请至少选择一个发布平台".to_string()];
            cx.notify();
            return;
        }
        let config = cx.global::<config::AppConfig>().clone();
        let mut title = self.publish.title.read(cx).value().trim().to_string();
        if title.is_empty() {
            title = self.docs[self.active].file_stem();
        }
        let source = self.editor.read(cx).value().to_string();
        let doc_dir = self.current_doc_dir();
        let doc_id = self.docs[self.active].id;
        let upload_images = self.publish.upload_images;
        self.publish.busy = true;
        self.publish.status = vec!["正在准备文章…".to_string()];
        cx.notify();

        let request = publish::PublishRequest {
            title,
            source,
            doc_dir,
            image_host: config.image_host,
            wechat: config.wechat,
            cnblogs: config.cnblogs,
            targets,
            upload_images,
        };

        cx.spawn_in(window, async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { publish::publish(request) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.publish.busy = false;
                if let Some(source) = outcome.updated_markdown {
                    this.apply_source_update(doc_id, source, window, cx);
                }
                if outcome.uploaded_images > 0 {
                    this.publish
                        .status
                        .push(format!("✓ 已上传 {} 张图片到图床", outcome.uploaded_images));
                }
                for line in outcome.log {
                    this.publish.status.push(format!("✓ {line}"));
                }
                for warning in outcome.warnings {
                    this.publish.status.push(format!("! {warning}"));
                }
                if let Some(html) = outcome.clipboard_html {
                    match publish::copy_rich_html(&html, &outcome.plain_text) {
                        Ok(()) => this
                            .publish
                            .status
                            .push("✓ 富文本已复制到剪贴板，可在浏览器里直接粘贴".to_string()),
                        Err(error) => this
                            .publish
                            .status
                            .push(format!("! 写入剪贴板失败：{error}")),
                    }
                }
                for url in outcome.open_urls {
                    cx.open_url(&url);
                }
                if this.publish.status.len() == 1 {
                    this.publish.status.push("完成".to_string());
                }
                if this.publish.status.len() > 100 {
                    this.publish
                        .status
                        .drain(0..this.publish.status.len() - 100);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn copy_publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.publish.busy {
            return;
        }
        let config = cx.global::<config::AppConfig>().clone();
        let source = self.editor.read(cx).value().to_string();
        let doc_dir = self.current_doc_dir();
        let doc_id = self.docs[self.active].id;
        let upload_images = self.publish.upload_images;
        let footnote = config.wechat.footnote_links;
        self.publish.busy = true;
        self.publish.status = vec!["正在准备富文本…".to_string()];
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let (source, uploaded, warnings, html) = cx
                .background_executor()
                .spawn(async move {
                    let mut source = source;
                    let mut uploaded = 0;
                    let mut warnings = Vec::new();
                    if upload_images {
                        let result = upload_local_images(&source, &doc_dir, &config.image_host);
                        source = result.markdown;
                        uploaded = result.uploaded;
                        warnings = result.warnings;
                    }
                    let (_, body) = doc::split_frontmatter(&source);
                    let html = publish::render_inline_html(
                        body,
                        &std::collections::HashMap::new(),
                        footnote,
                    );
                    (source, uploaded, warnings, html)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.publish.busy = false;
                this.apply_source_update(doc_id, source.clone(), window, cx);
                match publish::copy_rich_html(&html, &source) {
                    Ok(()) => this
                        .publish
                        .status
                        .push("✓ 排版已复制，可粘贴到任意富文本编辑器".to_string()),
                    Err(error) => this.publish.status.push(format!("! 复制失败：{error}")),
                }
                if uploaded > 0 {
                    this.publish
                        .status
                        .push(format!("✓ 已上传 {uploaded} 张图片"));
                }
                for warning in warnings {
                    this.publish.status.push(format!("! {warning}"));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_source_update(
        &mut self,
        doc_id: u64,
        source: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.docs.iter().position(|doc| doc.id == doc_id) else {
            return;
        };
        self.docs[index].content = source.clone();
        if index != self.active {
            return;
        }
        self.editor
            .update(cx, |state, cx| state.set_value(&source, window, cx));
        self.preview
            .update(cx, |state, cx| state.set_text(&source, cx));
        self.outline = doc::outline(&source);
        self.stats = doc::stats(&source);
        cx.notify();
    }

    // ---- action handlers ----------------------------------------------

    fn on_open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        self.open_folder(window, cx);
    }

    fn on_refresh_folder(&mut self, _: &RefreshFolder, _: &mut Window, cx: &mut Context<Self>) {
        self.refresh_tree(cx);
    }

    fn on_new_file(&mut self, _: &NewFile, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.prompt_target_dir() else {
            self.notice = Some("请先打开一个文件夹".to_string());
            cx.notify();
            return;
        };
        self.open_prompt(PromptKind::NewFile(dir), window, cx);
    }

    fn on_new_folder(&mut self, _: &NewFolder, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.prompt_target_dir() else {
            self.notice = Some("请先打开一个文件夹".to_string());
            cx.notify();
            return;
        };
        self.open_prompt(PromptKind::NewFolder(dir), window, cx);
    }

    fn on_rename_file(&mut self, _: &RenameFile, window: &mut Window, cx: &mut Context<Self>) {
        let path = self
            .selected_path
            .clone()
            .or_else(|| self.docs[self.active].path.clone());
        let Some(path) = path else {
            self.notice = Some("先在文件树里选中一个文件或文件夹".to_string());
            cx.notify();
            return;
        };
        self.open_prompt(PromptKind::Rename(path), window, cx);
    }

    fn on_delete_file(&mut self, _: &DeleteFile, window: &mut Window, cx: &mut Context<Self>) {
        let path = self
            .selected_path
            .clone()
            .or_else(|| self.docs[self.active].path.clone());
        let Some(path) = path else {
            self.notice = Some("先在文件树里选中一个文件或文件夹".to_string());
            cx.notify();
            return;
        };
        self.open_prompt(PromptKind::Delete(path), window, cx);
    }

    fn on_reveal_in_files(&mut self, _: &RevealInFiles, _: &mut Window, cx: &mut Context<Self>) {
        self.reveal_selected(cx);
    }

    fn on_close_overlay(&mut self, _: &CloseOverlay, _: &mut Window, cx: &mut Context<Self>) {
        if self.slash.take().is_some() {
            cx.notify();
            return;
        }
        if self.close_confirm.take().is_some() {
            cx.notify();
            return;
        }
        if self.prompt.take().is_some() {
            cx.notify();
            return;
        }
        if self.publish_open {
            self.publish_open = false;
            cx.notify();
            return;
        }
        if self.ai_open {
            self.ai_open = false;
            cx.notify();
            return;
        }
        if self.chat_open {
            self.chat_open = false;
            cx.notify();
            return;
        }
        if self.meta_open {
            self.meta_open = false;
            cx.notify();
            return;
        }
        if self.templates_open {
            self.templates_open = false;
            cx.notify();
            return;
        }
        if self.code_lang.take().is_some() {
            cx.notify();
            return;
        }
        if self.settings_open {
            self.close_settings(cx);
        }
    }

    fn on_publish_action(
        &mut self,
        _: &PublishArticle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_publish(window, cx);
    }

    fn on_ai_write(&mut self, _: &AiWrite, window: &mut Window, cx: &mut Context<Self>) {
        self.open_ai(window, cx);
    }

    fn on_open_settings_action(
        &mut self,
        _: &OpenSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(window, cx);
    }

    fn on_backup_action(&mut self, _: &CreateBackup, window: &mut Window, cx: &mut Context<Self>) {
        self.backup_now(window, cx);
    }

    fn on_git_sync_action(&mut self, _: &GitSync, window: &mut Window, cx: &mut Context<Self>) {
        self.git_sync_from_config(window, cx);
    }

    fn git_sync_from_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.workspace.as_ref().map(|tree| tree.root.clone()) else {
            self.notice = Some("请先打开一个文件夹".to_string());
            cx.notify();
            return;
        };
        let config = cx.global::<config::AppConfig>().git.clone();
        self.run_git(root, config, false, window, cx);
    }

    // ---- modals --------------------------------------------------------

    fn modal_shell(&self, id: &'static str, _cx: &App) -> Stateful<Div> {
        let backdrop_id = SharedString::from(format!("{id}-backdrop"));
        v_flex()
            .id(id)
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .items_center()
            .justify_center()
            .key_context("Modal")
            .child(
                div()
                    .id(backdrop_id)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(Hsla::from(rgba(0x1C171066)))
                    .occlude()
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(CloseOverlay), cx);
                    }),
            )
    }

    fn settings_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let kind = self.settings.image_kind;

        self.modal_shell("settings-overlay", cx).child(
            v_flex()
                .w(px(780.))
                .h(relative(0.92))
                .max_h(px(760.))
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .overflow_hidden()
                .occlude()
                .child(
                    h_flex()
                        .h(px(58.))
                        .flex_shrink_0()
                        .px_5()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            v_flex()
                                .gap_0()
                                .child(div().text_base().font_semibold().child("设置"))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("图床、发布账号、Git 与备份"),
                                ),
                        )
                        .child(
                            Button::new("settings-close")
                                .ghost()
                                .xsmall()
                                .icon(IconName::X)
                                .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx))),
                        ),
                )
                .child(
                    v_flex()
                        .id("settings-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_5()
                        .gap_4()
                        .child(
                            v_flex()
                                .gap_3()
                                .p_4()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(self.section_header(
                                    "AI 写作",
                                    "任何 OpenAI 兼容接口（OpenAI、DeepSeek、Moonshot、Ollama…）都可用。",
                                    cx,
                                ))
                                .child(self.settings_field(
                                    "Base URL",
                                    "包含 /v1，例如 https://api.openai.com/v1",
                                    &self.settings.ai_base,
                                    cx,
                                ))
                                .child(self.settings_field(
                                    "API Key",
                                    "只保存在本机配置文件",
                                    &self.settings.ai_key,
                                    cx,
                                ))
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .items_baseline()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_semibold()
                                                        .child("模型"),
                                                )
                                                .when(!self.settings.ai_models.is_empty(), |this| {
                                                    this.child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(format!(
                                                                "{} 个可选",
                                                                self.settings.ai_models.len()
                                                            )),
                                                    )
                                                }),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .child(Input::new(&self.settings.ai_model).flex_1().small())
                                                .when(!self.settings.ai_models.is_empty(), |this| {
                                                    this.child(self.ai_model_menu(cx))
                                                })
                                                .child(
                                                    Button::new("ai-fetch-models")
                                                        .outline()
                                                        .xsmall()
                                                        .label("拉取模型")
                                                        .w(px(100.))
                                                        .disabled(self.settings.busy.is_some())
                                                        .on_click(cx.listener(|this, _, window, cx| {
                                                            this.fetch_ai_models(window, cx)
                                                        })),
                                                ),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .items_baseline()
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_semibold()
                                                        .child("提示词"),
                                                )
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child("system 消息，决定文章的身份与结构"),
                                                ),
                                        )
                                        .child(
                                            Textarea::new(&self.settings.ai_prompt)
                                                .w_full()
                                                .h(px(128.))
                                                .small(),
                                        ),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_3()
                                .p_4()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(self.section_header(
                                    "图床",
                                    "本地图片会先上传到图床，再把 Markdown 里的路径替换为公开链接。",
                                    cx,
                                ))
                                .child(
                                    h_flex()
                                        .gap_1()
                                        .child(self.host_kind_button("关闭", config::ImageHostKind::None, cx))
                                        .child(self.host_kind_button("CloudFlare-ImgBed", config::ImageHostKind::CloudflareImgbed, cx))
                                        .child(self.host_kind_button("Lsky Pro", config::ImageHostKind::Lsky, cx)),
                                )
                                .when(kind != config::ImageHostKind::None, |this| {
                                    this.child(self.settings_field(
                                        "图床地址",
                                        "例如 https://img.example.com，不带结尾斜杠",
                                        &self.settings.image_url,
                                        cx,
                                    ))
                                })
                                .when(kind == config::ImageHostKind::CloudflareImgbed, |this| {
                                    this.child(self.settings_field(
                                        "上传 authCode",
                                        "CloudFlare-ImgBed 的上传认证码，可留空",
                                        &self.settings.image_auth,
                                        cx,
                                    ))
                                    .child(self.settings_field(
                                        "API Token",
                                        "可选，使用 Authorization: Bearer",
                                        &self.settings.image_token,
                                        cx,
                                    ))
                                })
                                .when(kind == config::ImageHostKind::Lsky, |this| {
                                    this.child(self.settings_field(
                                        "Lsky Token",
                                        "Lsky Pro 用户设置里生成的 API Token",
                                        &self.settings.image_token,
                                        cx,
                                    ))
                                })
                                .when(kind != config::ImageHostKind::None, |this| {
                                    this.child(
                                        Button::new("test-image-host")
                                            .outline()
                                            .xsmall()
                                            .label("上传测试图片")
                                            .w(px(160.))
                                            .disabled(self.settings.busy.is_some())
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.test_image_host(window, cx)
                                            })),
                                    )
                                })
                                .child(self.toggle_row(
                                    "image-upload-on-paste",
                                    "粘贴图片自动上传",
                                    "编辑器里粘贴截图时直接上传图床，并插入公开链接",
                                    self.settings.image_upload_on_paste,
                                    true,
                                    cx,
                                    |this| {
                                        this.settings.image_upload_on_paste =
                                            !this.settings.image_upload_on_paste;
                                    },
                                )),
                        )
                        .child(
                            v_flex()
                                .gap_3()
                                .p_4()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(self.section_header(
                                    "微信公众号",
                                    "认证号可直接创建图文草稿；需要把程序出口 IP 加入公众号后台白名单。",
                                    cx,
                                ))
                                .child(self.settings_field("AppID", "", &self.settings.wx_appid, cx))
                                .child(self.settings_field("AppSecret", "只保存在本机配置文件", &self.settings.wx_secret, cx))
                                .child(self.settings_field("作者名", "写入草稿的作者字段", &self.settings.wx_author, cx))
                                .child(self.settings_field("默认封面", "文章没有图片时使用；填本地绝对路径", &self.settings.wx_cover, cx))
                                .child(self.toggle_row(
                                    "wechat-footnote",
                                    "外链转参考",
                                    "公众号正文里的外链会被移除，开启后在文末生成参考链接列表",
                                    self.settings.wechat_footnote,
                                    true,
                                    cx,
                                    |this| {
                                        this.settings.wechat_footnote =
                                            !this.settings.wechat_footnote;
                                    },
                                ))
                                .child(
                                    Button::new("test-wechat")
                                        .outline()
                                        .xsmall()
                                        .label("测试公众号凭证")
                                        .w(px(160.))
                                        .disabled(self.settings.busy.is_some())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.test_wechat(window, cx)
                                        })),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_3()
                                .p_4()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(self.section_header(
                                    "博客园",
                                    "使用 MetaWeblog 接口发布；令牌在博客园设置 → 其他设置中生成。",
                                    cx,
                                ))
                                .child(self.settings_field("用户名", "", &self.settings.cn_user, cx))
                                .child(self.settings_field("MetaWeblog 令牌", "", &self.settings.cn_token, cx))
                                .child(self.settings_field("Blog ID", "留空默认使用用户名", &self.settings.cn_blogid, cx)),
                        )
                        .child(
                            v_flex()
                                .gap_3()
                                .p_4()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(self.section_header(
                                    "Git 同步",
                                    "使用本机 git 命令和系统凭据管理器，Inkdown 不保存 Git 密码。",
                                    cx,
                                ))
                                .child(self.settings_field("远端地址", "例如 https://github.com/you/notes.git", &self.settings.git_remote, cx))
                                .child(self.settings_field("分支", "默认 main", &self.settings.git_branch, cx))
                                .child(
                                    h_flex()
                                        .gap_3()
                                        .child(div().flex_1().child(self.settings_field("Git 用户名", "", &self.settings.git_name, cx)))
                                        .child(div().flex_1().child(self.settings_field("Git 邮箱", "", &self.settings.git_email, cx))),
                                )
                                .child(self.toggle_row(
                                    "git-auto-sync",
                                    "保存后自动同步",
                                    "每次保存文档后提交并推送",
                                    self.settings.git_auto_sync,
                                    true,
                                    cx,
                                    |this| {
                                        this.settings.git_auto_sync =
                                            !this.settings.git_auto_sync;
                                    },
                                ))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            Button::new("settings-git-check")
                                                .outline()
                                                .xsmall()
                                                .label("检测 Git")
                                                .disabled(self.settings.busy.is_some())
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.settings_git_check(window, cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("settings-git-init")
                                                .outline()
                                                .xsmall()
                                                .label("初始化仓库")
                                                .disabled(self.settings.busy.is_some())
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.settings_git_init(window, cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("settings-git-sync")
                                                .outline()
                                                .xsmall()
                                                .label("立即同步")
                                                .disabled(self.settings.busy.is_some())
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.settings_git_sync(window, cx)
                                                })),
                                        ),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_3()
                                .p_4()
                                .rounded_md()
                                .border_1()
                                .border_color(cx.theme().border)
                                .child(self.section_header(
                                    "备份",
                                    "把当前文件夹（或未保存的文稿列表）打包成 zip。",
                                    cx,
                                ))
                                .child(self.settings_field("备份目录", "留空使用默认目录", &self.settings.backup_dir, cx))
                                .child(self.toggle_row(
                                    "auto-backup",
                                    "保存后自动备份",
                                    "频繁保存时会产生多个 zip，建议只做手动备份",
                                    self.settings.auto_backup,
                                    true,
                                    cx,
                                    |this| {
                                        this.settings.auto_backup =
                                            !this.settings.auto_backup;
                                    },
                                ))
                                .child(
                                    Button::new("settings-backup-now")
                                        .outline()
                                        .xsmall()
                                        .icon(IconName::Archive)
                                        .label("立即备份")
                                        .w(px(140.))
                                        .disabled(self.settings.busy.is_some())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.settings_backup(window, cx)
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("AppSecret、Token 等密钥以明文保存在本机配置文件中，请勿把该文件提交到公开仓库。"),
                        ),
                )
                .child(
                    h_flex()
                        .h(px(60.))
                        .flex_shrink_0()
                        .px_5()
                        .gap_3()
                        .items_center()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .when_some(self.settings.busy.clone().or(self.settings.notice.clone()), |this, message| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(message),
                                    )
                                })
                                .when(self.settings.busy.is_none() && self.settings.notice.is_none(), |this| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .truncate()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "自动保存 · {}",
                                                config::config_path().display()
                                            )),
                                    )
                                }),
                        )
                        .child(
                            Button::new("settings-save")
                                .primary()
                                .xsmall()
                                .label("完成")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.save_settings(window, cx)
                                })),
                        ),
                ),
        )
        .into_any_element()
    }

    fn section_header(
        &self,
        title: &'static str,
        description: &'static str,
        cx: &App,
    ) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(div().text_sm().font_semibold().child(title))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(description),
            )
    }

    fn settings_field(
        &self,
        label: &'static str,
        hint: &'static str,
        input: &Entity<InputState>,
        cx: &App,
    ) -> impl IntoElement {
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_baseline()
                    .child(div().text_xs().font_semibold().child(label))
                    .when(!hint.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(hint),
                        )
                    }),
            )
            .child(Input::new(input).w_full().small())
    }

    fn host_kind_button(
        &self,
        label: &'static str,
        kind: config::ImageHostKind,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(label)
            .ghost()
            .xsmall()
            .label(label)
            .selected(self.settings.image_kind == kind)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings.image_kind = kind;
                this.flush_settings(cx).ok();
                cx.notify();
            }))
    }

    fn toggle_row<F>(
        &self,
        id: &'static str,
        label: &'static str,
        hint: &'static str,
        checked: bool,
        persist: bool,
        cx: &mut Context<Self>,
        toggle: F,
    ) -> impl IntoElement
    where
        F: Fn(&mut Self) + 'static,
    {
        h_flex()
            .id(id)
            .w_full()
            .py_1()
            .gap_3()
            .items_center()
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _, _, cx| {
                toggle(this);
                if persist {
                    this.flush_settings(cx).ok();
                }
                cx.notify();
            }))
            .child(
                v_flex()
                    .flex_1()
                    .child(div().text_xs().font_semibold().child(label))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(hint),
                    ),
            )
            .child(
                div()
                    .size(px(18.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(if checked {
                        cx.theme().primary
                    } else {
                        cx.theme().border
                    })
                    .when(checked, |this| {
                        this.bg(cx.theme().primary)
                            .flex()
                            .items_center()
                            .justify_center()
                    })
                    .when(checked, |this| {
                        this.child(
                            Icon::new(IconName::Check)
                                .with_size(px(12.))
                                .text_color(cx.theme().primary_foreground),
                        )
                    }),
            )
    }

    fn publish_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let config = cx
            .try_global::<config::AppConfig>()
            .cloned()
            .unwrap_or_default();
        self.modal_shell("publish-overlay", cx).child(
            v_flex()
                .id("publish-card")
                .w(px(760.))
                .h(relative(0.92))
                .max_h(px(760.))
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .overflow_hidden()
                .occlude()
                .child(
                    h_flex()
                        .h(px(58.))
                        .flex_shrink_0()
                        .px_5()
                        .items_center()
                        .justify_between()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            v_flex()
                                .gap_0()
                                .child(div().text_base().font_semibold().child("发布中心"))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("公众号/博客园走 API，其余平台复制排版并打开发布页"),
                                ),
                        )
                        .child(
                            Button::new("publish-close")
                                .ghost()
                                .xsmall()
                                .icon(IconName::X)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.publish_open = false;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    v_flex()
                        .id("publish-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .p_5()
                        .gap_4()
                        .child(self.settings_field(
                            "文章标题",
                            "公众号和博客园会使用这个标题",
                            &self.publish.title,
                            cx,
                        ))
                        .child(
                            v_flex()
                                .gap_2()
                                .child(self.section_header(
                                    "发布平台",
                                    "可多选。公众号和博客园需要先在设置里填好凭证。",
                                    cx,
                                ))
                                .children(
                                    publish::Platform::ALL
                                        .into_iter()
                                        .map(|platform| self.platform_row(platform, &config, cx))
                                        .collect::<Vec<_>>(),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .child(self.section_header("选项", "", cx))
                                .child(self.toggle_row(
                                    "publish-upload-images",
                                    "上传本地图片到图床",
                                    "发布前把本地相对路径图片上传，并改写 Markdown 源文件",
                                    self.publish.upload_images,
                                    false,
                                    cx,
                                    |this| {
                                        this.publish.upload_images =
                                            !this.publish.upload_images;
                                    },
                                )),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(self.section_header(
                                    "输出",
                                    "公众号草稿会打开公众号后台；浏览器平台会打开编辑页并复制富文本。",
                                    cx,
                                ))
                                .child(
                                    v_flex()
                                        .id("publish-log")
                                        .w_full()
                                        .min_h(px(110.))
                                        .max_h(px(180.))
                                        .overflow_y_scroll()
                                        .p_3()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .bg(cx.theme().muted)
                                        .gap_1()
                                        .when(self.publish.status.is_empty(), |this| {
                                            this.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("还没有发布记录。首次使用知乎/头条/CSDN 时，请先在打开的页面登录。"),
                                            )
                                        })
                                        .children(
                                            self.publish
                                                .status
                                                .iter()
                                                .map(|line| {
                                                    div()
                                                        .text_xs()
                                                        .font_family(cx.theme().mono_font_family.clone())
                                                        .text_color(if line.starts_with('!') {
                                                            cx.theme().danger
                                                        } else {
                                                            cx.theme().foreground
                                                        })
                                                        .child(line.clone())
                                                        .into_any_element()
                                                })
                                                .collect::<Vec<_>>(),
                                        ),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .h(px(60.))
                        .flex_shrink_0()
                        .px_5()
                        .gap_3()
                        .items_center()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .flex_1()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if self.publish.busy {
                                    "发布中，请稍候…"
                                } else {
                                    "知乎/头条/CSDN 等平台需要浏览器里已登录账号。"
                                }),
                        )
                        .child(
                            Button::new("publish-copy")
                                .outline()
                                .xsmall()
                                .label("只复制排版")
                                .disabled(self.publish.busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.copy_publish(window, cx)
                                })),
                        )
                        .child(
                            div()
                                .id("publish-cancel")
                                .h(px(30.))
                                .px(px(14.))
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .cursor(CursorStyle::PointingHand)
                                .hover(|style| style.bg(cx.theme().muted))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.publish_open = false;
                                    cx.notify();
                                }))
                                .child("关闭"),
                        )
                        .child(
                            Button::new("publish-start")
                                .primary()
                                .xsmall()
                                .label(if self.publish.busy {
                                    "发布中…"
                                } else if self
                                    .publish
                                    .targets
                                    .selected()
                                    .iter()
                                    .all(|platform| !platform.uses_api())
                                {
                                    "复制排版并打开发布页"
                                } else {
                                    "开始发布"
                                })
                                .disabled(self.publish.busy || !self.publish.targets.any())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.start_publish(window, cx)
                                })),
                        ),
                ),
        )
        .into_any_element()
    }

    fn platform_row(
        &self,
        platform: publish::Platform,
        config: &config::AppConfig,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.publish.targets.is_selected(platform);
        let api_ready = match platform {
            publish::Platform::Wechat => {
                !config.wechat.app_id.trim().is_empty()
                    && !config.wechat.app_secret.trim().is_empty()
            }
            publish::Platform::Cnblogs => {
                !config.cnblogs.username.trim().is_empty()
                    && !config.cnblogs.token.trim().is_empty()
            }
            _ => false,
        };
        let tag = match platform {
            publish::Platform::Wechat => {
                if api_ready {
                    "草稿 API"
                } else {
                    "复制排版"
                }
            }
            publish::Platform::Cnblogs => {
                if api_ready {
                    "MetaWeblog"
                } else {
                    "复制排版"
                }
            }
            _ => "浏览器粘贴",
        };
        let description = match platform {
            publish::Platform::Wechat => "上传图片与封面，创建公众号图文草稿",
            publish::Platform::Zhihu => "复制富文本，打开知乎写作页",
            publish::Platform::Toutiao => "复制富文本，打开头条发布页",
            publish::Platform::Csdn => "复制富文本，打开 CSDN 编辑器",
            publish::Platform::Cnblogs => "通过 MetaWeblog 直接发布",
            publish::Platform::Jianshu => "复制富文本，打开简书编辑器",
            publish::Platform::Juejin => "复制富文本，打开掘金编辑器",
        };
        let accent = cx.theme().accent_foreground;

        div()
            .id(platform.key())
            .w_full()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(if selected {
                accent.opacity(0.45)
            } else {
                cx.theme().border
            })
            .bg(if selected {
                accent.opacity(0.10)
            } else {
                cx.theme().transparent
            })
            .cursor(CursorStyle::PointingHand)
            .on_click(cx.listener(move |this, _, _, cx| {
                let now = this.publish.targets.is_selected(platform);
                this.publish.targets.set(platform, !now);
                cx.notify();
            }))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .size(px(18.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(if selected { accent } else { cx.theme().border })
                            .when(selected, |this| {
                                this.bg(accent).flex().items_center().justify_center()
                            })
                            .when(selected, |this| {
                                this.child(
                                    Icon::new(IconName::Check)
                                        .with_size(px(12.))
                                        .text_color(cx.theme().popover),
                                )
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .child(div().text_sm().font_semibold().child(platform.label()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(description),
                            ),
                    )
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded_full()
                            .bg(cx.theme().muted)
                            .text_xs()
                            .text_color(if api_ready {
                                cx.theme().accent_foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .child(tag),
                    ),
            )
            .into_any_element()
    }

    fn close_confirm_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dirty) = self.close_confirm else {
            return div().into_any_element();
        };
        self.modal_shell("quit-confirm-overlay", cx)
            .child(
                v_flex()
                    .w(px(440.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .overflow_hidden()
                    .occlude()
                    .child(
                        h_flex()
                            .px_5()
                            .pt_5()
                            .gap_3()
                            .items_start()
                            .child(
                                Icon::new(IconName::TriangleAlert)
                                    .with_size(px(18.))
                                    .text_color(cx.theme().warning),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .gap_1()
                                    .child(
                                        div().text_base().font_semibold().child("还有未保存的修改"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{dirty} 个文档有未保存的内容，直接退出将会丢失。"
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .h(px(58.))
                            .px_5()
                            .gap_2()
                            .items_center()
                            .justify_end()
                            .child(
                                Button::new("quit-cancel")
                                    .ghost()
                                    .xsmall()
                                    .label("留在编辑")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.close_confirm = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("quit-confirm")
                                    .danger()
                                    .xsmall()
                                    .label("退出")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.quit_confirmed = true;
                                        cx.quit();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn prompt_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(prompt) = self.prompt.as_ref() else {
            return div().into_any_element();
        };
        let danger = matches!(prompt.kind, PromptKind::Delete(_));
        let needs_input = prompt.needs_input;

        self.modal_shell("prompt-overlay", cx)
            .child(
                v_flex()
                    .w(px(440.))
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().popover)
                    .overflow_hidden()
                    .occlude()
                    .child(
                        v_flex()
                            .p_5()
                            .gap_3()
                            .child(
                                div()
                                    .text_base()
                                    .font_semibold()
                                    .child(prompt.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(prompt.hint.clone()),
                            )
                            .when(needs_input, |this| {
                                this.child(Input::new(&prompt.input).w_full().small())
                            })
                            .when_some(prompt.error.clone(), |this, error| {
                                this.child(
                                    div().text_xs().text_color(cx.theme().danger).child(error),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .h(px(54.))
                            .px_5()
                            .gap_2()
                            .items_center()
                            .justify_end()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(
                                Button::new("prompt-cancel")
                                    .ghost()
                                    .xsmall()
                                    .label("取消")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.prompt = None;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("prompt-confirm")
                                    .xsmall()
                                    .label(if danger { "删除" } else { "确定" })
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.apply_prompt(window, cx)
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn doc_item(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let doc = &self.docs[index];
        let active = index == self.active;
        let dirty = doc.is_dirty();
        let title = doc.title.clone();
        let accent = cx.theme().accent_foreground;
        let icon_color = if active {
            cx.theme().sidebar_accent_foreground
        } else {
            cx.theme().muted_foreground
        };

        div()
            .id(("doc-item", index))
            .w_full()
            .px_2()
            .py(px(5.))
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .cursor(CursorStyle::PointingHand)
            .when(active, |this| {
                this.bg(cx.theme().sidebar_accent)
                    .text_color(cx.theme().sidebar_accent_foreground)
            })
            .when(!active, |this| {
                this.text_color(cx.theme().sidebar_foreground)
                    .hover(|style| style.bg(cx.theme().sidebar_accent.opacity(0.55)))
            })
            .on_click(cx.listener(move |this, _, window, cx| this.activate_doc(index, window, cx)))
            .child(Icon::new(IconName::File).xsmall().text_color(icon_color))
            .child(div().flex_1().min_w_0().truncate().text_sm().child(title))
            .when(dirty, |this| this.child(widgets::dot(accent)))
            .into_any_element()
    }

    fn outline_item(&self, heading: &Heading, active: bool, cx: &mut Context<Self>) -> AnyElement {
        let offset = heading.offset;
        let title = heading.title.clone();
        let click_title = title.clone();
        let level = heading.level;
        let indent = 8.0 + (level.saturating_sub(1) as f32) * 10.0;
        let accent = cx.theme().accent_foreground;
        let muted = cx.theme().muted_foreground;

        div()
            .id(("outline-item", offset))
            .w_full()
            .pl(px(indent))
            .pr_2()
            .py_1()
            .rounded_md()
            .flex()
            .items_center()
            .gap_2()
            .cursor(CursorStyle::PointingHand)
            .when(active, |this| this.bg(cx.theme().sidebar_accent))
            .when(!active, |this| {
                this.hover(|style| style.bg(cx.theme().sidebar_accent.opacity(0.5)))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.jump_to(offset, click_title.clone(), window, cx)
            }))
            .child(
                div()
                    .w(px(2.))
                    .h(px(11.))
                    .rounded_full()
                    .flex_shrink_0()
                    .bg(if active {
                        accent
                    } else {
                        cx.theme().transparent
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(if active { accent } else { muted })
                    .when(level <= 2, |this| this.font_semibold())
                    .child(title),
            )
            .into_any_element()
    }

    fn sidebar_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let progress = (self.stats.chars as f32 / 1000.0).clamp(0.015, 1.0);
        v_flex()
            .mx_2()
            .mb_2()
            .mt_1()
            .p_3()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().sidebar_border)
            .bg(cx.theme().sidebar_accent.opacity(0.55))
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("本篇"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().accent_foreground)
                            .child(if self.stats.minutes == 0 {
                                "等待落笔".to_string()
                            } else {
                                format!("约 {} 分钟", self.stats.minutes)
                            }),
                    ),
            )
            .child(
                h_flex()
                    .items_baseline()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_semibold()
                            .child(widgets::number(self.stats.chars)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("字符"),
                    ),
            )
            .child(
                div()
                    .h(px(3.))
                    .w_full()
                    .mt_1()
                    .rounded_full()
                    .bg(cx.theme().sidebar_border)
                    .child(
                        div()
                            .h_full()
                            .w(relative(progress))
                            .rounded_full()
                            .bg(cx.theme().accent_foreground),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} 行 · 目标 1,000 字",
                        widgets::number(self.stats.lines)
                    )),
            )
    }

    fn panel_header(&self, preview: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let accent = if preview {
            cx.theme().success
        } else {
            cx.theme().accent_foreground
        };
        h_flex()
            .h(px(36.))
            .w_full()
            .flex_shrink_0()
            .px_4()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .child(widgets::dot(accent))
            .child(
                div()
                    .text_xs()
                    .font_semibold()
                    .text_color(cx.theme().muted_foreground)
                    .child(if preview { "PREVIEW" } else { "MARKDOWN" }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground.opacity(0.7))
                    .child(if preview { "实时预览" } else { "源码" }),
            )
            .child(div().flex_1())
            .when(preview, |this| {
                this.child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Button::new("preview-find-toggle")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Search)
                                .tooltip("在预览中查找 · Ctrl+F")
                                .selected(self.preview_find_open)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.toggle_preview_find(window, cx)
                                })),
                        )
                        .child(
                            Button::new("zoom-out")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Minus)
                                .tooltip("缩小预览")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.preview_zoom = (this.preview_zoom - 0.1).max(0.8);
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .w(px(38.))
                                .text_xs()
                                .text_center()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{:.0}%", self.preview_zoom * 100.)),
                        )
                        .child(
                            Button::new("zoom-in")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Plus)
                                .tooltip("放大预览")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.preview_zoom = (this.preview_zoom + 0.1).min(1.4);
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(!preview, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{} 行", widgets::number(self.stats.lines))),
                )
            })
    }

    fn editor_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let handle = cx.weak_entity();
        v_flex()
            .size_full()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .overflow_hidden()
            .child(self.panel_header(false, cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        Editor::new(&self.editor)
                            .on_paste(move |item, window, cx| {
                                let Some(image) = clipboard_image(item) else {
                                    return false;
                                };
                                handle
                                    .update(cx, |this, cx| this.paste_image(image, window, cx))
                                    .ok();
                                true
                            })
                            .appearance(false)
                            .bordered(false)
                            .h(relative(1.))
                            .p_0()
                            .text_size(px(14.)),
                    )
                    .when(self.slash.is_some(), |this| {
                        this.child(self.slash_palette(cx))
                    }),
            )
    }

    fn slash_row(
        &self,
        entry: SlashEntry,
        index: usize,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let badge = entry.badge;
        let icon = entry.icon;
        let name = entry.name.clone();
        let description = entry.description.clone();
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id(("slash-row", index))
            .w_full()
            .px_2()
            .py(px(6.))
            .rounded_md()
            .gap_3()
            .items_center()
            .cursor(CursorStyle::PointingHand)
            .when(selected, |this| this.bg(cx.theme().accent))
            .when(!selected, |this| {
                this.hover(|style| style.bg(cx.theme().muted))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(state) = this.slash.as_mut() {
                    state.selected = index;
                }
                this.accept_slash(window, cx);
            }))
            .child(
                div()
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded_md()
                    .bg(cx.theme().muted)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(icon)
                            .with_size(px(14.))
                            .text_color(cx.theme().accent_foreground),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0()
                    .child(div().text_sm().font_semibold().truncate().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(description),
                    ),
            )
            .when_some(badge, |this, badge| {
                this.child(
                    div()
                        .px_1()
                        .py(px(1.))
                        .rounded(px(3.))
                        .bg(cx.theme().muted)
                        .text_xs()
                        .text_color(muted)
                        .child(badge),
                )
            })
            .into_any_element()
    }

    fn slash_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        let entries = self.slash_entries();
        let count = entries.len();
        let selected = self
            .slash
            .as_ref()
            .map(|state| state.selected.min(count.saturating_sub(1)))
            .unwrap_or(0);
        let query = self
            .slash
            .as_ref()
            .map(|state| state.query.clone())
            .unwrap_or_default();
        let muted = cx.theme().muted_foreground;

        let rows = if entries.is_empty() {
            vec![
                v_flex()
                    .px_3()
                    .py_4()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().text_color(muted).child("没有匹配的指令"))
                    .child(
                        Button::new("slash-generate")
                            .outline()
                            .xsmall()
                            .icon(IconName::FilePlus)
                            .label("生成命令模板")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.slash.is_none() {
                                    return;
                                }
                                if this.take_slash_trigger(window, cx) {
                                    this.execute_slash_action(
                                        SlashAction::Builtin(BuiltinCommand::NewCommandTemplates),
                                        String::new(),
                                        window,
                                        cx,
                                    );
                                }
                            })),
                    )
                    .into_any_element(),
            ]
        } else {
            entries
                .into_iter()
                .enumerate()
                .map(|(index, entry)| self.slash_row(entry, index, index == selected, cx))
                .collect()
        };

        v_flex()
            .id("slash-palette")
            .absolute()
            .top(px(10.))
            .left(px(8.))
            .w(relative(0.94))
            .max_w(px(430.))
            .max_h(px(400.))
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .overflow_hidden()
            .occlude()
            .child(
                h_flex()
                    .h(px(46.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Icon::new(IconName::Command)
                            .with_size(px(14.))
                            .text_color(cx.theme().accent_foreground),
                    )
                    .child(div().text_sm().font_semibold().child("快捷指令"))
                    .child(div().flex_1())
                    .child(
                        div()
                            .max_w(px(180.))
                            .truncate()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(muted)
                            .child(format!("/{query}")),
                    )
                    .child(div().text_xs().text_color(muted).child(format!("{count}"))),
            )
            .child(
                v_flex()
                    .id("slash-list")
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .p_2()
                    .gap_1()
                    .children(rows),
            )
            .child(
                v_flex()
                    .px_3()
                    .py_2()
                    .gap_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("↑↓ 选择 · Enter/Tab 插入 · Esc 关闭"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted.opacity(0.75))
                            .child("自定义 JS/Python 以当前用户权限运行 · 输入「命令」管理"),
                    ),
            )
            .into_any_element()
    }

    fn workbench_card<F>(
        &self,
        id: &'static str,
        icon: IconName,
        title: &'static str,
        detail: &'static str,
        cx: &mut Context<Self>,
        handler: F,
    ) -> AnyElement
    where
        F: Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    {
        div()
            .id(id)
            .flex_1()
            .min_w_0()
            .p_3()
            .gap_1()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().muted)
            .cursor(CursorStyle::PointingHand)
            .hover(|style| style.bg(cx.theme().accent))
            .on_click(cx.listener(move |this, _, window, cx| handler(this, window, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(icon)
                            .with_size(px(15.))
                            .text_color(cx.theme().accent_foreground),
                    )
                    .child(div().text_sm().font_semibold().child(title)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(detail),
            )
            .into_any_element()
    }

    fn empty_workbench(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = vec![
            h_flex()
                .w_full()
                .gap_2()
                .children(vec![
                    self.workbench_card(
                        "workbench-template",
                        IconName::LayoutTemplate,
                        "从模板开始",
                        "文章、技术笔记、周报、会议纪要",
                        cx,
                        |this, _, cx| {
                            this.slash = None;
                            this.templates_open = true;
                            cx.notify();
                        },
                    ),
                    self.workbench_card(
                        "workbench-open",
                        IconName::FileText,
                        "打开 Markdown",
                        "继续写本地已有文件",
                        cx,
                        |this, window, cx| this.open_document(window, cx),
                    ),
                ])
                .into_any_element(),
            h_flex()
                .w_full()
                .gap_2()
                .children(vec![
                    self.workbench_card(
                        "workbench-folder",
                        IconName::FolderOpen,
                        "打开文件夹",
                        "把整个文稿目录放进来",
                        cx,
                        |this, window, cx| this.open_folder(window, cx),
                    ),
                    self.workbench_card(
                        "workbench-ai",
                        IconName::Sparkles,
                        "AI 写作",
                        "给一个主题，先生成初稿",
                        cx,
                        |this, window, cx| this.open_ai(window, cx),
                    ),
                ])
                .into_any_element(),
        ];

        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .px_6()
            .gap_5()
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::new(IconName::Feather)
                            .with_size(px(32.))
                            .text_color(cx.theme().accent_foreground.opacity(0.75)),
                    )
                    .child(div().text_lg().font_semibold().child("从一张空白稿纸开始"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("选一个模板，或者直接在左侧写下第一行"),
                    ),
            )
            .child(v_flex().w_full().max_w(px(470.)).gap_2().children(rows))
            .child(
                Button::new("workbench-samples")
                    .ghost()
                    .xsmall()
                    .icon(IconName::BookOpen)
                    .label("载入示例文稿")
                    .on_click(cx.listener(|this, _, window, cx| this.load_samples(window, cx))),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground.opacity(0.7))
                    .child("Ctrl+N 新建 · Ctrl+O 打开 · 输入 / 快捷指令"),
            )
            .into_any_element()
    }

    fn preview_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.docs[self.active].content.trim().is_empty();
        let zoom = self.preview_zoom;
        let doc_dir = std::sync::Arc::new(self.current_doc_dir());
        let code_lang_handle = cx.weak_entity();
        let mut code_block_style = TextViewStyle::default();
        code_block_style.code_block = gpui_kit::StyleRefinement::default().pt(px(28.));

        v_flex()
            .size_full()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .overflow_hidden()
            .child(self.panel_header(true, cx))
            .when(self.preview_find_open, |this| {
                this.child(self.preview_find_bar(cx))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(empty, |this| this.child(self.empty_workbench(cx)))
                    .when(!empty, |this| {
                        this.child(
                            TextView::new(&self.preview)
                                .selectable(true)
                                .scrollable(true)
                                .selection_format(SelectionFormat::Source)
                                .markdown_extensions(MarkdownExtensions::default().frontmatter())
                                .plugin(FrontmatterPlugin::new())
                                // The language chip sits in the top-right
                                // corner, so reserve the same band above the
                                // first code line that the built-in language
                                // label reserves.
                                .style(code_block_style)
                                .code_block_actions(move |block, _window, cx| {
                                    let current = block
                                        .lang()
                                        .map(|lang| lang.to_string())
                                        .unwrap_or_default();
                                    let fence = block.span.map(|span| span.start);
                                    let label = if current.is_empty() {
                                        "语言".to_string()
                                    } else {
                                        doc::code_language_display(&current)
                                            .map(str::to_string)
                                            .unwrap_or_else(|| current.clone())
                                    };
                                    let handle = code_lang_handle.clone();
                                    let muted = cx.theme().muted_foreground;
                                    let hovered = cx.theme().foreground;
                                    div()
                                        .id("code-lang-chip")
                                        .flex()
                                        .items_center()
                                        .h(px(18.))
                                        .px(px(7.))
                                        .rounded(px(4.))
                                        .text_size(px(11.))
                                        .line_height(px(16.))
                                        .font_family(cx.theme().font_family.clone())
                                        .text_color(muted)
                                        .cursor_pointer()
                                        .hover(move |style| style.text_color(hovered))
                                        .child(label)
                                        .on_click(move |_, _, cx| {
                                            let Some(fence) = fence else {
                                                return;
                                            };
                                            let _ = handle.update(cx, |this, cx| {
                                                this.open_code_lang(fence, current.clone(), cx)
                                            });
                                        })
                                })
                                .image_source(move |uri| {
                                    let source = uri.to_string();
                                    if let Some(path) =
                                        publish::local_path(&source, doc_dir.as_ref())
                                    {
                                        if path.exists() {
                                            return ImageSource::from(path);
                                        }
                                    }
                                    ImageSource::from(source)
                                })
                                .text_size(px(16. * zoom))
                                .size_full()
                                .px(px(38.))
                                .pt(px(34.))
                                .pb(px(64.)),
                        )
                    }),
            )
    }

    fn workspace(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.view_mode {
            ViewMode::Write => div()
                .size_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(880.))
                        .h_full()
                        .child(self.editor_card(cx)),
                )
                .into_any_element(),
            ViewMode::Preview => div()
                .size_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(820.))
                        .h_full()
                        .child(self.preview_card(cx)),
                )
                .into_any_element(),
            ViewMode::Split => div()
                .size_full()
                .child(
                    h_resizable("inkdown-split")
                        .child(
                            resizable_panel()
                                .child(div().size_full().pr(px(3.)).child(self.editor_card(cx))),
                        )
                        .child(
                            resizable_panel()
                                .child(div().size_full().pl(px(3.)).child(self.preview_card(cx))),
                        ),
                )
                .into_any_element(),
        };

        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .p_3()
            .bg(cx.theme().muted)
            .child(body)
    }

    fn status_bar(&self, line: usize, column: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let doc = &self.docs[self.active];
        let dirty = doc.is_dirty();

        h_flex()
            .h(px(28.))
            .w_full()
            .flex_shrink_0()
            .px_3()
            .gap_2()
            .items_center()
            .bg(cx.theme().status_bar)
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                Icon::new(if dirty {
                    IconName::CircleDot
                } else {
                    IconName::CircleCheck
                })
                .with_size(px(12.))
                .text_color(if dirty {
                    cx.theme().accent_foreground
                } else {
                    cx.theme().success
                }),
            )
            .child(if dirty { "未保存" } else { "已保存" })
            .child(widgets::dot(cx.theme().border))
            .child("Markdown")
            .child(widgets::dot(cx.theme().border))
            .child(format!("第 {line} 行，第 {column} 列"))
            .child(div().flex_1())
            .when_some(self.notice.clone(), |this, notice| {
                this.child(notice).child(widgets::dot(cx.theme().border))
            })
            .child(format!("{} 字符", widgets::number(self.stats.chars)))
            .child(widgets::dot(cx.theme().border))
            .child("UTF-8")
            .child(widgets::dot(cx.theme().border))
            .child(if self.stats.minutes == 0 {
                "—".to_string()
            } else {
                format!("约 {} 分钟", self.stats.minutes)
            })
    }

    // ---- command handlers ----------------------------------------------

    fn dispatch_editor_action(
        &mut self,
        action: Box<dyn Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_editor(window, cx);
        window.dispatch_action(action, cx);
    }

    fn on_new(&mut self, _: &NewDocument, window: &mut Window, cx: &mut Context<Self>) {
        self.new_document(window, cx);
    }

    fn on_new_from_template(
        &mut self,
        _: &NewFromTemplate,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.slash = None;
        self.templates_open = true;
        cx.notify();
    }

    fn on_open(&mut self, _: &OpenDocument, window: &mut Window, cx: &mut Context<Self>) {
        self.open_document(window, cx);
    }

    fn on_save(&mut self, _: &SaveDocument, window: &mut Window, cx: &mut Context<Self>) {
        self.save(window, cx);
    }

    fn on_save_as(&mut self, _: &SaveDocumentAs, window: &mut Window, cx: &mut Context<Self>) {
        self.save_as(window, cx);
    }

    fn on_export(&mut self, _: &ExportHtml, window: &mut Window, cx: &mut Context<Self>) {
        self.export_html(window, cx);
    }

    fn on_toggle_theme(&mut self, _: &ToggleTheme, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_theme(cx);
    }

    fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        theme::toggle(cx);
        let dark = cx.theme().mode.is_dark();
        cx.global_mut::<config::AppConfig>().dark_mode = dark;
        config::save(cx).ok();
        cx.notify();
    }

    fn on_toggle_sidebar_action(
        &mut self,
        _: &ToggleSidebar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_sidebar(window, cx);
    }

    fn on_view_write(&mut self, _: &ViewWrite, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(ViewMode::Write, window, cx);
    }

    fn on_view_split(&mut self, _: &ViewSplit, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(ViewMode::Split, window, cx);
    }

    fn on_view_preview(&mut self, _: &ViewPreview, window: &mut Window, cx: &mut Context<Self>) {
        self.set_view(ViewMode::Preview, window, cx);
    }

    fn on_format_bold(&mut self, _: &FormatBold, window: &mut Window, cx: &mut Context<Self>) {
        self.wrap_selection("**", "**", "粗体", window, cx);
    }

    fn on_format_italic(&mut self, _: &FormatItalic, window: &mut Window, cx: &mut Context<Self>) {
        self.wrap_selection("*", "*", "斜体", window, cx);
    }

    fn on_format_strike(&mut self, _: &FormatStrike, window: &mut Window, cx: &mut Context<Self>) {
        self.wrap_selection("~~", "~~", "删除线", window, cx);
    }

    fn on_format_inline_code(
        &mut self,
        _: &FormatInlineCode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.wrap_selection("`", "`", "代码", window, cx);
    }

    fn on_format_code_block(
        &mut self,
        _: &FormatCodeBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.code_block(window, cx);
    }

    fn on_format_link(&mut self, _: &FormatLink, window: &mut Window, cx: &mut Context<Self>) {
        self.insert_link(window, cx);
    }

    fn on_format_h1(&mut self, _: &FormatHeading1, window: &mut Window, cx: &mut Context<Self>) {
        self.set_heading(1, window, cx);
    }

    fn on_format_h2(&mut self, _: &FormatHeading2, window: &mut Window, cx: &mut Context<Self>) {
        self.set_heading(2, window, cx);
    }

    fn on_format_quote(&mut self, _: &FormatQuote, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_line_prefix("> ", window, cx);
    }

    fn on_format_bullet(
        &mut self,
        _: &FormatBulletList,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_line_prefix("- ", window, cx);
    }

    fn on_format_ordered(
        &mut self,
        _: &FormatOrderedList,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_line_prefix("1. ", window, cx);
    }

    fn on_format_table(&mut self, _: &FormatTable, window: &mut Window, cx: &mut Context<Self>) {
        self.insert_template(TABLE_TEMPLATE, window, cx);
    }

    fn on_format_divider(
        &mut self,
        _: &FormatDivider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.insert_template("\n---\n", window, cx);
    }

    // ---- non-action entry points used by toolbar/sidebar ----------------

    fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_visible = !self.sidebar_visible;
        if self.sidebar_visible {
            self.focus_editor(window, cx);
        }
        cx.notify();
    }

    fn open_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("打开 Markdown 文件".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    this.update_in(cx, |this, window, cx| {
                        this.open_path(path, content, window, cx);
                    })
                    .ok();
                }
                Err(error) => {
                    this.update_in(cx, |this, _window, cx| {
                        this.notice = Some(format!("打开失败：{error}"));
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}

impl Render for InkdownApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cursor = self.editor.read(cx).cursor_position();
        let line = cursor.line as usize + 1;
        let column = cursor.character as usize + 1;
        let dark = cx.theme().mode.is_dark();
        let doc = &self.docs[self.active];
        let window_title = format!(
            "{}{} — Inkdown",
            if doc.is_dirty() { "• " } else { "" },
            doc.title
        );
        window.set_window_title(&window_title);

        v_flex()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.title_bar(cx))
            .child(self.menu_bar(dark, cx))
            .child(self.toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .when(self.sidebar_visible, |this| this.child(self.sidebar(cx)))
                    .child(self.workspace(cx)),
            )
            .child(self.status_bar(line, column, cx))
            .when(self.settings_open, |this| {
                this.child(self.settings_modal(cx))
            })
            .when(self.publish_open, |this| this.child(self.publish_modal(cx)))
            .when(self.ai_open, |this| this.child(self.ai_modal(cx)))
            .when(self.chat_open, |this| this.child(self.chat_modal(cx)))
            .when(self.close_confirm.is_some(), |this| {
                this.child(self.close_confirm_modal(cx))
            })
            .when(self.meta_open, |this| this.child(self.meta_modal(cx)))
            .when(self.templates_open, |this| {
                this.child(self.templates_modal(cx))
            })
            .when(self.code_lang.is_some(), |this| {
                this.child(self.code_lang_modal(cx))
            })
            .when(self.prompt.is_some(), |this| {
                this.child(self.prompt_modal(cx))
            })
            .on_action(cx.listener(Self::on_new))
            .on_action(cx.listener(Self::on_new_from_template))
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_save_as))
            .on_action(cx.listener(Self::on_export))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_toggle_sidebar_action))
            .on_action(cx.listener(Self::on_open_folder))
            .on_action(cx.listener(Self::on_refresh_folder))
            .on_action(cx.listener(Self::on_new_file))
            .on_action(cx.listener(Self::on_new_folder))
            .on_action(cx.listener(Self::on_rename_file))
            .on_action(cx.listener(Self::on_delete_file))
            .on_action(cx.listener(Self::on_reveal_in_files))
            .on_action(cx.listener(Self::on_publish_action))
            .on_action(cx.listener(Self::on_ai_write))
            .on_action(cx.listener(Self::on_open_settings_action))
            .on_action(cx.listener(Self::on_backup_action))
            .on_action(cx.listener(Self::on_git_sync_action))
            .on_action(cx.listener(Self::on_close_overlay))
            .on_action(cx.listener(Self::on_view_write))
            .on_action(cx.listener(Self::on_view_split))
            .on_action(cx.listener(Self::on_view_preview))
            .on_action(cx.listener(Self::on_format_bold))
            .on_action(cx.listener(Self::on_format_italic))
            .on_action(cx.listener(Self::on_format_strike))
            .on_action(cx.listener(Self::on_format_inline_code))
            .on_action(cx.listener(Self::on_format_code_block))
            .on_action(cx.listener(Self::on_format_link))
            .on_action(cx.listener(Self::on_format_image))
            .on_action(cx.listener(Self::on_preview_find))
            .on_action(cx.listener(Self::on_edit_metadata))
            .on_action(cx.listener(Self::on_format_h1))
            .on_action(cx.listener(Self::on_format_h2))
            .on_action(cx.listener(Self::on_format_quote))
            .on_action(cx.listener(Self::on_format_bullet))
            .on_action(cx.listener(Self::on_format_ordered))
            .on_action(cx.listener(Self::on_format_table))
            .on_action(cx.listener(Self::on_format_divider))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.open_dropped(paths, window, cx)
            }))
    }
}

const TABLE_TEMPLATE: &str = "\n| 列 1 | 列 2 | 列 3 |
| --- | --- | --- |
| 内容 | 内容 | 内容 |
";

/// The first image carried by a clipboard item, if any.
fn clipboard_image(item: &ClipboardItem) -> Option<Image> {
    item.entries.iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image.clone()),
        _ => None,
    })
}

/// A slash only opens the command palette at the start of a word, so URLs and
/// fractions keep working.
fn slash_boundary(text: &str, at: usize) -> bool {
    text[..at].chars().next_back().map_or(true, |ch| {
        ch.is_whitespace() || matches!(ch, '>' | '(' | '[' | '（' | '【')
    })
}

/// First `# ` heading, used as the generated document's title.
fn first_heading(source: &str) -> Option<String> {
    for line in source.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# ") {
            let title = rest.trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    None
}

fn adjust_cursor(cursor: usize, start: usize, removed: usize, inserted: usize) -> usize {
    if cursor >= start + removed {
        cursor - removed + inserted
    } else if cursor > start {
        start + inserted
    } else {
        cursor + inserted
    }
}
#[cfg(test)]
mod chat_tests {
    use super::chat_system_prompt;
    use crate::ai;

    #[test]
    fn chat_system_prompt_embeds_the_document() {
        assert_eq!(chat_system_prompt(""), ai::CHAT_SYSTEM_PROMPT);
        assert_eq!(chat_system_prompt("   \n  "), ai::CHAT_SYSTEM_PROMPT);

        let prompt = chat_system_prompt("  # 标题\n\n正文  ");
        assert!(prompt.starts_with(ai::CHAT_SYSTEM_PROMPT));
        assert!(prompt.contains("<document>\n# 标题\n\n正文\n</document>"));
    }
}
