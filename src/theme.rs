//! Inkdown's palette.
//!
//! GPUI Kit ships a neutral shadcn theme. Inkdown warms it into paper and ink:
//! ivory in the light, lamp-lit charcoal in the dark, with a seal-red accent
//! used sparingly for carets, links and the mark.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

fn ca(hex: u32) -> Hsla {
    rgba(hex).into()
}

/// Apply the palette for the theme's current mode.
pub fn apply(cx: &mut App) {
    Theme::update(cx, |theme| {
        theme.radius = px(8.);
        theme.radius_lg = px(12.);
        if theme.mode.is_dark() {
            apply_dark(theme);
        } else {
            apply_light(theme);
        }
    });
}

/// Switch light/dark and re-apply the palette on top of the new config.
pub fn set_mode(mode: ThemeMode, cx: &mut App) {
    if Theme::global(cx).mode == mode {
        return;
    }
    Theme::change(mode, None, cx);
    apply(cx);
    cx.refresh_windows();
}

pub fn toggle(cx: &mut App) {
    let next = if Theme::global(cx).mode.is_dark() {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    set_mode(next, cx);
}

fn apply_light(theme: &mut Theme) {
    theme.background = c(0xF6F2EA);
    theme.foreground = c(0x25221C);
    theme.border = c(0xE3DACB);
    theme.muted = c(0xEFE9DE);
    theme.muted_foreground = c(0x857B6B);
    theme.primary = c(0x2B2620);
    theme.primary_foreground = c(0xFCF8F0);
    theme.primary_hover = c(0x3B342B);
    theme.primary_active = c(0x17130E);
    theme.secondary = c(0xEAE2D5);
    theme.secondary_foreground = c(0x34302A);
    theme.secondary_hover = c(0xE1D7C7);
    theme.secondary_active = c(0xD7CBBA);
    theme.accent = c(0xF2E7D8);
    theme.accent_foreground = c(0x9E4526);
    theme.popover = c(0xFFFCF6);
    theme.popover_foreground = c(0x25221C);
    theme.button = c(0xEFE9DE);
    theme.button_foreground = c(0x2B2620);
    theme.button_hover = c(0xE7DFD2);
    theme.button_active = c(0xDDD3C3);
    theme.button_primary = c(0x2B2620);
    theme.button_primary_foreground = c(0xFCF8F0);
    theme.button_primary_hover = c(0x3B342B);
    theme.button_primary_active = c(0x17130E);
    theme.button_secondary = c(0xEAE2D5);
    theme.button_secondary_foreground = c(0x34302A);
    theme.button_secondary_hover = c(0xE1D7C7);
    theme.button_secondary_active = c(0xD7CBBA);
    theme.button_danger = c(0xB34A3A);
    theme.button_danger_foreground = c(0xFFF8F1);
    theme.button_danger_hover = c(0xC45A48);
    theme.button_danger_active = c(0x983628);
    theme.input = c(0xDCD2C2);
    theme.caret = c(0xB4552D);
    theme.link = c(0x9B4527);
    theme.link_hover = c(0xB85A35);
    theme.link_active = c(0x7C341A);
    theme.selection = ca(0xE7C3A980);
    theme.ring = c(0xC9A181);

    theme.colors.list = c(0xF6F2EA);
    theme.colors.list_hover = c(0xEFE8DC);
    theme.colors.list_active = c(0xEADCCB);
    theme.colors.list_active_border = c(0xC9A181);
    theme.colors.list_even = c(0xFAF6EF);
    theme.colors.list_head = c(0xF1EADF);

    theme.sidebar = c(0xF1EBE1);
    theme.sidebar_foreground = c(0x3A342B);
    theme.sidebar_accent = c(0xE8DFD0);
    theme.sidebar_accent_foreground = c(0x25221C);
    theme.sidebar_border = c(0xE1D7C8);
    theme.sidebar_primary = c(0xB4552D);
    theme.sidebar_primary_foreground = c(0xFFF9F2);

    theme.title_bar = c(0xF2ECE2);
    theme.title_bar_border = c(0xE1D7C8);
    theme.status_bar = c(0xF2ECE2);
    theme.status_bar_border = c(0xE1D7C8);

    theme.table = c(0xFFFCF6);
    theme.table_even = c(0xFAF6EF);
    theme.table_head = c(0xF1E9DD);
    theme.table_head_foreground = c(0x6B6152);
    theme.table_hover = c(0xF6F0E6);
    theme.table_row_border = c(0xE8DFD1);
    theme.table_active = c(0xF0E5D6);
    theme.table_active_border = c(0xC7A181);

    theme.group_box = c(0xF3EDE3);
    theme.group_box_foreground = c(0x3A342B);
    theme.scrollbar = ca(0x00000000);
    theme.scrollbar_thumb = ca(0xC4B7A380);
    theme.scrollbar_thumb_hover = ca(0xA99A85CC);
    theme.progress_bar = c(0xB4552D);
    theme.switch = c(0xDCD2C2);
    theme.switch_thumb = c(0xFFFDF8);
    theme.overlay = ca(0x1C171033);
    theme.skeleton = c(0xE8E0D4);
    theme.drop_target = ca(0xB4552D33);
    theme.drag_border = c(0xB4552D);
    theme.slider_bar = c(0xDCD2C2);
    theme.slider_thumb = c(0xFFFCF6);
    theme.accordion = c(0xF6F2EA);
    theme.description_list_label = c(0xEFE8DC);
    theme.description_list_label_foreground = c(0x6B6152);

    theme.chart_1 = c(0xC98B6B);
    theme.chart_2 = c(0x6E8C7B);
    theme.chart_3 = c(0xB4552D);
    theme.chart_4 = c(0x9A8A6E);
    theme.chart_5 = c(0x5E6F82);
    theme.chart_grid = ca(0x25221C1A);
}

fn apply_dark(theme: &mut Theme) {
    theme.background = c(0x14161B);
    theme.foreground = c(0xE9E2D6);
    theme.border = c(0x2A2C33);
    theme.muted = c(0x1C1F25);
    theme.muted_foreground = c(0x8E8A80);
    theme.primary = c(0xE9DFCE);
    theme.primary_foreground = c(0x1B1916);
    theme.primary_hover = c(0xF2E9DA);
    theme.primary_active = c(0xD8CCB8);
    theme.secondary = c(0x24272E);
    theme.secondary_foreground = c(0xDAD2C5);
    theme.secondary_hover = c(0x2C3038);
    theme.secondary_active = c(0x333842);
    theme.accent = c(0x2A2621);
    theme.accent_foreground = c(0xE7A47C);
    theme.popover = c(0x1A1D23);
    theme.popover_foreground = c(0xE9E2D6);
    theme.button = c(0x24272E);
    theme.button_foreground = c(0xDAD2C5);
    theme.button_hover = c(0x2C3038);
    theme.button_active = c(0x333842);
    theme.button_primary = c(0xE9DFCE);
    theme.button_primary_foreground = c(0x1B1916);
    theme.button_primary_hover = c(0xF2E9DA);
    theme.button_primary_active = c(0xD8CCB8);
    theme.button_secondary = c(0x24272E);
    theme.button_secondary_foreground = c(0xDAD2C5);
    theme.button_secondary_hover = c(0x2C3038);
    theme.button_secondary_active = c(0x333842);
    theme.button_danger = c(0xB35446);
    theme.button_danger_foreground = c(0xFFF8F1);
    theme.button_danger_hover = c(0xC46454);
    theme.button_danger_active = c(0x944033);
    theme.input = c(0x343841);
    theme.caret = c(0xE8A87C);
    theme.link = c(0xE8A87C);
    theme.link_hover = c(0xF2BC96);
    theme.link_active = c(0xD08C60);
    theme.selection = ca(0xB4552D66);
    theme.ring = c(0x8A5A3E);

    theme.colors.list = c(0x181B20);
    theme.colors.list_hover = c(0x20242B);
    theme.colors.list_active = c(0x2F2823);
    theme.colors.list_active_border = c(0x8A5A3E);
    theme.colors.list_even = c(0x1B1E24);
    theme.colors.list_head = c(0x20242B);

    theme.sidebar = c(0x101216);
    theme.sidebar_foreground = c(0xD8D1C4);
    theme.sidebar_accent = c(0x22262D);
    theme.sidebar_accent_foreground = c(0xEFE7DA);
    theme.sidebar_border = c(0x262A31);
    theme.sidebar_primary = c(0xE2D7C4);
    theme.sidebar_primary_foreground = c(0x1A1713);

    theme.title_bar = c(0x121419);
    theme.title_bar_border = c(0x272A31);
    theme.status_bar = c(0x121419);
    theme.status_bar_border = c(0x272A31);

    theme.table = c(0x1A1D23);
    theme.table_even = c(0x1D2026);
    theme.table_head = c(0x22262D);
    theme.table_head_foreground = c(0xA79F91);
    theme.table_hover = c(0x22262D);
    theme.table_row_border = c(0x2B2F37);
    theme.table_active = c(0x2F2823);
    theme.table_active_border = c(0x8A5A3E);

    theme.group_box = c(0x1B1E24);
    theme.group_box_foreground = c(0xD8D1C4);
    theme.scrollbar = ca(0x00000000);
    theme.scrollbar_thumb = ca(0x6A655B80);
    theme.scrollbar_thumb_hover = ca(0x8B8478CC);
    theme.progress_bar = c(0xE2A875);
    theme.switch = c(0x343841);
    theme.switch_thumb = c(0xE9E2D6);
    theme.overlay = ca(0x00000080);
    theme.skeleton = c(0x24272E);
    theme.drop_target = ca(0xE2A87533);
    theme.drag_border = c(0xE2A875);
    theme.slider_bar = c(0x343841);
    theme.slider_thumb = c(0xE9E2D6);
    theme.accordion = c(0x181B20);
    theme.description_list_label = c(0x1C1F25);
    theme.description_list_label_foreground = c(0xA79F91);

    theme.chart_1 = c(0xE2A875);
    theme.chart_2 = c(0x8FB59C);
    theme.chart_3 = c(0xD67C55);
    theme.chart_4 = c(0xB8A98B);
    theme.chart_5 = c(0x7E93AD);
    theme.chart_grid = ca(0xE9E2D61A);
}
