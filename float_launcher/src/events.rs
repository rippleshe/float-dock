use eframe::egui;
use std::path::PathBuf;

#[derive(Debug)]
pub enum UserEvent {
    Show,
    Hide,
    Quit,
    IconReady(IconResult),
}

pub struct IconRequest {
    pub path: PathBuf,
    pub name_hint: Option<String>,
    pub size: u32,
}

#[derive(Debug)]
pub struct IconResult {
    pub path: PathBuf,
    pub image: Option<egui::ColorImage>,
}
