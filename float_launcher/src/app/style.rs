use crate::config::WindowShape;
use eframe::egui::Color32;

pub const HEADER_HEIGHT: f32 = 28.0;
pub const ROW_HEIGHT: f32 = 46.0;
pub const CONTENT_PADDING: f32 = 9.0;
pub const ICON_SIDE: f32 = 20.0;
pub const DROP_SHADOW: f32 = 8.0;

#[derive(Clone, Copy)]
pub struct LauncherTheme {
    pub panel_bg_bottom: Color32,
    pub panel_border: Color32,
    pub panel_shadow: Color32,
    pub header_bg_bottom: Color32,
    pub title_color: Color32,
    pub row_bg: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub row_border: Color32,
    pub icon_placeholder: Color32,
    pub drop_hint: Color32,
    pub toast_bg: Color32,
    pub toast_text: Color32,
}

impl Default for LauncherTheme {
    fn default() -> Self {
        Self {
            panel_bg_bottom: Color32::from_rgba_premultiplied(14, 20, 31, 184),
            panel_border: Color32::from_rgba_premultiplied(161, 179, 201, 36),
            panel_shadow: Color32::from_rgba_premultiplied(3, 8, 16, 75),
            header_bg_bottom: Color32::from_rgba_premultiplied(21, 32, 48, 184),
            title_color: Color32::from_rgb(242, 248, 255),
            row_bg: Color32::from_rgba_premultiplied(24, 36, 50, 154),
            row_hover: Color32::from_rgba_premultiplied(35, 53, 74, 184),
            row_selected: Color32::from_rgba_premultiplied(45, 104, 114, 192),
            row_border: Color32::from_rgba_premultiplied(147, 169, 194, 78),
            icon_placeholder: Color32::from_rgba_premultiplied(205, 221, 238, 108),
            drop_hint: Color32::from_rgba_premultiplied(93, 214, 189, 186),
            toast_bg: Color32::from_rgba_premultiplied(8, 12, 18, 236),
            toast_text: Color32::from_rgb(245, 250, 255),
        }
    }
}

pub fn rounding(shape: WindowShape) -> f32 {
    match shape {
        WindowShape::Circle => 210.0,
        WindowShape::Square => 6.0,
        WindowShape::RoundedRect => 20.0,
    }
}
