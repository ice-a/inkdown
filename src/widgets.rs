//! Tiny shared presentation helpers.

use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::*;

/// The seal-red mark in the title bar.
pub fn logo() -> impl IntoElement {
    let warm = Hsla::from(rgb(0xB4552D));
    let light = Hsla::from(rgb(0xD98E5F));
    div()
        .size(px(20.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .bg(linear_gradient(
            140.,
            linear_color_stop(warm, 0.),
            linear_color_stop(light, 1.),
        ))
        .child(
            Icon::new(IconName::Feather)
                .with_size(px(12.))
                .text_color(Hsla::from(rgb(0xFFF7EF))),
        )
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
