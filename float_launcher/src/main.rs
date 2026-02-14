#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod branding;
mod config;
mod events;
mod icons;
mod system;

use crate::app::{MyApp, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, WINDOW_HEIGHT, WINDOW_WIDTH};
use crate::branding::APP_DISPLAY_NAME;
use crate::config::AppConfig;
use eframe::egui;

fn main() -> eframe::Result<()> {
    let startup_size = load_startup_window_size();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(startup_size)
            .with_resizable(true)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_visible(true),
        ..Default::default()
    };

    eframe::run_native(
        APP_DISPLAY_NAME,
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            install_windows_font_fallback(&cc.egui_ctx);
            Ok(Box::new(MyApp::new(cc)))
        }),
    )
}

fn load_startup_window_size() -> [f32; 2] {
    let config = AppConfig::load();
    if let Some((w, h)) = config.last_size {
        [
            sanitize_dimension(w, WINDOW_WIDTH, MIN_WINDOW_WIDTH),
            sanitize_dimension(h, WINDOW_HEIGHT, MIN_WINDOW_HEIGHT),
        ]
    } else {
        [WINDOW_WIDTH, WINDOW_HEIGHT]
    }
}

fn sanitize_dimension(value: f32, fallback: f32, min: f32) -> f32 {
    if !value.is_finite() {
        return fallback;
    }
    value.clamp(min, 4096.0)
}

fn install_windows_font_fallback(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let font_candidates = [
        ("yahei", r"C:\Windows\Fonts\msyh.ttc"),
        ("yahei_ui", r"C:\Windows\Fonts\msyhbd.ttc"),
        ("simhei", r"C:\Windows\Fonts\simhei.ttf"),
    ];

    for (name, path) in font_candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.to_owned(), egui::FontData::from_owned(data).into());

            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, name.to_owned());
            }
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                family.push(name.to_owned());
            }
            break;
        }
    }

    ctx.set_fonts(fonts);
}
