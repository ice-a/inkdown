// Windows GUI apps must not carry the console subsystem, otherwise Explorer
// opens a cmd window next to the app and closing it kills the process. This
// applies to debug builds too; use `cargo run --features console` when you
// need stdout/stderr in a terminal.
#![cfg_attr(all(windows, not(feature = "console")), windows_subsystem = "windows")]

mod actions;
mod ai;
mod app;
mod backup;
mod commands;
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
                KeyBinding::new("ctrl-shift-n", NewFromTemplate, None),
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
                KeyBinding::new("ctrl-f", PreviewFind, None),
                KeyBinding::new("ctrl-shift-m", EditMetadata, None),
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
                app_id: Some("inkdown".into()),
                titlebar: Some(TitlebarOptions {
                    title: Some("Inkdown".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(9.), px(9.))),
                }),
                ..TitleBar::window_options()
            };

            // The Windows icon is a PE resource (see assets/windows.rc) and the
            // macOS icon lives in the .app bundle. Only X11 exposes a runtime
            // window icon, so decode the PNG there.
            #[cfg(target_os = "linux")]
            let options = {
                let mut options = options;
                if let Ok(image) =
                    image::load_from_memory(include_bytes!("../assets/icon/inkdown-256.png"))
                {
                    options.icon = Some(std::sync::Arc::new(image.into_rgba8()));
                }
                options
            };

            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| InkdownApp::new(window, cx))
            })
            .expect("failed to open Inkdown window");
        });
}
