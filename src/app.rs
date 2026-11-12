//! The application view: title bar, command toolbar, document sidebar,
//! editor/preview workspace and status bar.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, Sizable as _, StyledExt as _, TitleBar,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{
        Editor, EditorState, Input, InputEvent, InputState, Redo, RopeExt as _, TabSize, Textarea,
        Undo,
    },
    menu::{DropdownMenu as _, PopupMenuItem},
    resizable::{h_resizable, resizable_panel},
    text::{SelectionFormat, TextView, TextViewState},
    v_flex,
};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::actions::*;
use crate::doc::{self, Document, Heading, Stats};
use crate::publish::upload_local_images;
use crate::ui::{AiForm, PromptKind, PromptState, PublishForm, SettingsForm, SidebarTab};
use crate::{ai, backup, config, git, imagebed, publish, theme, ui, widgets, workspace};

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
    notice: Option<String>,
    _subscriptions: Vec<Subscription>,

    workspace: Option<workspace::FileTree>,
    expanded: HashSet<PathBuf>,
    selected_path: Option<PathBuf>,
    sidebar_tab: SidebarTab,

    settings_open: bool,
    settings: SettingsForm,
    ai_open: bool,
    ai: AiForm,
    publish_open: bool,
    publish: PublishForm,
    prompt: Option<PromptState>,
    action_busy: Option<String>,
}

/// Where the AI result lands.
#[derive(Clone, Copy)]
enum AiTarget {
    NewDocument,
    Insert,
}

