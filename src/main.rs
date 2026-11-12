// Release builds are GUI apps: without this, Windows opens a console window
// next to the app and closing it kills the process.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod ai;
mod app;
mod backup;
mod config;
mod doc;
mod git;
mod imagebed;
mod publish;
mod theme;
mod ui;
mod widgets;
mod workspace;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

use actions::*;
use app::InkdownApp;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            config::init(cx);
            theme::apply(cx);
            if cx.global::<config::AppConfig>().dark_mode {
                theme::set_mode(gpui_kit::component::ThemeMode::Dark, cx);
            }

            cx.bind_keys([
                KeyBinding::new("ctrl-n", NewDocument, None),
                KeyBinding::new("ctrl-o", OpenDocument, None),
                KeyBinding::new("ctrl-shift-o", OpenFolder, None),
                KeyBinding::new("ctrl-shift-p", PublishArticle, None),
                KeyBinding::new("ctrl-shift-g", GitSync, None),
                KeyBinding::new("ctrl-shift-a", AiWrite, None),
                KeyBinding::new("ctrl-s", SaveDocument, None),
                KeyBinding::new("ctrl-shift-s", SaveDocumentAs, None),
                KeyBinding::new("ctrl-shift-e", ExportHtml, None),
                KeyBinding::new("ctrl-b", FormatBold, None),
                KeyBinding::new("ctrl-i", FormatItalic, None),
                KeyBinding::new("ctrl-e", FormatInlineCode, None),
                KeyBinding::new("ctrl-k", FormatLink, None),
                KeyBinding::new("ctrl-shift-i", FormatImage, None),
                KeyBinding::new("ctrl-shift-t", ToggleTheme, None),
                KeyBinding::new("ctrl-shift-b", ToggleSidebar, None),
                KeyBinding::new("ctrl-1", ViewWrite, None),
                KeyBinding::new("ctrl-2", ViewSplit, None),
                KeyBinding::new("ctrl-3", ViewPreview, None),
                KeyBinding::new("escape", CloseOverlay, Some("Modal")),
            ]);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1360.), px(860.)), cx)),
                window_min_size: Some(size(px(900.), px(600.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Inkdown".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(9.))),
                }),
                ..TitleBar::window_options()
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| InkdownApp::new(window, cx))
            })
            .expect("failed to open Inkdown window");
        });
}
