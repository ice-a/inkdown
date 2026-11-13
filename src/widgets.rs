//! Tiny shared presentation helpers.

use std::sync::{Arc, OnceLock};

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

fn logo_image() -> Arc<Image> {
    static LOGO: OnceLock<Arc<Image>> = OnceLock::new();
    LOGO.get_or_init(|| {
        Arc::new(Image::from_bytes(
            ImageFormat::Png,
            include_bytes!("../assets/icon/inkdown-64.png").to_vec(),
        ))
    })
    .clone()
}

/// The Inkdown mark in the title bar.
pub fn logo() -> impl IntoElement {
    img(ImageSource::from(logo_image()))
        .size(px(20.))
        .rounded(px(5.))
}

/// A one-pixel vertical rule for toolbars and title bars.
pub fn hairline(cx: &App) -> AnyElement {
    div()
        .w(px(1.))
        .h(px(16.))
        .mx_1()
        .flex_shrink_0()
        .bg(cx.theme().border)
        .into_any_element()
}

pub fn dot(color: Hsla) -> impl IntoElement {
    div().size(px(6.)).rounded_full().bg(color).flex_shrink_0()
}

/// 12345 -> "12,345"
pub fn number(n: usize) -> String {
    let digits = n.to_string();
    let len = digits.len();
    let mut out = String::with_capacity(len + len / 3);
    for (ix, ch) in digits.chars().enumerate() {
        if ix > 0 && (len - ix) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