impl InkdownApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let docs = doc::sample_documents();
        let first = docs[0].content.clone();
        let config = cx
            .try_global::<config::AppConfig>()
            .cloned()
            .unwrap_or_default();

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
        let sidebar_tab = if workspace.is_some() {
            SidebarTab::Files
        } else {
            SidebarTab::Documents
        };
        let settings = SettingsForm::new(window, cx, &config);
        let ai = AiForm::new(window, cx);
        let publish = PublishForm::new(window, cx, &docs[0].file_stem(), &config);

        let mut this = Self {
            docs,
            active: 0,
            next_id: 10,
            untitled_count: 0,
            editor: editor.clone(),
            preview,
            view_mode: ViewMode::Split,
            sidebar_visible: true,
            outline,
            stats,
            preview_zoom: 1.0,
            notice: None,
            _subscriptions: Vec::new(),
            workspace,
            expanded,
            selected_path: None,
            sidebar_tab,
            settings_open: false,
            settings,
            ai_open: false,
            ai,
            publish_open: false,
            publish,
            prompt: None,
            action_busy: None,
        };

        let change_sub = cx.subscribe(&editor, |this, _editor, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_editor(cx);
            }
        });
        // The status bar and the outline follow the caret, which can move
        // without changing the text, so listen to every editor notification.
        let cursor_sub = cx.observe(&editor, |_, _, cx| cx.notify());
        this._subscriptions.push(change_sub);
        this._subscriptions.push(cursor_sub);

        let focus = this.editor.read(cx).focus_handle(cx);
        window.defer(cx, move |window, cx| focus.focus(window, cx));
        this
    }

    fn sync_editor(&mut self, cx: &mut Context<Self>) {
        let value = self.editor.read(cx).value().to_string();
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.content = value.clone();
        }
        self.preview
            .update(cx, |state, cx| state.set_text(&value, cx));
        self.outline = doc::outline(&value);
        self.stats = doc::stats(&value);
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
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn new_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.untitled_count += 1;
        let id = self.next_id;
        self.next_id += 1;
        self.docs
            .push(Document::untitled(id, self.untitled_count));
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

    fn write_doc(
        &mut self,
        id: u64,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.docs.iter().position(|doc| doc.id == id) else {
            return;
        };
        let content = self.docs[index].content.clone();
        match std::fs::write(&path, content.as_bytes()) {
            Ok(()) => {
                if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                    self.docs[index].title = name.into();
                }
                self.docs[index].path = Some(path);
                self.docs[index].saved_content = content;
                self.notice = Some("已保存到磁盘".to_string());
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

    fn insert_template(
        &mut self,
        template: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |state, cx| state.insert(template, window, cx));
        self.after_edit(window, cx);
    }

    fn jump_to(&mut self, offset: usize, title: String, window: &mut Window, cx: &mut Context<Self>) {
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
        self.menu_button("menu-file", "文件").dropdown_menu(|menu, _, _| {
            menu.item(
                PopupMenuItem::new("新建文档")
                    .icon(IconName::FilePlus)
                    .action(Box::new(NewDocument)),
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
                .item(
                    PopupMenuItem::new("显示侧边栏")
                        .checked(sidebar)
                        .action(Box::new(ToggleSidebar)),
                )
                .separator()
                .item(
                    PopupMenuItem::new(if dark { "切换到浅色" } else { "切换到深色" })
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
                    .on_click(cx.listener(|this, _, window, cx| this.git_sync_from_config(window, cx))),
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
                    .tooltip(if dark { "切换到浅色" } else { "切换到深色" })
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
                    Button::new("sidebar-open")
                        .ghost()
                        .xsmall()
                        .icon(IconName::FileText)
                        .tooltip("打开 Markdown")
                        .on_click(cx.listener(|this, _, window, cx| this.open_document(window, cx))),
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
                div()
                    .px_2()
                    .py_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("这个文件夹里还没有 Markdown 文件")
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
        cx.global_mut::<config::AppConfig>().last_folder =
            Some(path.to_string_lossy().to_string());
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

    fn open_workspace_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn prompt_target_dir(&self) -> Option<PathBuf> {
        let root = self.workspace.as_ref().map(|tree| tree.root.clone())?;
        Some(workspace::target_dir(&root, self.selected_path.as_deref()))
    }

    fn open_prompt(&mut self, kind: PromptKind, window: &mut Window, cx: &mut Context<Self>) {
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
                            self.docs
                                .push(Document::untitled(self.next_id - 1, 1));
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

    // ---- AI -------------------------------------------------------------

    fn open_ai(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
                        if needs_default
                            && let Some(first) = models.first()
                        {
                            let first = first.clone();
                            this.settings.ai_model.update(cx, |state, cx| {
                                state.set_value(first, window, cx);
                            });
                        }
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
                    menu = menu.item(
                        PopupMenuItem::new(model).checked(selected).on_click(
                            move |_, window, cx| {
                                handle
                                    .update(cx, |this, cx| {
                                        let value = value.clone();
                                        this.settings.ai_model.update(cx, |state, cx| {
                                            state.set_value(value, window, cx);
                                        });
                                        cx.notify();
                                    })
                                    .ok();
                            },
                        ),
                    );
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
            self.ai.status =
                vec!["还没有选择模型，先在设置里点“拉取模型”".to_string()];
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
        let title = first_heading(content)
            .unwrap_or_else(|| format!("AI 文稿 {}", self.untitled_count));
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

    fn ai_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let model = self.settings.ai_model.read(cx).value().to_string();
        let configured = cx
            .try_global::<config::AppConfig>()
            .map(|config| {
                !config.ai.base_url.is_empty() && !config.ai.api_key.is_empty()
            })
            .unwrap_or(false);

        self.modal_shell("ai-overlay", cx).child(
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
                                        .child(div().text_xs().font_semibold().child("主题 / 要求"))
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
        let config = cx.global::<config::AppConfig>().clone();
        self.settings = SettingsForm::new(window, cx, &config);
        self.settings_open = true;
        cx.notify();
    }

    fn save_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let mut config = cx.global::<config::AppConfig>().clone();
        if let Err(error) = self.settings.apply(&mut config, cx) {
            self.settings.notice = Some(error);
            cx.notify();
            return;
        }
        *cx.global_mut::<config::AppConfig>() = config;
        match config::save(cx) {
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
                .spawn(async move {
                    publish::wechat_test(&wechat.app_id, &wechat.app_secret)
                })
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
        let label = if init { "正在初始化 Git…" } else { "正在 Git 同步…" };
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
                        this.notice = Some(
                            log.lines()
                                .last()
                                .unwrap_or("Git 操作完成")
                                .to_string(),
                        );
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
        let title = self.docs[self.active].file_stem();
        let config = cx.global::<config::AppConfig>().clone();
        self.publish.reset(&title, _window, cx);
        self.publish.targets.wechat = !config.wechat.app_id.trim().is_empty()
            && !config.wechat.app_secret.trim().is_empty();
        self.publish.targets.cnblogs = !config.cnblogs.username.trim().is_empty()
            && !config.cnblogs.token.trim().is_empty();
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
                    this.publish.status.drain(0..this.publish.status.len() - 100);
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
                    let html = publish::render_inline_html(
                        &source,
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
                    Err(error) => this
                        .publish
                        .status
                        .push(format!("! 复制失败：{error}")),
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
        let path = self.selected_path.clone().or_else(|| self.docs[self.active].path.clone());
        let Some(path) = path else {
            self.notice = Some("先在文件树里选中一个文件或文件夹".to_string());
            cx.notify();
            return;
        };
        self.open_prompt(PromptKind::Rename(path), window, cx);
    }

    fn on_delete_file(&mut self, _: &DeleteFile, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.selected_path.clone().or_else(|| self.docs[self.active].path.clone());
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
        if self.settings_open {
            self.settings_open = false;
            cx.notify();
        }
    }

    fn on_publish_action(&mut self, _: &PublishArticle, window: &mut Window, cx: &mut Context<Self>) {
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
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings_open = false;
                                    cx.notify();
                                })),
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
                                }),
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
                                            .child(config::config_path().display().to_string()),
                                    )
                                }),
                        )
                        .child(
                            Button::new("settings-cancel")
                                .ghost()
                                .xsmall()
                                .label("取消")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings_open = false;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("settings-save")
                                .primary()
                                .xsmall()
                                .label("保存设置")
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
                cx.notify();
            }))
    }

    fn toggle_row<F>(
        &self,
        id: &'static str,
        label: &'static str,
        hint: &'static str,
        checked: bool,
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
                        this.bg(cx.theme().primary).flex().items_center().justify_center()
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
                            .border_color(if selected {
                                accent
                            } else {
                                cx.theme().border
                            })
                            .when(selected, |this| {
                                this.bg(accent)
                                    .flex()
                                    .items_center()
                                    .justify_center()
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

    fn prompt_modal(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(prompt) = self.prompt.as_ref() else {
            return div().into_any_element();
        };
        let danger = matches!(prompt.kind, PromptKind::Delete(_));
        let needs_input = prompt.needs_input;

        self.modal_shell("prompt-overlay", cx).child(
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
                        .child(div().text_base().font_semibold().child(prompt.title.clone()))
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
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .child(error),
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
            .on_click(
                cx.listener(move |this, _, window, cx| this.activate_doc(index, window, cx)),
            )
            .child(Icon::new(IconName::File).xsmall().text_color(icon_color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .child(title),
            )
            .when(dirty, |this| this.child(widgets::dot(accent)))
            .into_any_element()
    }

    fn outline_item(
        &self,
        heading: &Heading,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
                    .bg(if active { accent } else { cx.theme().transparent }),
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

    fn panel_header(
        &self,
        preview: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
                div().flex_1().min_h_0().child(
                    Editor::new(&self.editor)
                        .appearance(false)
                        .bordered(false)
                        .h(relative(1.))
                        .p_0()
                        .text_size(px(14.)),
                ),
            )
    }

    fn preview_card(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.docs[self.active].content.trim().is_empty();
        let zoom = self.preview_zoom;
        let doc_dir = std::sync::Arc::new(self.current_doc_dir());

        v_flex()
            .size_full()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .overflow_hidden()
            .child(self.panel_header(true, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(empty, |this| {
                        this.child(
                            v_flex()
                                .size_full()
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .child(
                                    Icon::new(IconName::Feather)
                                        .with_size(px(30.))
                                        .text_color(cx.theme().muted_foreground.opacity(0.45)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child("右边会长出你写下的世界"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground.opacity(0.65))
                                        .child("从左侧输入第一行文字"),
                                ),
                        )
                    })
                    .when(!empty, |this| {
                        this.child(
                            TextView::new(&self.preview)
                                .selectable(true)
                                .scrollable(true)
                                .selection_format(SelectionFormat::Source)
                                .image_source(move |uri| {
                                    let source = uri.to_string();
                                    if let Some(path) = publish::local_path(&source, doc_dir.as_ref())
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
                .child(div().w_full().max_w(px(880.)).h_full().child(self.editor_card(cx)))
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
                            resizable_panel().child(
                                div()
                                    .size_full()
                                    .pr(px(3.))
                                    .child(self.editor_card(cx)),
                            ),
                        )
                        .child(
                            resizable_panel().child(
                                div()
                                    .size_full()
                                    .pl(px(3.))
                                    .child(self.preview_card(cx)),
                            ),
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

    fn status_bar(
        &self,
        line: usize,
        column: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
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
            .when(self.publish_open, |this| {
                this.child(self.publish_modal(cx))
            })
            .when(self.ai_open, |this| this.child(self.ai_modal(cx)))
            .when(self.prompt.is_some(), |this| this.child(self.prompt_modal(cx)))
            .on_action(cx.listener(Self::on_new))
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
            .on_action(cx.listener(Self::on_format_h1))
            .on_action(cx.listener(Self::on_format_h2))
            .on_action(cx.listener(Self::on_format_quote))
            .on_action(cx.listener(Self::on_format_bullet))
            .on_action(cx.listener(Self::on_format_ordered))
            .on_action(cx.listener(Self::on_format_table))
            .on_action(cx.listener(Self::on_format_divider))
    }
}

const TABLE_TEMPLATE: &str = "\n| 列 1 | 列 2 | 列 3 |
| --- | --- | --- |
| 内容 | 内容 | 内容 |
";

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
